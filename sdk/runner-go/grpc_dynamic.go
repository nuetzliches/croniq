package croniqrunner

import (
	"context"
	"embed"
	"errors"
	"fmt"
	"io"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/jhump/protoreflect/desc"
	"github.com/jhump/protoreflect/desc/protoparse"
	"github.com/jhump/protoreflect/dynamic"
	"github.com/jhump/protoreflect/dynamic/grpcdynamic"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

//go:embed protos/runner.proto
var runnerProtoFS embed.FS

var (
	runnerService     *desc.ServiceDescriptor
	runnerConnect      *desc.MethodDescriptor
	runnerServiceOnce  sync.Once
	runnerServiceError error
)

func loadRunnerService() (*desc.ServiceDescriptor, *desc.MethodDescriptor, error)
{
	runnerServiceOnce.Do(func() {
		parser := protoparse.Parser{
			Accessor: func(filename string) (io.ReadCloser, error) {
				if filename != "runner.proto" {
					return nil, fmt.Errorf("unknown proto: %s", filename)
				}
				file, err := runnerProtoFS.Open("protos/runner.proto")
				if err != nil {
					return nil, err
				}
				return file, nil
			},
		}
		files, err := parser.ParseFiles("runner.proto")
		if err != nil {
			runnerServiceError = err
			return
		}
		service := files[0].FindService("croniq.rpc.Runner")
		if service == nil {
			runnerServiceError = errors.New("Runner service not found in proto")
			return
		}
		method := service.FindMethodByName("Connect")
		if method == nil {
			runnerServiceError = errors.New("Runner.Connect method not found in proto")
			return
		}
		runnerService = service
		runnerConnect = method
	})

	if runnerServiceError != nil {
		return nil, nil, runnerServiceError
	}
	return runnerService, runnerConnect, nil
}

type grpcRunnerConnection struct {
	endpoint            string
	useTLS              bool
	runnerId            string
	apiKey              string
	bearerToken         string
	allowTestExecutions bool
	maxInflight         int
	capabilities        []string
	retryBase           time.Duration
	retryMax            time.Duration
	retryMaxAttempts    int

	mu        sync.Mutex
	connected bool
	stream    *grpcdynamic.BidiStream
}

func newGrpcRunnerConnection(config RunnerConfig) (*grpcRunnerConnection, error)
{
	endpoint := config.GrpcBaseURL
	if endpoint == "" {
		endpoint = config.BaseURL
	}

	parsed, err := url.Parse(endpoint)
	if err != nil {
		return nil, fmt.Errorf("invalid grpc base url: %w", err)
	}

	return &grpcRunnerConnection{
		endpoint:            parsed.Host,
		useTLS:              strings.EqualFold(parsed.Scheme, "https"),
		runnerId:            config.RunnerId,
		apiKey:              config.ApiKey,
		bearerToken:         config.BearerToken,
		allowTestExecutions: config.AllowTestExecutions,
		maxInflight:         config.MaxInflight,
		capabilities:        config.Capabilities,
		retryBase:           config.RetryBase,
		retryMax:            config.RetryMax,
		retryMaxAttempts:    config.RetryMaxAttempts,
	}, nil
}

func (c *grpcRunnerConnection) isConnected() bool
{
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.connected
}

func (c *grpcRunnerConnection) start(ctx context.Context, onAssigned func(Lease))
{
	go c.connectLoop(ctx, onAssigned)
}

func (c *grpcRunnerConnection) send(message *dynamic.Message) error
{
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.stream == nil {
		return errors.New("grpc stream is not connected")
	}
	return c.stream.SendMsg(message)
}

func (c *grpcRunnerConnection) connectLoop(ctx context.Context, onAssigned func(Lease))
{
	attempt := 0
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		if err := c.connectOnce(ctx, onAssigned); err != nil {
			attempt++
			if c.retryMaxAttempts > 0 && attempt >= c.retryMaxAttempts {
				return
			}
			delay := nextDelay(c.retryBase, c.retryMax, attempt)
			time.Sleep(delay)
			continue
		}
		attempt = 0
	}
}

