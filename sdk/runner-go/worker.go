package croniqrunner

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/jhump/protoreflect/dynamic"
)

type Config struct {
	BaseURL        string
	TenantID       string
	EnvironmentTag string
	ApiKey         string
	BearerToken    string
	HTTPClient     *http.Client
	Timeout        time.Duration
}

type Client struct {
	baseURL        *url.URL
	tenantID       string
	environmentTag string
	apiKey         string
	bearerToken    string
	httpClient     *http.Client
}

type Lease struct {
	ExecutionId       string    `json:"executionId"`
	LeaseId           string    `json:"leaseId"`
	TriggerId         string    `json:"triggerId"`
	JobKey            string    `json:"jobKey"`
	FireAtUtc         time.Time `json:"fireAtUtc"`
	LeaseExpiresAtUtc time.Time `json:"leaseExpiresAtUtc"`
	Payload           *string   `json:"payload,omitempty"`
	ExecutionMode     string    `json:"executionMode,omitempty"`
	InvocationSource  string    `json:"invocationSource,omitempty"`
}

type WorkEvent struct {
	Message      string            `json:"message"`
	Level        string            `json:"level,omitempty"`
	TimestampUtc *time.Time        `json:"timestampUtc,omitempty"`
	Properties   map[string]string `json:"properties,omitempty"`
	EventType    string            `json:"eventType,omitempty"`
}

type ApiError struct {
	StatusCode int
	Body       string
}

func (e *ApiError) Error() string
{
	return fmt.Sprintf("croniq api error: status=%d body=%s", e.StatusCode, e.Body)
}

func IsLeaseConflict(err error) bool
{
	var apiErr *ApiError
	if errors.As(err, &apiErr) && apiErr.StatusCode == http.StatusConflict {
		return true
	}
	return false
}

func IsLeaseNotFound(err error) bool
{
	var apiErr *ApiError
	if errors.As(err, &apiErr) && apiErr.StatusCode == http.StatusNotFound {
		return true
	}
	return false
}

func NewClient(cfg Config) (*Client, error)
{
	if strings.TrimSpace(cfg.BaseURL) == "" {
		return nil, errors.New("base url is required")
	}
	if strings.TrimSpace(cfg.TenantID) == "" {
		return nil, errors.New("tenant id is required")
	}
	if strings.TrimSpace(cfg.ApiKey) == "" && strings.TrimSpace(cfg.BearerToken) == "" {
		return nil, errors.New("api key or bearer token is required")
	}

	parsed, err := url.Parse(cfg.BaseURL)
	if err != nil {
		return nil, fmt.Errorf("invalid base url: %w", err)
	}

	client := cfg.HTTPClient
	if client == nil {
		timeout := cfg.Timeout
		if timeout <= 0 {
			timeout = 60 * time.Second
		}
		client = &http.Client{Timeout: timeout}
	}

	return &Client{
		baseURL:        parsed,
		tenantID:       strings.TrimSpace(cfg.TenantID),
		environmentTag: strings.TrimSpace(cfg.EnvironmentTag),
		apiKey:         strings.TrimSpace(cfg.ApiKey),
		bearerToken:    strings.TrimSpace(cfg.BearerToken),
		httpClient:     client,
	}, nil
}

func (c *Client) Poll(ctx context.Context, runnerId string, batchSize int, waitFor time.Duration) ([]Lease, error)
{
	options := PollOptions{
		BatchSize: batchSize,
		WaitFor:   waitFor,
	}
	return c.PollWithOptions(ctx, runnerId, options)
}

type PollOptions struct {
	BatchSize           int
	WaitFor             time.Duration
	AllowTestExecutions *bool
	MaxInflight         *int
	Capabilities        []string
}

