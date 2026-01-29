package croniqrunner

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"sync"
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

type RunnerJobRegistration struct {
	Description string            `json:"description,omitempty"`
	Metadata    map[string]string `json:"metadata,omitempty"`
}

type ApiError struct {
	StatusCode int
	Body       string
}

func (e *ApiError) Error() string {
	return fmt.Sprintf("croniq api error: status=%d body=%s", e.StatusCode, e.Body)
}

type RunnerMismatchError struct {
	Body string
}

func (e *RunnerMismatchError) Error() string {
	return fmt.Sprintf("runner mismatch: %s", e.Body)
}

type RunnerIdInUseError struct {
	Body string
}

func (e *RunnerIdInUseError) Error() string {
	return fmt.Sprintf("runner id in use: %s", e.Body)
}

type RunnerJobRegistrationDeniedError struct {
	Body string
}

func (e *RunnerJobRegistrationDeniedError) Error() string {
	return fmt.Sprintf("runner registration denied: %s", e.Body)
}

func IsLeaseConflict(err error) bool {
	var apiErr *ApiError
	if errors.As(err, &apiErr) && apiErr.StatusCode == http.StatusConflict {
		return true
	}
	return false
}

func IsLeaseNotFound(err error) bool {
	var apiErr *ApiError
	if errors.As(err, &apiErr) && apiErr.StatusCode == http.StatusNotFound {
		return true
	}
	return false
}

func IsRunnerMismatch(err error) bool {
	var mismatch *RunnerMismatchError
	return errors.As(err, &mismatch)
}

func IsRunnerIdInUse(err error) bool {
	var inUse *RunnerIdInUseError
	return errors.As(err, &inUse)
}

func IsRunnerJobRegistrationDenied(err error) bool {
	var denied *RunnerJobRegistrationDeniedError
	return errors.As(err, &denied)
}

