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
	if strings.TrimSpace(runnerId) == "" {
		return nil, errors.New("runner id is required")
	}
	if batchSize <= 0 {
		batchSize = 1
	}

	request := pollRequest{
		RunnerId:  strings.TrimSpace(runnerId),
		BatchSize: &batchSize,
	}

	if waitFor > 0 {
		waitMs := int(waitFor / time.Millisecond)
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