func (c *Client) PollWithOptions(ctx context.Context, runnerId string, options PollOptions) ([]Lease, error)
{
	if strings.TrimSpace(runnerId) == "" {
		return nil, errors.New("runner id is required")
	}
	batchSize := options.BatchSize
	if batchSize <= 0 {
		batchSize = 1
	}

	request := pollRequest{
		RunnerId:  strings.TrimSpace(runnerId),
		BatchSize: &batchSize,
	}
	if options.AllowTestExecutions != nil {
		request.AllowTestExecutions = options.AllowTestExecutions
	}
	if options.MaxInflight != nil {
		request.MaxInflight = options.MaxInflight
	}
	if len(options.Capabilities) > 0 {
		request.Capabilities = options.Capabilities
	}

	if options.WaitFor > 0 {
		waitMs := int(options.WaitFor / time.Millisecond)
		request.WaitForMs = &waitMs
	}

	var response pollResponse
	if err := c.post(ctx, "/work/poll", request, &response); err != nil {
		return nil, err
	}

	return response.Leases, nil
}

func (c *Client) Renew(ctx context.Context, runnerId string, lease Lease) (*Lease, bool, error)
{
	if strings.TrimSpace(runnerId) == "" {
		return nil, false, errors.New("runner id is required")
	}

	request := renewRequest{
		RunnerId: strings.TrimSpace(runnerId),
		Lease:    lease,
	}

	var response renewResponse
	err := c.post(ctx, "/work/renew", request, &response)
	if err != nil {
		if IsLeaseNotFound(err) {
			return nil, false, nil
		}
		return nil, false, err
	}

	if !response.Renewed || response.Lease == nil {
		return nil, false, nil
	}

	return response.Lease, true, nil
}

func (c *Client) Ack(
	ctx context.Context,
	runnerId string,
	lease Lease,
	succeeded bool,
	nextFireTimeUtc *time.Time,
	deadLetterReason string,
) error
{
	if strings.TrimSpace(runnerId) == "" {
		return errors.New("runner id is required")
	}

	request := ackRequest{
		RunnerId:         strings.TrimSpace(runnerId),
		Lease:            lease,
		Succeeded:        succeeded,
		NextFireTimeUtc:  nextFireTimeUtc,
		DeadLetterReason: deadLetterReason,
	}

	return c.post(ctx, "/work/ack", request, nil)
}

func (c *Client) Events(ctx context.Context, runnerId string, lease Lease, events []WorkEvent) error
{
	if strings.TrimSpace(runnerId) == "" {
		return errors.New("runner id is required")
	}
	if strings.TrimSpace(lease.ExecutionId) == "" {
		return errors.New("execution id is required")
	}

	request := eventsRequest{
		RunnerId: strings.TrimSpace(runnerId),
		Lease:    lease,
		Events:   events,
	}

	path := fmt.Sprintf("/work/%s:events", url.PathEscape(lease.ExecutionId))
	return c.post(ctx, path, request, nil)
}