func NewClient(cfg Config) (*Client, error) {
	if strings.TrimSpace(cfg.BaseURL) == "" {
		return nil, errors.New("base url is required")
	}
	if strings.TrimSpace(cfg.TenantID) == "" {
		return nil, errors.New("tenant id is required")
	}
	apiKey := strings.TrimSpace(cfg.ApiKey)
	bearer := strings.TrimSpace(cfg.BearerToken)
	if (apiKey == "" && bearer == "") || (apiKey != "" && bearer != "") {
		return nil, errors.New("api key or bearer token is required (but not both)")
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

func (c *Client) Poll(ctx context.Context, runnerId string, batchSize int, waitFor time.Duration) ([]Lease, error) {
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
	RunnerInstanceId    string
}

func (c *Client) PollWithOptions(ctx context.Context, runnerId string, options PollOptions) ([]Lease, error) {
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
	if strings.TrimSpace(options.RunnerInstanceId) != "" {
		request.RunnerInstanceId = strings.TrimSpace(options.RunnerInstanceId)
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

func (c *Client) Renew(ctx context.Context, runnerId string, lease Lease) (*Lease, bool, error) {
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
) error {
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

func (c *Client) Events(ctx context.Context, runnerId string, lease Lease, events []WorkEvent) error {
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

func (c *Client) Heartbeat(ctx context.Context, runnerId string, environmentTag string, metadataJson string, seenAtUtc *time.Time) error {
	return c.HeartbeatWithInstance(ctx, runnerId, "", environmentTag, metadataJson, seenAtUtc)
}

func (c *Client) HeartbeatWithInstance(ctx context.Context, runnerId string, runnerInstanceId string, environmentTag string, metadataJson string, seenAtUtc *time.Time) error {
	if strings.TrimSpace(runnerId) == "" {
		return errors.New("runner id is required")
	}
	if strings.TrimSpace(environmentTag) == "" {
		return errors.New("environment tag is required")
	}

	request := heartbeatRequest{
		EnvironmentTag: strings.TrimSpace(environmentTag),
		RunnerId:       strings.TrimSpace(runnerId),
		SeenAtUtc:      seenAtUtc,
		MetadataJson:   metadataJson,
	}
	if strings.TrimSpace(runnerInstanceId) != "" {
		request.RunnerInstanceId = strings.TrimSpace(runnerInstanceId)
	}

	return c.post(ctx, "/runners/heartbeat", request, nil)
}

func (c *Client) RegisterJob(ctx context.Context, request runnerJobRegistrationRequest) (*jobRegistrationResponse, error) {
	if strings.TrimSpace(request.RunnerId) == "" {
		return nil, errors.New("runner id is required")
	}
	if strings.TrimSpace(request.EnvironmentTag) == "" {
		return nil, errors.New("environment tag is required")
	}
	if strings.TrimSpace(request.JobKey) == "" {
		return nil, errors.New("job key is required")
	}

	var response jobRegistrationResponse
	if err := c.post(ctx, "/jobs:register", request, &response); err != nil {
		return nil, err
	}
	return &response, nil
}

func (c *Client) post(ctx context.Context, path string, payload interface{}, out interface{}) error {
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
	if resp.StatusCode == http.StatusForbidden && isRunnerMismatchBody(string(body)) {
		return &RunnerMismatchError{Body: string(body)}
	}
	if resp.StatusCode == http.StatusForbidden && isRunnerRegistrationDeniedBody(string(body)) {
		return &RunnerJobRegistrationDeniedError{Body: string(body)}
	}
	if resp.StatusCode == http.StatusConflict && isRunnerIdInUseBody(string(body)) {
		return &RunnerIdInUseError{Body: string(body)}
	}
	return &ApiError{StatusCode: resp.StatusCode, Body: string(body)}
}

func isRunnerMismatchBody(body string) bool {
	if strings.Contains(strings.ToLower(body), "runner-mismatch") {
		return true
	}
	var payload map[string]any
	if err := json.Unmarshal([]byte(body), &payload); err != nil {
		return false
	}
	if title, ok := payload["title"].(string); ok && strings.EqualFold(title, "runner-mismatch") {
		return true
	}
	if errValue, ok := payload["error"].(string); ok && strings.EqualFold(errValue, "runner-mismatch") {
		return true
	}
	return false
}

func isRunnerIdInUseBody(body string) bool {
	if strings.Contains(strings.ToLower(body), "runner-id-in-use") {
		return true
	}
	var payload map[string]any
	if err := json.Unmarshal([]byte(body), &payload); err != nil {
		return false
	}
	if title, ok := payload["title"].(string); ok && strings.EqualFold(title, "runner-id-in-use") {
		return true
	}
	if errValue, ok := payload["error"].(string); ok && strings.EqualFold(errValue, "runner-id-in-use") {
		return true
	}
	return false
}

func isRunnerRegistrationDeniedBody(body string) bool {
	if strings.Contains(strings.ToLower(body), "runner-registration-denied") {
		return true
	}
	var payload map[string]any
	if err := json.Unmarshal([]byte(body), &payload); err != nil {
		return false
	}
	if title, ok := payload["title"].(string); ok && strings.EqualFold(title, "runner-registration-denied") {
		return true
	}
	if errValue, ok := payload["error"].(string); ok && strings.EqualFold(errValue, "runner-registration-denied") {
		return true
	}
	return false
}

type pollRequest struct {
	RunnerId            string   `json:"runnerId"`
	RunnerInstanceId    string   `json:"runnerInstanceId,omitempty"`
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

type heartbeatRequest struct {
	EnvironmentTag string     `json:"environmentTag"`
	RunnerId       string     `json:"runnerId"`
	RunnerInstanceId string   `json:"runnerInstanceId,omitempty"`
	SeenAtUtc      *time.Time `json:"seenAtUtc,omitempty"`
	MetadataJson   string     `json:"metadataJson,omitempty"`
}

type runnerJobRegistrationRequest struct {
	EnvironmentTag  string            `json:"environmentTag"`
	RunnerId        string            `json:"runnerId"`
	RunnerInstanceId string           `json:"runnerInstanceId,omitempty"`
	JobKey          string            `json:"jobKey"`
	Description     string            `json:"description,omitempty"`
	Metadata        map[string]string `json:"metadata,omitempty"`
}

type jobRegistrationResponse struct {
	JobKey   string `json:"jobKey"`
	IsActive bool   `json:"isActive"`
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
	RunnerInstanceId    string
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
	HeartbeatInterval   time.Duration
	HeartbeatMetadata   map[string]any
	RegisterJobs        *bool
	OutboxPath          string
	OutboxMaxEntries    int
	OutboxMaxBytes      int64
}

func LoadRunnerConfigFromEnv() (RunnerConfig, error) {
	baseURL, err := requiredEnv("CRONIQ_API_BASEURL")
	if err != nil {
		return RunnerConfig{}, err
	}
	tenantID, err := requiredEnv("CRONIQ_TENANT_ID")
	if err != nil {
		return RunnerConfig{}, err
	}
	environmentTag, err := requiredEnv("CRONIQ_ENVIRONMENT")
	if err != nil {
		return RunnerConfig{}, err
	}
	runnerID, err := requiredEnv("CRONIQ_RUNNER_ID")
	if err != nil {
		return RunnerConfig{}, err
	}
	runnerInstanceID := getOptionalEnv("CRONIQ_RUNNER_INSTANCE_ID")
	if runnerInstanceID == "" {
		runnerInstanceID = generateRunnerInstanceId()
	}

	apiKey := strings.TrimSpace(os.Getenv("CRONIQ_API_KEY"))
	bearerToken := strings.TrimSpace(os.Getenv("CRONIQ_BEARER_TOKEN"))
	if (apiKey == "" && bearerToken == "") || (apiKey != "" && bearerToken != "") {
		return RunnerConfig{}, errors.New("set exactly one of CRONIQ_API_KEY or CRONIQ_BEARER_TOKEN")
	}

	transportMode := TransportMode(strings.ToLower(strings.TrimSpace(getOptionalEnv("CRONIQ_TRANSPORT_MODE"))))
	if transportMode == "" {
		transportMode = TransportAuto
	}
	if transportMode != TransportAuto && transportMode != TransportGrpc && transportMode != TransportPolling {
		return RunnerConfig{}, errors.New("CRONIQ_TRANSPORT_MODE must be auto, grpc, or polling")
	}

	registerJobs := true
	if strings.TrimSpace(os.Getenv("CRONIQ_RUNNER_REGISTER_JOBS")) != "" {
		registerJobs = parseBoolEnv("CRONIQ_RUNNER_REGISTER_JOBS")
	}

	allowTests := parseBoolEnv("CRONIQ_ALLOW_TEST_EXECUTIONS")
	maxInflight := parseIntEnv("CRONIQ_MAX_INFLIGHT", 1)
	capabilities := parseListEnv("CRONIQ_CAPABILITIES")
	batchSize := parseIntEnv("CRONIQ_POLL_BATCH_SIZE", 1)
	pollWait := time.Duration(parseIntEnv("CRONIQ_POLL_WAIT_MS", 25000)) * time.Millisecond
	requestTimeout := time.Duration(parseIntEnv("CRONIQ_REQUEST_TIMEOUT_MS", 60000)) * time.Millisecond
	renewLead := time.Duration(parseIntEnv("CRONIQ_RENEW_LEAD_MS", 10000)) * time.Millisecond
	retryBase := time.Duration(parseIntEnv("CRONIQ_RETRY_BASE_MS", 500)) * time.Millisecond
	retryMax := time.Duration(parseIntEnv("CRONIQ_RETRY_MAX_MS", 10000)) * time.Millisecond
	retryMaxAttempts := parseOptionalIntEnv("CRONIQ_RETRY_MAX_ATTEMPTS")
	grpcBaseURL := getOptionalEnv("CRONIQ_GRPC_BASEURL")

	return RunnerConfig{
		Config: Config{
			BaseURL:        baseURL,
			TenantID:       tenantID,
			EnvironmentTag: environmentTag,
			ApiKey:         apiKey,
			BearerToken:    bearerToken,
			Timeout:        requestTimeout,
		},
		RunnerId:            runnerID,
		RunnerInstanceId:    runnerInstanceID,
		TransportMode:       transportMode,
		GrpcBaseURL:         grpcBaseURL,
		AllowTestExecutions: allowTests,
		MaxInflight:         maxInflight,
		Capabilities:        capabilities,
		PollBatchSize:       batchSize,
		PollWait:            pollWait,
		RenewLead:           renewLead,
		RetryBase:           retryBase,
		RetryMax:            retryMax,
		RetryMaxAttempts:    retryMaxAttempts,
		RegisterJobs:        boolPtr(registerJobs),
	}, nil
}

func requiredEnv(key string) (string, error) {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return "", fmt.Errorf("%s is required", key)
	}
	return value, nil
}

func getOptionalEnv(key string) string {
	return strings.TrimSpace(os.Getenv(key))
}

func parseBoolEnv(key string) bool {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return false
	}
	switch strings.ToLower(value) {
	case "1", "true", "yes":
		return true
	case "0", "false", "no":
		return false
	default:
		return false
	}
}

func parseIntEnv(key string, defaultValue int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return defaultValue
	}
	parsed, err := strconv.Atoi(value)
	if err != nil {
		return defaultValue
	}
	return parsed
}

func parseOptionalIntEnv(key string) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return 0
	}
	parsed, err := strconv.Atoi(value)
	if err != nil {
		return 0
	}
	return parsed
}

func parseListEnv(key string) []string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return nil
	}
	parts := strings.Split(value, ",")
	result := make([]string, 0, len(parts))
	for _, entry := range parts {
		trimmed := strings.TrimSpace(entry)
		if trimmed != "" {
			result = append(result, trimmed)
		}
	}
	if len(result) == 0 {
		return nil
	}
	return result
}

