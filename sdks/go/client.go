package croniq

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
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

// OwnershipDeniedError reports a 403 from one of the work endpoints: the
// authenticated credential is bound to a different runner_id than the one
// the request named (server issue #436).
//
// Unlike a 409 — where a duplicate deployment may release the identity on
// its own — this is permanent. Retrying cannot clear it; an operator has to
// give the runner its own runner_id or release the existing binding with
// DELETE /v1/runners/{id}. [Runner.Run] returns this error rather than
// polling forever, so a misconfigured runner exits instead of looking idle
// (issue #437). Callers can match it with [errors.As].
type OwnershipDeniedError struct {
	RunnerID string
	Endpoint string
	Body     string
}

func (e *OwnershipDeniedError) Error() string {
	return fmt.Sprintf(
		"work ownership denied on %s — this runner's credential does not own runner_id %q. "+
			"Give the runner its own runner_id, or release the existing binding with "+
			"DELETE /v1/runners/{id}: %s",
		e.Endpoint, e.RunnerID, e.Body)
}

// PollInstanceConflictError reports that POST /v1/work/poll answered
// 409 Conflict MaxConsecutivePollConflicts times in a row: another
// process is already registered under this runner_id and keeps winning
// the identity (fencing, server issue #374).
//
// A single 409 is transient — a deposed instance may legitimately take
// its identity back — so the runner backs off and retries. A streak of
// them is not: it is a duplicate deployment, two processes started with
// the same fixed runner_id. Retrying forever there logs a warning that
// scrolls past and leaves the misconfiguration invisible, so
// [Runner.Run] returns this error instead and the process can exit
// non-zero (issue #134 sub-item 1). Callers can match it with
// [errors.As].
//
// Distinct from [OwnershipDeniedError], which is a 403 and permanent
// from the first response.
type PollInstanceConflictError struct {
	RunnerID string
	// ConsecutiveCount is the streak length observed before bailing,
	// equal to Options.MaxConsecutivePollConflicts at return time.
	ConsecutiveCount int
	Body             string
}

func (e *PollInstanceConflictError) Error() string {
	return fmt.Sprintf(
		"poll instance conflict — another runner is already registered with runner_id %q. "+
			"Observed %d consecutive 409 Conflict responses on POST /v1/work/poll. "+
			"Stop the duplicate process or rotate the runner_id: %s",
		e.RunnerID, e.ConsecutiveCount, e.Body)
}

// AuthFailedError reports that a work endpoint answered 401 Unauthorized
// MaxConsecutiveAuthFailures times in a row: the API key was rejected and
// keeps being rejected.
//
// The credential is read once, when the client is built, and never re-read,
// so retrying presents the same rejected key forever. Before this existed a
// 401 fell into the generic transient bucket and [Runner.Run] retried on the
// poll interval indefinitely: the process stayed up, looked healthy, did
// nothing, and never exited non-zero — so no restart policy fired, and
// restarting is exactly what would have picked up the new key (issue #473).
//
// Not returned on the first 401. Key rotation hands over by installing the
// new key and giving the old one an expiry (server issue #471), and dropping
// dead on a single 401 would turn a narrow race around that handover into an
// outage. Callers can match it with [errors.As].
type AuthFailedError struct {
	RunnerID string
	Endpoint string
	// ConsecutiveCount is the streak length observed before bailing, equal
	// to Options.MaxConsecutiveAuthFailures at return time.
	ConsecutiveCount int
	Body             string
}

func (e *AuthFailedError) Error() string {
	return fmt.Sprintf(
		"unauthorized on %s — the API key was rejected on %d consecutive attempts. "+
			"It may have been revoked, or its rotation grace window may have elapsed. "+
			"Restart the runner with the current key: %s",
		e.Endpoint, e.ConsecutiveCount, e.Body)
}

// isUnauthorized reports whether err is a 401 from a work endpoint. Counted
// rather than acted on immediately: see [AuthFailedError].
func isUnauthorized(err error) bool {
	var se *ServerError
	return errors.As(err, &se) && se.Status == http.StatusUnauthorized
}

// isOwnershipDenied reports whether err is a 403 from a work endpoint —
// the wire layer keeps every non-2xx as a *ServerError, so the runner loop
// is where the status becomes a policy decision.
func isOwnershipDenied(err error) bool {
	var se *ServerError
	return errors.As(err, &se) && se.Status == http.StatusForbidden
}

// isInstanceConflict reports whether err is a 409 from the poll endpoint —
// the fencing refusal a duplicate deployment produces. Counted rather than
// acted on immediately: see [PollInstanceConflictError].
func isInstanceConflict(err error) bool {
	var se *ServerError
	return errors.As(err, &se) && se.Status == http.StatusConflict
}

// serverStatus extracts the HTTP status from err, or 0 when err is not a
// *ServerError (network failure, timeout, …).
func serverStatus(err error) int {
	var se *ServerError
	if errors.As(err, &se) {
		return se.Status
	}
	return 0
}

// Client is a thin typed wrapper around the Croniq HTTP API. It is
// safe for concurrent use; the runner shares one client across all
// in-flight executions.
type Client struct {
	httpClient *http.Client
	baseURL    string
	authHeader string

	// insecureHTTP records the caller's explicit opt-in to a cleartext
	// http:// base URL on a non-loopback host (see WithInsecureHTTP).
	insecureHTTP bool

	// configErr holds the base-URL validation failure, if any. Recorded
	// at construction time rather than returned, because NewClient has
	// no error result — Err surfaces it, Runner.Run returns it before
	// the first poll, and do refuses to send anything while it is set.
	configErr error
}

// NewClient constructs a Client targeting the given base URL. Trailing
// slashes are tolerated.
//
// The base URL must be https:// unless its host is loopback (localhost,
// 127.0.0.0/8, ::1) — the credential is attached to every request and would
// otherwise travel in cleartext. A non-loopback http:// URL is recorded as a
// configuration error (see [Client.Err]) unless [Client.WithInsecureHTTP] is
// chained on.
func NewClient(baseURL string) *Client {
	return &Client{
		// 0 timeout = unbounded; individual requests use ctx deadlines.
		httpClient: &http.Client{},
		baseURL:    strings.TrimRight(baseURL, "/"),
		configErr:  validateBaseURL(baseURL, false),
	}
}

// WithInsecureHTTP opts this client in to a cleartext http:// base URL on a
// non-loopback host, clearing the configuration error NewClient recorded and
// emitting one loud warning instead: the API key then travels in cleartext on
// every request. Intended for lab and staging setups that genuinely have no
// TLS terminator — never for production.
func (c *Client) WithInsecureHTTP() *Client {
	c.insecureHTTP = true
	// Re-run validation with the opt-in applied: the flag necessarily
	// arrives after NewClient in a builder chain, so the constructor
	// could not have taken it into account.
	c.configErr = validateBaseURL(c.baseURL, true)
	return c
}

// Err reports the base-URL validation failure recorded at construction, or
// nil when the configuration is sound. Every request short-circuits with this
// error while it is set.
func (c *Client) Err() error { return c.configErr }

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
	// Never put a credential on the wire against a base URL we refused.
	if c.configErr != nil {
		return c.configErr
	}

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