func (c *Client) post(ctx context.Context, path string, payload interface{}, out interface{}) error
{
	payloadBytes, err := json.Marshal(payload)
	if err != nil {
		return err
	}

	endpoint := *c.baseURL
	endpoint.Path = strings.TrimSuffix(endpoint.Path, "/") + "/tenants/" + url.PathEscape(c.tenantID) + path
	if c.environmentTag != "" {
		query := endpoint.Query()
		query.Set("environment", c.environmentTag)
		endpoint.RawQuery = query.Encode()
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint.String(), bytes.NewBuffer(payloadBytes))
	if err != nil {
		return err
	}

	req.Header.Set("Content-Type", "application/json")
	if c.bearerToken != "" {
		req.Header.Set("Authorization", "Bearer "+c.bearerToken)
	} else {
		req.Header.Set("X-Croniq-Key", c.apiKey)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 200 && resp.StatusCode < 300 {
		if out == nil {
			return nil
		}
		return json.NewDecoder(resp.Body).Decode(out)
	}

	body, _ := io.ReadAll(resp.Body)
	return &ApiError{StatusCode: resp.StatusCode, Body: string(body)}
}

type pollRequest struct {
	RunnerId            string   `json:"runnerId"`
	BatchSize           *int     `json:"batchSize,omitempty"`
	WaitForMs           *int     `json:"waitForMs,omitempty"`
	AllowTestExecutions *bool    `json:"allowTestExecutions,omitempty"`
	MaxInflight         *int     `json:"maxInflight,omitempty"`
	Capabilities        []string `json:"capabilities,omitempty"`
}

type pollResponse struct {
	Leases []Lease `json:"leases"`
}

type renewRequest struct {
	RunnerId string `json:"runnerId"`
	Lease    Lease  `json:"lease"`
}

type renewResponse struct {
	Renewed bool   `json:"renewed"`
	Lease   *Lease `json:"lease"`
}

type ackRequest struct {
	RunnerId         string     `json:"runnerId"`
	Lease            Lease      `json:"lease"`
	Succeeded        bool       `json:"succeeded"`
	NextFireTimeUtc  *time.Time `json:"nextFireTimeUtc,omitempty"`
	DeadLetterReason string     `json:"deadLetterReason,omitempty"`
}

type eventsRequest struct {
	RunnerId string      `json:"runnerId"`
	Lease    Lease       `json:"lease"`
	Events   []WorkEvent `json:"events"`
}

type TransportMode string

const (
	TransportAuto    TransportMode = "auto"
	TransportGrpc    TransportMode = "grpc"
	TransportPolling TransportMode = "polling"
)

type RunnerConfig struct {
	Config
	RunnerId            string
	TransportMode       TransportMode
	GrpcBaseURL         string
	AllowTestExecutions bool
	MaxInflight         int
	Capabilities        []string
	PollBatchSize       int
	PollWait            time.Duration
	RenewLead           time.Duration
	RetryBase           time.Duration
	RetryMax            time.Duration
	RetryMaxAttempts    int
	OutboxPath          string
	OutboxMaxEntries    int
	OutboxMaxBytes      int64
}

type ExecutionContext struct {
	ExecutionId     string
	LeaseId         string
	TriggerId       string
	JobKey          string
	FireAtUtc       time.Time
	LeaseExpiresAt  time.Time
	ExecutionMode   string
	InvocationSource string
	EmitEvent       func(events []WorkEvent) error
}

type RunnerLogger interface {
	Info(message string, fields map[string]any)
	Warn(message string, fields map[string]any)
	Error(message string, fields map[string]any)
}

type defaultRunnerLogger struct{}

func (l *defaultRunnerLogger) Info(message string, fields map[string]any)  { logWithFields("info", message, fields) }
func (l *defaultRunnerLogger) Warn(message string, fields map[string]any)  { logWithFields("warn", message, fields) }
func (l *defaultRunnerLogger) Error(message string, fields map[string]any) { logWithFields("error", message, fields) }

type ExecuteHandler func(ctx ExecutionContext, payload *string, logger RunnerLogger) error

type Runner struct {
	config  RunnerConfig
	client  *Client
	logger  RunnerLogger
	handler ExecuteHandler
	grpcConn *grpcRunnerConnection
	outbox  *outboxStore
}

func NewRunner(config RunnerConfig) (*Runner, error)
{
	if strings.TrimSpace(config.RunnerId) == "" {
		return nil, errors.New("runner id is required")
	}
	if config.TransportMode == "" {
		config.TransportMode = TransportAuto
	}
	if config.MaxInflight <= 0 {
		config.MaxInflight = 1
	}
	if config.PollBatchSize <= 0 {
		config.PollBatchSize = config.MaxInflight
	}
	if config.PollWait <= 0 {
		config.PollWait = 25 * time.Second
	}
	if config.RenewLead <= 0 {
		config.RenewLead = 10 * time.Second
	}
	if config.RetryBase <= 0 {
		config.RetryBase = 500 * time.Millisecond
	}
	if config.RetryMax <= 0 {
		config.RetryMax = 10 * time.Second
	}
	if config.OutboxMaxEntries <= 0 {
		config.OutboxMaxEntries = 500
	}
	if config.OutboxMaxBytes <= 0 {
		config.OutboxMaxBytes = 1_000_000
	}
	if strings.TrimSpace(config.OutboxPath) == "" {
		config.OutboxPath = ".croniq/runner-outbox.jsonl"
	}

	client, err := NewClient(config.Config)
	if err != nil {
		return nil, err
	}

	return &Runner{
		config: config,
		client: client,
		logger: &defaultRunnerLogger{},
		outbox: newOutboxStore(config.OutboxPath, config.OutboxMaxEntries, config.OutboxMaxBytes),
	}, nil
}

func (r *Runner) OnExecute(handler ExecuteHandler)
{
	r.handler = handler
}

func (r *Runner) Run(ctx context.Context) error
{
	if r.handler == nil {
		return errors.New("execute handler must be registered")
	}
	if r.config.TransportMode != TransportPolling {
		grpcConn, err := newGrpcRunnerConnection(r.config)
		if err != nil {
			return err
		}
		r.grpcConn = grpcConn
		grpcConn.start(ctx, func(lease Lease) {
			select {
			case <-ctx.Done():
				return
			case queue <- lease:
			}
		})
	}

	queue := make(chan Lease, r.config.MaxInflight*2)
	semaphore := make(chan struct{}, r.config.MaxInflight)

	if r.outbox != nil {
		r.outbox.Load()
		go r.replayOutboxLoop(ctx)
	}

	go r.pollLoop(ctx, queue)

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case lease := <-queue:
			semaphore <- struct{}{}
			go func(lease Lease) {
				defer func() { <-semaphore }()
				r.runLease(ctx, lease)
			}(lease)
		}
	}
}