func generateRunnerInstanceId() string {
	seed := make([]byte, 16)
	if _, err := rand.Read(seed); err == nil {
		return hex.EncodeToString(seed)
	}
	return fmt.Sprintf("runner-%d", time.Now().UnixNano())
}

type ExecutionContext struct {
	ExecutionId      string
	LeaseId          string
	TriggerId        string
	JobKey           string
	FireAtUtc        time.Time
	LeaseExpiresAt   time.Time
	ExecutionMode    string
	InvocationSource string
	EmitEvent        func(events []WorkEvent) error
}

type RunnerLogger interface {
	Info(message string, fields map[string]any)
	Warn(message string, fields map[string]any)
	Error(message string, fields map[string]any)
}

type defaultRunnerLogger struct{}

func (l *defaultRunnerLogger) Info(message string, fields map[string]any) {
	logWithFields("info", message, fields)
}
func (l *defaultRunnerLogger) Warn(message string, fields map[string]any) {
	logWithFields("warn", message, fields)
}
func (l *defaultRunnerLogger) Error(message string, fields map[string]any) {
	logWithFields("error", message, fields)
}

type ExecuteHandler func(ctx ExecutionContext, payload *string, logger RunnerLogger) error

type handlerRegistration struct {
	handler      ExecuteHandler
	registration *RunnerJobRegistration
}

