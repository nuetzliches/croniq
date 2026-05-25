package croniq

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// ServerError is returned when the server responds with a non-2xx status.
// The wire layer treats every non-success as transient — callers (the
// runner loop) decide whether to retry, back off, or escalate based on
// the status code and their own policy.
type ServerError struct {
	Status int
	Body   string
}

func (e *ServerError) Error() string {
	return fmt.Sprintf("server error: %d — %s", e.Status, e.Body)
}

// Client is a thin typed wrapper around the Croniq HTTP API. It is
// safe for concurrent use; the runner shares one client across all
// in-flight executions.
type Client struct {
	httpClient *http.Client
	baseURL    string
	authHeader string
}

// NewClient constructs a Client targeting the given base URL. Trailing
// slashes are tolerated.
func NewClient(baseURL string) *Client {
	return &Client{
		// 0 timeout = unbounded; individual requests use ctx deadlines.
		httpClient: &http.Client{},
		baseURL:    strings.TrimRight(baseURL, "/"),
	}
}

// WithAPIKey configures `Authorization: ApiKey {key}` on every request.
// Returns the same client for chaining.
func (c *Client) WithAPIKey(key string) *Client {
	c.authHeader = "ApiKey " + key
	return c
}

// WithBearer configures `Authorization: Bearer {token}` on every request.
func (c *Client) WithBearer(token string) *Client {
	c.authHeader = "Bearer " + token
	return c
}

// WithHTTPClient lets callers inject a custom http.Client (for tests,
// custom transports, proxy settings, mTLS, etc.).
func (c *Client) WithHTTPClient(hc *http.Client) *Client {
	if hc != nil {
		c.httpClient = hc
	}
	return c
}

// Poll calls POST /v1/work/poll. The default request timeout is 35s
// to accommodate the server's long-poll window — pass a context with
// a different deadline to override.
func (c *Client) Poll(ctx context.Context, req *PollRequest) (*PollResponse, error) {
	var resp PollResponse
	// 35s aligns with the Rust SDK's poll timeout — long enough for the
	// server's 30 s long-poll window plus a bit of slack.
	if _, ok := ctx.Deadline(); !ok {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, 35*time.Second)
		defer cancel()
	}
	if err := c.do(ctx, http.MethodPost, "/v1/work/poll", req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// Ack calls POST /v1/work/ack.
func (c *Client) Ack(ctx context.Context, req *AckRequest) error {
	return c.do(ctx, http.MethodPost, "/v1/work/ack", req, nil)
}

// Renew calls POST /v1/work/renew to extend the lease on an in-flight
// execution. Called periodically by the runner while a handler runs.
func (c *Client) Renew(ctx context.Context, req *RenewRequest) error {
	return c.do(ctx, http.MethodPost, "/v1/work/renew", req, nil)
}

// PushEvents calls POST /v1/work/{execution_id}/events with a batch of
// structured log events.
func (c *Client) PushEvents(ctx context.Context, executionID string, events []WorkEvent) error {
	if len(events) == 0 {
		return nil
	}
	path := "/v1/work/" + executionID + "/events"
	return c.do(ctx, http.MethodPost, path, events, nil)
}

// RegisterJob calls POST /v1/jobs/register to self-register a job + schedule.
func (c *Client) RegisterJob(ctx context.Context, req *RegisterJobRequest) error {
	return c.do(ctx, http.MethodPost, "/v1/jobs/register", req, nil)
}

// do is the single HTTP send-and-decode helper. Non-2xx responses are
// returned as *ServerError so the runner can decide retry policy.
func (c *Client) do(ctx context.Context, method, path string, body any, out any) error {
	var reader io.Reader
	if body != nil {
		buf, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("marshal request body: %w", err)
		}
		reader = bytes.NewReader(buf)
	}

	req, err := http.NewRequestWithContext(ctx, method, c.baseURL+path, reader)
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if c.authHeader != "" {
		req.Header.Set("Authorization", c.authHeader)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		buf, _ := io.ReadAll(resp.Body)
		return &ServerError{Status: resp.StatusCode, Body: string(buf)}
	}

	if out == nil {
		// Drain so the connection can be reused.
		_, _ = io.Copy(io.Discard, resp.Body)
		return nil
	}
	return json.NewDecoder(resp.Body).Decode(out)
}