func (r *Runner) pollLoop(ctx context.Context, queue chan<- Lease)
{
	attempt := 0
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		if r.config.TransportMode == TransportAuto && r.grpcConn != nil && r.grpcConn.isConnected() {
			time.Sleep(250 * time.Millisecond)
			continue
		}

		options := PollOptions{
			BatchSize:           r.config.PollBatchSize,
			WaitFor:             r.config.PollWait,
			AllowTestExecutions: boolPtr(r.config.AllowTestExecutions),
			MaxInflight:         intPtr(r.config.MaxInflight),
			Capabilities:        r.config.Capabilities,
		}

		leases, err := r.client.PollWithOptions(ctx, r.config.RunnerId, options)
		if err != nil {
			attempt++
			delay := nextDelay(r.config.RetryBase, r.config.RetryMax, attempt)
			r.logger.Warn("poll failed", map[string]any{
				"error":   err.Error(),
				"delayMs": delay.Milliseconds(),
			})
			time.Sleep(delay)
			continue
		}
		attempt = 0

		for _, lease := range leases {
			queue <- lease
		}
	}
}

func (r *Runner) runLease(ctx context.Context, lease Lease)
{
	if !r.config.AllowTestExecutions && strings.EqualFold(lease.ExecutionMode, "test") {
		if r.grpcConn != nil && r.grpcConn.isConnected() {
			_ = r.grpcConn.send(buildAckFailureMessage(lease, "test-not-allowed", "test executions are disabled for this runner", "test-not-allowed"))
		} else {
			_ = r.client.Ack(ctx, r.config.RunnerId, lease, false, nil, "test-not-allowed")
		}
		return
	}

	renewCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	go r.renewLoop(renewCtx, lease)

	ctxPayload := ExecutionContext{
		ExecutionId:     lease.ExecutionId,
		LeaseId:         lease.LeaseId,
		TriggerId:       lease.TriggerId,
		JobKey:          lease.JobKey,
		FireAtUtc:       lease.FireAtUtc,
		LeaseExpiresAt:  lease.LeaseExpiresAtUtc,
		ExecutionMode:   lease.ExecutionMode,
		InvocationSource: lease.InvocationSource,
		EmitEvent: func(events []WorkEvent) error {
			return r.sendEvents(ctx, lease, events, true)
		},
	}

	if err := r.handler(ctxPayload, lease.Payload, r.logger); err != nil {
		if r.grpcConn != nil && r.grpcConn.isConnected() {
			_ = r.grpcConn.send(buildAckFailureMessage(lease, "execution-failed", err.Error(), "execution-failed"))
			return
		}
		if err := r.client.Ack(ctx, r.config.RunnerId, lease, false, nil, "execution-failed"); err != nil {
			r.enqueueOutboxAckFailure(lease, "execution-failed", err.Error(), "execution-failed")
		}
		return
	}

	if r.grpcConn != nil && r.grpcConn.isConnected() {
		_ = r.grpcConn.send(buildAckSuccessMessage(lease))
		return
	}
	if err := r.client.Ack(ctx, r.config.RunnerId, lease, true, nil, ""); err != nil {
		r.enqueueOutboxAckSuccess(lease)
	}
}