type Runner struct {
	config   RunnerConfig
	client   *Client
	logger   RunnerLogger
	handlers map[string]handlerRegistration
	handlersMu sync.RWMutex
	grpcConn *grpcRunnerConnection
	outbox   *outboxStore
	fatalErr chan error
	cancel   context.CancelFunc
	transportCancel context.CancelFunc
	inflight sync.WaitGroup
	activeMu sync.Mutex
	activeLeases map[string]Lease
	renewMu sync.Mutex
	renewCancels map[string]context.CancelFunc
	abandonedMu sync.RWMutex
	abandoned map[string]struct{}
	registerJobs bool
}

func NewRunner(config RunnerConfig) (*Runner, error) {
	if strings.TrimSpace(config.RunnerId) == "" {
		return nil, errors.New("runner id is required")
	}
	if strings.TrimSpace(config.RunnerInstanceId) == "" {
		config.RunnerInstanceId = generateRunnerInstanceId()
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
	if config.HeartbeatInterval < 0 {
		config.HeartbeatInterval = 0
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

	registerJobs := true
	if config.RegisterJobs != nil {
		registerJobs = *config.RegisterJobs
	}
	config.RegisterJobs = boolPtr(registerJobs)

	client, err := NewClient(config.Config)
	if err != nil {
		return nil, err
	}

	return &Runner{
		config: config,
		client: client,
		logger: &defaultRunnerLogger{},
		handlers: map[string]handlerRegistration{},
		outbox: newOutboxStore(config.OutboxPath, config.OutboxMaxEntries, config.OutboxMaxBytes),
		activeLeases: map[string]Lease{},
		renewCancels: map[string]context.CancelFunc{},
		abandoned: map[string]struct{}{},
		registerJobs: registerJobs,
	}, nil
}

func (r *Runner) OnExecute(jobKey string, handler ExecuteHandler) {
	r.OnExecuteWithRegistration(jobKey, handler, nil)
}

func (r *Runner) OnExecuteWithRegistration(jobKey string, handler ExecuteHandler, registration *RunnerJobRegistration) {
	if strings.TrimSpace(jobKey) == "" {
		panic("jobKey is required")
	}
	if handler == nil {
		panic("handler is required")
	}
	r.handlersMu.Lock()
	r.handlers[strings.TrimSpace(jobKey)] = handlerRegistration{handler: handler, registration: registration}
	r.handlersMu.Unlock()
}

func (r *Runner) getHandler(jobKey string) (handlerRegistration, bool) {
	r.handlersMu.RLock()
	handler, ok := r.handlers[jobKey]
	r.handlersMu.RUnlock()
	return handler, ok
}

func (r *Runner) Run(ctx context.Context) error {
	r.handlersMu.RLock()
	hasHandlers := len(r.handlers) > 0
	r.handlersMu.RUnlock()
	if !hasHandlers {
		return errors.New("execute handler must be registered for at least one jobKey")
	}
	runCtx, cancel := context.WithCancel(ctx)
	r.cancel = cancel
	r.fatalErr = make(chan error, 1)
	transportCtx, transportCancel := context.WithCancel(runCtx)
	r.transportCancel = transportCancel

	queue := make(chan Lease, r.config.MaxInflight*2)
	semaphore := make(chan struct{}, r.config.MaxInflight)

	if r.config.TransportMode != TransportPolling {
		grpcConn, err := newGrpcRunnerConnection(r.config, r.fail)
		if err != nil {
			return err
		}
		r.grpcConn = grpcConn
		grpcConn.start(transportCtx, func(lease Lease) {
			select {
			case <-transportCtx.Done():
				return
			case queue <- lease:
			}
		})
	}

	if r.outbox != nil {
		r.outbox.Load()
		go r.replayOutboxLoop(runCtx)
	}

	if r.registerJobs {
		if err := r.registerJobDefinitions(runCtx); err != nil {
			cancel()
			return err
		}
	}

	go r.pollLoop(transportCtx, queue)
	if r.config.HeartbeatInterval > 0 {
		go r.heartbeatLoop(transportCtx)
	}

	for {
		select {
		case err := <-r.fatalErr:
			cancel()
			return err
		case <-runCtx.Done():
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return runCtx.Err()
		case lease := <-queue:
			semaphore <- struct{}{}
			r.inflight.Add(1)
			go func(lease Lease) {
				defer r.inflight.Done()
				defer func() { <-semaphore }()
				r.runLease(runCtx, lease)
			}(lease)
		}
	}
}

func (r *Runner) registerJobDefinitions(ctx context.Context) error {
	if strings.TrimSpace(r.config.EnvironmentTag) == "" {
		return errors.New("environment tag is required for job registration")
	}

	r.handlersMu.RLock()
	entries := make([]struct {
		jobKey       string
		registration *RunnerJobRegistration
	}, 0, len(r.handlers))
	for jobKey, registration := range r.handlers {
		entries = append(entries, struct {
			jobKey       string
			registration *RunnerJobRegistration
		}{jobKey: jobKey, registration: registration.registration})
	}
	r.handlersMu.RUnlock()

	for _, entry := range entries {
		request := runnerJobRegistrationRequest{
			EnvironmentTag:  r.config.EnvironmentTag,
			RunnerId:        r.config.RunnerId,
			RunnerInstanceId: r.config.RunnerInstanceId,
			JobKey:          entry.jobKey,
		}
		if entry.registration != nil {
			request.Description = entry.registration.Description
			request.Metadata = entry.registration.Metadata
		}

		response, err := r.client.RegisterJob(ctx, request)
		if err != nil {
			return err
		}

		if response != nil && !response.IsActive {
			r.logger.Warn("job registration pending approval", map[string]any{"jobKey": entry.jobKey})
		} else {
			r.logger.Info("job registration completed", map[string]any{"jobKey": entry.jobKey})
		}
	}

	return nil
}

func (r *Runner) Drain(timeout time.Duration) error {
	if r.transportCancel != nil {
		r.transportCancel()
	}

	if timeout <= 0 {
		timeout = 30 * time.Second
	}

	done := make(chan struct{})
	go func() {
		r.inflight.Wait()
		close(done)
	}()

	select {
	case <-done:
		r.sendDisconnectHeartbeat()
		if r.cancel != nil {
			r.cancel()
		}
		return nil
	case <-time.After(timeout):
		r.abandonInflight()
		r.sendDisconnectHeartbeat()
		if r.cancel != nil {
			r.cancel()
		}
		return context.DeadlineExceeded
	}
}

func (r *Runner) fail(err error) {
	if err == nil {
		return
	}
	if r.logger != nil {
		label := "runner mismatch"
		if IsRunnerIdInUse(err) {
			label = "runner id in use"
		}
		r.logger.Error(label, map[string]any{"error": err.Error()})
	}
	select {
	case r.fatalErr <- err:
	default:
	}
	if r.cancel != nil {
		r.cancel()
	}
}

func (r *Runner) trackLease(lease Lease, cancel context.CancelFunc) {
	r.activeMu.Lock()
	r.activeLeases[lease.LeaseId] = lease
	r.activeMu.Unlock()

	r.renewMu.Lock()
	r.renewCancels[lease.LeaseId] = cancel
	r.renewMu.Unlock()
}

func (r *Runner) untrackLease(leaseId string) {
	r.activeMu.Lock()
	delete(r.activeLeases, leaseId)
	r.activeMu.Unlock()

	r.renewMu.Lock()
	delete(r.renewCancels, leaseId)
	r.renewMu.Unlock()

	r.abandonedMu.Lock()
	delete(r.abandoned, leaseId)
	r.abandonedMu.Unlock()
}

func (r *Runner) markAbandoned(leaseId string) {
	r.abandonedMu.Lock()
	r.abandoned[leaseId] = struct{}{}
	r.abandonedMu.Unlock()
}

func (r *Runner) isAbandoned(leaseId string) bool {
	r.abandonedMu.RLock()
	_, ok := r.abandoned[leaseId]
	r.abandonedMu.RUnlock()
	return ok
}

func (r *Runner) abandonInflight() {
	r.activeMu.Lock()
	leases := make([]Lease, 0, len(r.activeLeases))
	for _, lease := range r.activeLeases {
		r.markAbandoned(lease.LeaseId)
		leases = append(leases, lease)
	}
	r.activeMu.Unlock()

	r.renewMu.Lock()
	for leaseId, cancel := range r.renewCancels {
		cancel()
		delete(r.renewCancels, leaseId)
	}
	r.renewMu.Unlock()

	for _, lease := range leases {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = r.client.Ack(ctx, r.config.RunnerId, lease, false, nil, "runner-shutdown")
		cancel()
	}
}

func (r *Runner) pollLoop(ctx context.Context, queue chan<- Lease) {
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
			RunnerInstanceId:    r.config.RunnerInstanceId,
		}

		leases, err := r.client.PollWithOptions(ctx, r.config.RunnerId, options)
		if err != nil {
			if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
				r.fail(err)
				return
			}
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

func (r *Runner) heartbeatLoop(ctx context.Context) {
	interval := r.config.HeartbeatInterval
	if interval <= 0 {
		return
	}
	if strings.TrimSpace(r.config.EnvironmentTag) == "" {
		r.logger.Warn("heartbeat skipped; environment is required", map[string]any{})
		return
	}

	ticker := time.NewTicker(interval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			metadataJson := r.buildHeartbeatMetadata()
			err := r.client.HeartbeatWithInstance(ctx, r.config.RunnerId, r.config.RunnerInstanceId, r.config.EnvironmentTag, metadataJson, nil)
			if err != nil {
				if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
					r.fail(err)
					return
				}
				r.logger.Warn("heartbeat failed", map[string]any{"error": err.Error()})
			}
		}
	}
}