func (c *grpcRunnerConnection) connectOnce(ctx context.Context, onAssigned func(Lease)) error
{
	_, method, err := loadRunnerService()
	if err != nil {
		return err
	}

	creds := insecure.NewCredentials()
	if c.useTLS {
		creds = credentials.NewTLS(nil)
	}

	conn, err := grpc.DialContext(ctx, c.endpoint, grpc.WithTransportCredentials(creds))
	if err != nil {
		return err
	}
	defer conn.Close()

	md := metadata.New(map[string]string{})
	if c.bearerToken != "" {
		md.Set("authorization", "Bearer "+c.bearerToken)
	} else if c.apiKey != "" {
		md.Set("x-croniq-key", c.apiKey)
	}
	ctx = metadata.NewOutgoingContext(ctx, md)

	stub := grpcdynamic.NewStub(conn)
	stream, err := stub.InvokeRpcBidiStream(ctx, method)
	if err != nil {
		return err
	}

	hello := dynamic.NewMessage(method.GetInputType())
	helloPayload := dynamic.NewMessage(method.GetInputType().FindFieldByName("hello").GetMessageType())
	helloPayload.SetFieldByName("runner_id", c.runnerId)
	helloPayload.SetFieldByName("max_inflight", int32(c.maxInflight))
	helloPayload.SetFieldByName("allow_test_executions", c.allowTestExecutions)
	capabilities := map[string]string{}
	for _, entry := range c.capabilities {
		value := strings.TrimSpace(entry)
		if value != "" {
			capabilities[value] = "true"
		}
	}
	helloPayload.SetFieldByName("capabilities", capabilities)
	hello.SetFieldByName("hello", helloPayload)
	if err := stream.SendMsg(hello); err != nil {
		return err
	}

	c.mu.Lock()
	c.stream = stream
	c.connected = true
	c.mu.Unlock()

	for {
		msg, err := stream.RecvMsg()
		if err != nil {
			c.mu.Lock()
			c.connected = false
			c.stream = nil
			c.mu.Unlock()
			return err
		}
		current, ok := msg.(*dynamic.Message)
		if !ok {
			continue
		}
		assigned := current.GetFieldByName("assigned")
		if assignedMsg, ok := assigned.(*dynamic.Message); ok {
			lease, err := leaseFromDynamic(assignedMsg)
			if err == nil {
				onAssigned(lease)
			}
		}
	}
}

func leaseFromDynamic(message *dynamic.Message) (Lease, error)
{
	executionId, _ := message.TryGetFieldByName("execution_id")
	leaseId, _ := message.TryGetFieldByName("lease_id")
	triggerId, _ := message.TryGetFieldByName("trigger_id")
	jobKey, _ := message.TryGetFieldByName("job_key")
	fireAt, _ := message.TryGetFieldByName("fire_at_utc")
	expiresAt, _ := message.TryGetFieldByName("lease_expires_at_utc")
	payload, _ := message.TryGetFieldByName("payload")
	execMode, _ := message.TryGetFieldByName("execution_mode")
	invocation, _ := message.TryGetFieldByName("invocation_source")

	fireAtMs, ok := fireAt.(int64)
	if !ok {
		fireAtMs = 0
	}
	expiresAtMs, ok := expiresAt.(int64)
	if !ok {
		expiresAtMs = 0
	}

	payloadStr := ""
	if payload != nil {
		payloadStr, _ = payload.(string)
	}

	executionMode := ""
	if execMode != nil {
		executionMode, _ = execMode.(string)
	}
	invocationSource := ""
	if invocation != nil {
		invocationSource, _ = invocation.(string)
	}

	var payloadPtr *string
	if payloadStr != "" {
		payloadPtr = &payloadStr
	}

	return Lease{
		ExecutionId:       fmt.Sprintf("%v", executionId),
		LeaseId:           fmt.Sprintf("%v", leaseId),
		TriggerId:         fmt.Sprintf("%v", triggerId),
		JobKey:            fmt.Sprintf("%v", jobKey),
		FireAtUtc:         time.UnixMilli(fireAtMs),
		LeaseExpiresAtUtc: time.UnixMilli(expiresAtMs),
		Payload:           payloadPtr,
		ExecutionMode:     executionMode,
		InvocationSource:  invocationSource,
	}, nil
}

func nextDelay(base time.Duration, max time.Duration, attempt int) time.Duration
{
	if attempt < 1 {
		attempt = 1
	}
	factor := time.Duration(1 << (attempt - 1))
	delay := base * factor
	if delay > max {
		delay = max
	}
	jitter := time.Duration(float64(delay) * 0.2)
	return delay + time.Duration(randInt63n(int64(jitter)))
}

func randInt63n(max int64) int64
{
	if max <= 0 {
		return 0
	}
	return time.Now().UnixNano() % max
}