func (r *Runner) renewLoop(ctx context.Context, lease Lease)
{
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		refreshAt := lease.LeaseExpiresAtUtc.Add(-r.config.RenewLead)
		delay := time.Until(refreshAt)
		if delay < time.Second {
			delay = time.Second
		}
		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}

		updated, renewed, err := r.client.Renew(ctx, r.config.RunnerId, lease)
		if err != nil {
			r.logger.Warn("renew failed", map[string]any{"error": err.Error(), "leaseId": lease.LeaseId})
			continue
		}
		if !renewed || updated == nil {
			return
		}
		lease = *updated
	}
}

func boolPtr(value bool) *bool { return &value }
func intPtr(value int) *int    { return &value }

func logWithFields(level string, message string, fields map[string]any)
{
	if fields == nil {
		fields = map[string]any{}
	}
	fields["level"] = level
	fields["message"] = message
	fmt.Printf("%s %v\n", level, fields)
}

type outboxAckSuccessPayload struct {
	Lease Lease `json:"lease"`
}

type outboxAckFailurePayload struct {
	Lease           Lease  `json:"lease"`
	ErrorType       string `json:"error_type"`
	ErrorMessage    string `json:"error_message"`
	DeadLetterReason string `json:"dead_letter_reason"`
}

type outboxEventsPayload struct {
	Lease  Lease      `json:"lease"`
	Events []WorkEvent `json:"events"`
}

func (r *Runner) enqueueOutboxAckSuccess(lease Lease) {
	if r.outbox == nil {
		return
	}
	payload, _ := json.Marshal(outboxAckSuccessPayload{Lease: lease})
	r.outbox.Enqueue(outboxEntry{ID: fmt.Sprintf("%d", time.Now().UnixNano()), Type: "ack_success", Payload: payload})
}

func (r *Runner) enqueueOutboxAckFailure(lease Lease, errorType string, message string, reason string) {
	if r.outbox == nil {
		return
	}
	payload, _ := json.Marshal(outboxAckFailurePayload{Lease: lease, ErrorType: errorType, ErrorMessage: message, DeadLetterReason: reason})
	r.outbox.Enqueue(outboxEntry{ID: fmt.Sprintf("%d", time.Now().UnixNano()), Type: "ack_failure", Payload: payload})
}

func (r *Runner) enqueueOutboxEvents(lease Lease, events []WorkEvent) {
	if r.outbox == nil {
		return
	}
	payload, _ := json.Marshal(outboxEventsPayload{Lease: lease, Events: events})
	r.outbox.Enqueue(outboxEntry{ID: fmt.Sprintf("%d", time.Now().UnixNano()), Type: "events", Payload: payload})
}

func (r *Runner) sendEvents(ctx context.Context, lease Lease, events []WorkEvent, allowOutbox bool) error {
	if r.grpcConn != nil && r.grpcConn.isConnected() {
		msg := buildEventsMessage(lease, events)
		if msg != nil {
			return r.grpcConn.send(msg)
		}
	}
	if err := r.client.Events(ctx, r.config.RunnerId, lease, events); err != nil {
		if allowOutbox {
			r.enqueueOutboxEvents(lease, events)
		}
		return err
	}
	return nil
}