func (r *Runner) runLease(ctx context.Context, lease Lease) {
	registration, ok := r.getHandler(lease.JobKey)
	if !ok {
		r.logger.Warn("no handler registered for jobKey", map[string]any{"jobKey": lease.JobKey})
		if r.grpcConn != nil && r.grpcConn.isConnected() {
			_ = r.grpcConn.send(buildAckFailureMessage(lease, "handler-not-found", "handler not registered", "handler-not-found"))
		} else {
			if err := r.client.Ack(ctx, r.config.RunnerId, lease, false, nil, "handler-not-found"); err != nil {
				if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
					r.fail(err)
					return
				}
				r.enqueueOutboxAckFailure(lease, "handler-not-found", "handler not registered", "handler-not-found")
			}
		}
		return
	}

	if !r.config.AllowTestExecutions && strings.EqualFold(lease.ExecutionMode, "test") {
		if r.grpcConn != nil && r.grpcConn.isConnected() {
			_ = r.grpcConn.send(buildAckFailureMessage(lease, "test-not-allowed", "test executions are disabled for this runner", "test-not-allowed"))
		} else {
			if err := r.client.Ack(ctx, r.config.RunnerId, lease, false, nil, "test-not-allowed"); err != nil {
				if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
					r.fail(err)
					return
				}
			}
		}
		return
	}

	renewCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	r.trackLease(lease, cancel)
	defer r.untrackLease(lease.LeaseId)
	go r.renewLoop(renewCtx, lease)

	ctxPayload := ExecutionContext{
		ExecutionId:      lease.ExecutionId,
		LeaseId:          lease.LeaseId,
		TriggerId:        lease.TriggerId,
		JobKey:           lease.JobKey,
		FireAtUtc:        lease.FireAtUtc,
		LeaseExpiresAt:   lease.LeaseExpiresAtUtc,
		ExecutionMode:    lease.ExecutionMode,
		InvocationSource: lease.InvocationSource,
		EmitEvent: func(events []WorkEvent) error {
			return r.sendEvents(ctx, lease, events, true)
		},
	}

	if err := registration.handler(ctxPayload, lease.Payload, r.logger); err != nil {
		if r.isAbandoned(lease.LeaseId) {
			r.logger.Warn("lease abandoned during shutdown", map[string]any{"leaseId": lease.LeaseId})
			return
		}
		if r.grpcConn != nil && r.grpcConn.isConnected() {
			_ = r.grpcConn.send(buildAckFailureMessage(lease, "execution-failed", err.Error(), "execution-failed"))
			return
		}
		if err := r.client.Ack(ctx, r.config.RunnerId, lease, false, nil, "execution-failed"); err != nil {
			if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
				r.fail(err)
				return
			}
			r.enqueueOutboxAckFailure(lease, "execution-failed", err.Error(), "execution-failed")
		}
		return
	}

	if r.isAbandoned(lease.LeaseId) {
		r.logger.Warn("lease abandoned during shutdown", map[string]any{"leaseId": lease.LeaseId})
		return
	}
	if r.grpcConn != nil && r.grpcConn.isConnected() {
		_ = r.grpcConn.send(buildAckSuccessMessage(lease))
		return
	}
	if err := r.client.Ack(ctx, r.config.RunnerId, lease, true, nil, ""); err != nil {
		if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
			r.fail(err)
			return
		}
		r.enqueueOutboxAckSuccess(lease)
	}
}

