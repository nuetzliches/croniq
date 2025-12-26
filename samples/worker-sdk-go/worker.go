package croniqworker

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

	return c.post(ctx, fmt.Sprintf("/work/%s:events", url.PathEscape(lease.ExecutionId)), request, nil)
}

func (c *Client) post(ctx context.Context, suffix string, body any, out any) error
{
	endpoint, err := c.buildURL(fmt.Sprintf("/tenants/%s%s", url.PathEscape(c.tenantID), suffix))
	if err != nil {
		return err
	}

	payload, err := json.Marshal(body)
	if err != nil {
		return fmt.Errorf("encode request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(payload))
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	c.applyAuth(req)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	bodyBytes, _ := io.ReadAll(resp.Body)

	if resp.StatusCode == http.StatusNoContent && out == nil {
		return nil
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return &ApiError{StatusCode: resp.StatusCode, Body: strings.TrimSpace(string(bodyBytes))}
	}

	if out == nil {
		return nil
	}

	if len(bodyBytes) == 0 {
		return nil
	}

	if err := json.Unmarshal(bodyBytes, out); err != nil {
		return fmt.Errorf("decode response: %w", err)
	}

	return nil
}

func (c *Client) buildURL(path string) (string, error)
{
	base := *c.baseURL
	base.Path = strings.TrimRight(base.Path, "/") + path
	query := base.Query()
	if c.environmentTag != "" {
		query.Set("environment", c.environmentTag)
	}
	base.RawQuery = query.Encode()
	return base.String(), nil
}

func (c *Client) applyAuth(req *http.Request)
{
	if c.bearerToken != "" {
		req.Header.Set("Authorization", "Bearer "+c.bearerToken)
		return
	}

	if c.apiKey != "" {
		req.Header.Set("X-Croniq-Key", c.apiKey)
	}
}

type pollRequest struct {
	EnvironmentTag string `json:"environmentTag,omitempty"`
	RunnerId       string `json:"runnerId"`
	BatchSize      *int   `json:"batchSize,omitempty"`
	WaitForMs      *int   `json:"waitForMs,omitempty"`
}

type pollResponse struct {
	Leases []Lease `json:"leases"`
}

type renewRequest struct {
	EnvironmentTag string `json:"environmentTag,omitempty"`
	RunnerId       string `json:"runnerId"`
	Lease          Lease  `json:"lease"`
}

type renewResponse struct {
	Renewed bool   `json:"renewed"`
	Lease   *Lease `json:"lease,omitempty"`
}

type ackRequest struct {
	EnvironmentTag  string     `json:"environmentTag,omitempty"`
	RunnerId        string     `json:"runnerId"`
	Lease           Lease      `json:"lease"`
	Succeeded       bool       `json:"succeeded"`
	NextFireTimeUtc *time.Time `json:"nextFireTimeUtc,omitempty"`
	DeadLetterReason string    `json:"deadLetterReason,omitempty"`
}

type eventsRequest struct {
	EnvironmentTag string      `json:"environmentTag,omitempty"`
	RunnerId       string      `json:"runnerId"`
	Lease          Lease       `json:"lease"`
	Events         []WorkEvent `json:"events"`
}