func (r *Runner) replayOutboxLoop(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}
		if r.outbox == nil {
			time.Sleep(time.Second)
			continue
		}
		items := r.outbox.Items()
		if len(items) == 0 {
			time.Sleep(time.Second)
			continue
		}
		for _, entry := range items {
			switch entry.Type {
			case "ack_success":
				var payload outboxAckSuccessPayload
				if err := json.Unmarshal(entry.Payload, &payload); err == nil {
					if err := r.client.Ack(ctx, r.config.RunnerId, payload.Lease, true, nil, ""); err == nil {
						r.outbox.Remove(entry.ID)
					} else {
						r.outbox.MarkFailed(entry.ID)
					}
				}
			case "ack_failure":
				var payload outboxAckFailurePayload
				if err := json.Unmarshal(entry.Payload, &payload); err == nil {
					if err := r.client.Ack(ctx, r.config.RunnerId, payload.Lease, false, nil, payload.DeadLetterReason); err == nil {
						r.outbox.Remove(entry.ID)
					} else {
						r.outbox.MarkFailed(entry.ID)
					}
				}
			case "events":
				var payload outboxEventsPayload
				if err := json.Unmarshal(entry.Payload, &payload); err == nil {
					if err := r.client.Events(ctx, r.config.RunnerId, payload.Lease, payload.Events); err == nil {
						r.outbox.Remove(entry.ID)
					} else {
						r.outbox.MarkFailed(entry.ID)
					}
				}
			default:
				r.outbox.Remove(entry.ID)
			}
		}
	}
}

func buildAckSuccessMessage(lease Lease) *dynamic.Message
{
	_, method, err := loadRunnerService()
	if err != nil {
		return nil
	}
	msg := dynamic.NewMessage(method.GetInputType())
	ack := dynamic.NewMessage(method.GetInputType().FindFieldByName("ack_success").GetMessageType())
	ack.SetFieldByName("execution_id", lease.ExecutionId)
	ack.SetFieldByName("lease_id", lease.LeaseId)
	msg.SetFieldByName("ack_success", ack)
	return msg
}

func buildAckFailureMessage(lease Lease, errorType string, message string, reason string) *dynamic.Message
{
	_, method, err := loadRunnerService()
	if err != nil {
		return nil
	}
	msg := dynamic.NewMessage(method.GetInputType())
	ack := dynamic.NewMessage(method.GetInputType().FindFieldByName("ack_failure").GetMessageType())
	ack.SetFieldByName("execution_id", lease.ExecutionId)
	ack.SetFieldByName("lease_id", lease.LeaseId)
	ack.SetFieldByName("error_type", errorType)
	ack.SetFieldByName("error_message", message)
	ack.SetFieldByName("dead_letter_reason", reason)
	msg.SetFieldByName("ack_failure", ack)
	return msg
}

func buildEventsMessage(lease Lease, events []WorkEvent) *dynamic.Message
{
	_, method, err := loadRunnerService()
	if err != nil {
		return nil
	}
	msg := dynamic.NewMessage(method.GetInputType())
	entries := dynamic.NewMessage(method.GetInputType().FindFieldByName("events").GetMessageType())
	entries.SetFieldByName("execution_id", lease.ExecutionId)
	entries.SetFieldByName("lease_id", lease.LeaseId)
	entryType := entries.GetMessageDescriptor().FindFieldByName("events").GetMessageType()
	list := make([]*dynamic.Message, 0, len(events))
	for _, event := range events {
		item := dynamic.NewMessage(entryType)
		item.SetFieldByName("message", event.Message)
		if event.Level != "" {
			item.SetFieldByName("level", event.Level)
		}
		if event.EventType != "" {
			item.SetFieldByName("event_type", event.EventType)
		}
		if event.TimestampUtc != nil {
			item.SetFieldByName("timestamp_utc", event.TimestampUtc.UnixMilli())
		}
		if len(event.Properties) > 0 {
			item.SetFieldByName("properties", event.Properties)
		}
		list = append(list, item)
	}
	entries.SetFieldByName("events", list)
	msg.SetFieldByName("events", entries)
	return msg
}