func (r *Runner) buildHeartbeatMetadata() string {
	transportState := "polling"
	if r.grpcConn != nil && r.grpcConn.isConnected() {
		transportState = "grpc"
	}
	metadata := map[string]any{
		"runnerInstanceId":   r.config.RunnerInstanceId,
		"transportMode":       r.config.TransportMode,
		"transportState":      transportState,
		"allowTestExecutions": r.config.AllowTestExecutions,
		"maxInflight":         r.config.MaxInflight,
		"capabilities":        r.config.Capabilities,
	}
	for key, value := range r.config.HeartbeatMetadata {
		metadata[key] = value
	}
	payload, err := json.Marshal(metadata)
	if err != nil {
		return ""
	}
	return string(payload)
}

func (r *Runner) buildDisconnectMetadata() string {
	metadata := map[string]any{
		"runnerInstanceId":   r.config.RunnerInstanceId,
		"transportMode":       r.config.TransportMode,
		"transportState":      "disconnected",
		"allowTestExecutions": r.config.AllowTestExecutions,
		"maxInflight":         r.config.MaxInflight,
		"capabilities":        r.config.Capabilities,
		"disconnectedAtUtc":   time.Now().UTC().Format(time.RFC3339Nano),
	}
	for key, value := range r.config.HeartbeatMetadata {
		metadata[key] = value
	}
	payload, err := json.Marshal(metadata)
	if err != nil {
		return ""
	}
	return string(payload)
}

func (r *Runner) sendDisconnectHeartbeat() {
	if strings.TrimSpace(r.config.EnvironmentTag) == "" {
		return
	}
	metadataJson := r.buildDisconnectMetadata()
	seenAt := time.Now().UTC()
	if err := r.client.HeartbeatWithInstance(
		context.Background(),
		r.config.RunnerId,
		r.config.RunnerInstanceId,
		r.config.EnvironmentTag,
		metadataJson,
		&seenAt,
	); err != nil {
		r.logger.Warn("disconnect heartbeat failed", map[string]any{"error": err.Error()})
	}
}

func (r *Runner) renewLoop(ctx context.Context, lease Lease) {
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		if r.isAbandoned(lease.LeaseId) {
			return
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
			if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
				r.fail(err)
				return
			}
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

func logWithFields(level string, message string, fields map[string]any) {
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
	Lease            Lease  `json:"lease"`
	ErrorType        string `json:"error_type"`
	ErrorMessage     string `json:"error_message"`
	DeadLetterReason string `json:"dead_letter_reason"`
}

type outboxEventsPayload struct {
	Lease  Lease       `json:"lease"`
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
		if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
			r.fail(err)
			return err
		}
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
						if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
							r.fail(err)
							return
						}
						r.outbox.MarkFailed(entry.ID)
					}
				}
			case "ack_failure":
				var payload outboxAckFailurePayload
				if err := json.Unmarshal(entry.Payload, &payload); err == nil {
					if err := r.client.Ack(ctx, r.config.RunnerId, payload.Lease, false, nil, payload.DeadLetterReason); err == nil {
						r.outbox.Remove(entry.ID)
					} else {
						if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
							r.fail(err)
							return
						}
						r.outbox.MarkFailed(entry.ID)
					}
				}
			case "events":
				var payload outboxEventsPayload
				if err := json.Unmarshal(entry.Payload, &payload); err == nil {
					if err := r.client.Events(ctx, r.config.RunnerId, payload.Lease, payload.Events); err == nil {
						r.outbox.Remove(entry.ID)
					} else {
						if IsRunnerMismatch(err) || IsRunnerIdInUse(err) {
							r.fail(err)
							return
						}
						r.outbox.MarkFailed(entry.ID)
					}
				}
			default:
				r.outbox.Remove(entry.ID)
			}
		}
	}
}

func buildAckSuccessMessage(lease Lease) *dynamic.Message {
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

func buildAckFailureMessage(lease Lease, errorType string, message string, reason string) *dynamic.Message {
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

func buildEventsMessage(lease Lease, events []WorkEvent) *dynamic.Message {
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
