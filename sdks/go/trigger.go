package croniq

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"time"
)

// DefaultTriggerRequestTimeout bounds a single POST /v1/trigger call when
// the context passed to [TriggerClient.Trigger] carries no deadline of its
// own. Matches the .NET SDK's CroniqClientOptions.RequestTimeout so the two
// producer clients behave the same out of the box.
const DefaultTriggerRequestTimeout = 30 * time.Second

// TriggerRequest is the body sent to POST /v1/trigger.
//
// Optional fields use `omitempty` so a value the caller never supplied is
// omitted from the JSON entirely rather than sent as null/empty: the server
// distinguishes "unset" (inherit the job's configured value) from an explicit
// value, and a producer must not fabricate defaults on the wire.
//
// `omitempty` also drops an EXPLICITLY empty value — a nil-or-empty slice, a
// blank string — and that is the intended contract, not an artefact (issue
// #553). The server already reads an empty Require as "inherit the job's
// runner { require … }", so "require": [] would only be a second wire spelling
// of a message that has one. And Timeout "" is not a parseable duration:
// sending it would hand the runner a broken value where omitting it inherits
// the job's own timeout. The other SDKs normalize empty to absent to match.
type TriggerRequest struct {
	// JobKey is the job to fire, e.g. "billing:invoice". Required.
	JobKey string `json:"job_key"`

	// Metadata is arbitrary caller JSON, merged over the job's DSL metadata
	// and forwarded to the handler verbatim. Nested objects and non-string
	// values are preserved — it is NOT flattened or stringified. Keys
	// starting with "__" are reserved for internal use.
	Metadata map[string]any `json:"metadata,omitempty"`

	// Require lists capabilities a runner MUST have to be assigned this
	// execution. Empty → inherit the job's runner { require … }.
	Require []string `json:"require,omitempty"`

	// Prefer lists capabilities used to prefer runners when several are
	// eligible.
	Prefer []string `json:"prefer,omitempty"`

	// Timeout is the execution timeout as a server duration string
	// (e.g. "30s", "5m"). Empty → inherit the job's configured timeout; the
	// server falls back to 5m only when the job declares none either.
	Timeout string `json:"timeout,omitempty"`

	// IdempotencyKey is an optional dedup key scoped per job_key. A server
	// with trigger-idempotency support coalesces a repeat trigger carrying
	// the same (job_key, idempotency_key) onto the existing execution (see
	// [TriggerResponse.Deduplicated]); older servers ignore it. Capped at
	// 200 characters server-side — a longer key is rejected with 400.
	IdempotencyKey string `json:"idempotency_key,omitempty"`
}

// TriggerResponse is the parsed body of a successful POST /v1/trigger.
type TriggerResponse struct {
	// ExecutionID identifies the execution the trigger resolved to. On a
	// dedup hit this is the EXISTING execution's id, not a new one.
	ExecutionID string `json:"execution_id"`

	// Queued is the server work-queue depth after the trigger was processed
	// (unchanged on a dedup hit — nothing is enqueued then).
	Queued int `json:"queued"`

	// Deduplicated is true when the server coalesced this trigger onto an
	// existing execution because the request carried an idempotency_key it
	// had already seen. Servers without idempotency-key support omit the
	// field entirely; it then decodes to false (Go's zero value), which is
	// the intended default.
	Deduplicated bool `json:"deduplicated"`
}

// TriggerClient is the producer-side client for firing Croniq jobs on
// demand via POST /v1/trigger. It is the counterpart to [Runner] (the
// consumer side) and is deliberately independent of it: triggering requires
// the jobs:trigger (or admin) scope, which runner poll keys typically do
// not carry, so a TriggerClient authenticates with ITS OWN credentials.
//
// Construct one with [NewTriggerClient] and configure auth via the builder
// methods. Safe for concurrent use.
type TriggerClient struct {
	client         *Client
	requestTimeout time.Duration
}

// NewTriggerClient constructs a TriggerClient targeting serverURL (trailing
// slashes tolerated). Each call is bounded by [DefaultTriggerRequestTimeout]
// unless the caller's context already carries a deadline; override with
// [TriggerClient.WithRequestTimeout]. Configure credentials with
// [TriggerClient.WithAPIKey] or [TriggerClient.WithBearer] — the endpoint
// requires the jobs:trigger or admin scope.
//
// serverURL must be https:// unless its host is loopback (localhost,
// 127.0.0.0/8, ::1); a non-loopback http:// URL makes every
// [TriggerClient.Trigger] call fail with a configuration error unless
// [TriggerClient.WithInsecureHTTP] is chained on. See [TriggerClient.Err].
func NewTriggerClient(serverURL string) *TriggerClient {
	return &TriggerClient{
		client:         NewClient(serverURL),
		requestTimeout: DefaultTriggerRequestTimeout,
	}
}

// WithAPIKey configures `Authorization: ApiKey {key}` on every trigger
// request. Returns the same client for chaining.
func (tc *TriggerClient) WithAPIKey(key string) *TriggerClient {
	tc.client.WithAPIKey(key)
	return tc
}

// WithBearer configures `Authorization: Bearer {token}` on every trigger
// request.
func (tc *TriggerClient) WithBearer(token string) *TriggerClient {
	tc.client.WithBearer(token)
	return tc
}

// WithHTTPClient injects a custom *http.Client (for tests, custom
// transports, proxies, mTLS, …).
func (tc *TriggerClient) WithHTTPClient(hc *http.Client) *TriggerClient {
	tc.client.WithHTTPClient(hc)
	return tc
}

// WithInsecureHTTP opts this client in to a cleartext http:// server URL on a
// non-loopback host. Without it such a URL makes every trigger call fail; with
// it the client works but logs one loud warning, because the credential then
// travels in cleartext on every call (and through any HTTP proxy the
// environment configures). Lab and staging only — never production.
func (tc *TriggerClient) WithInsecureHTTP() *TriggerClient {
	tc.client.WithInsecureHTTP()
	return tc
}

// Err reports the base-URL validation failure recorded at construction, or
// nil when the configuration is sound.
func (tc *TriggerClient) Err() error { return tc.client.Err() }

// WithRequestTimeout overrides the per-call timeout applied when the context
// passed to [TriggerClient.Trigger] carries no deadline of its own. A
// non-positive duration is ignored (the default is kept).
func (tc *TriggerClient) WithRequestTimeout(d time.Duration) *TriggerClient {
	if d > 0 {
		tc.requestTimeout = d
	}
	return tc
}

// Trigger fires a job immediately via POST /v1/trigger. The job's registered
// handler runs on the next eligible runner, exactly like a scheduled fire.
//
// Unset optional fields on req are omitted from the request body (never sent
// as null). A non-2xx response — including the 429 the server returns when a
// job is at its per-job queue-overflow cap (max_queue_depth) — is returned
// as a [*ServerError]; a producer batching or retrying triggers should
// inspect [ServerError.Status] to observe backpressure rather than treating
// every failure alike.
//
// When ctx carries no deadline, the call is bounded by the client's request
// timeout (see [TriggerClient.WithRequestTimeout]).
func (tc *TriggerClient) Trigger(ctx context.Context, req *TriggerRequest) (*TriggerResponse, error) {
	if req == nil {
		return nil, errors.New("croniq: trigger request must not be nil")
	}
	if strings.TrimSpace(req.JobKey) == "" {
		return nil, errors.New("croniq: trigger job_key must not be empty")
	}

	if tc.requestTimeout > 0 {
		if _, ok := ctx.Deadline(); !ok {
			var cancel context.CancelFunc
			ctx, cancel = context.WithTimeout(ctx, tc.requestTimeout)
			defer cancel()
		}
	}

	var resp TriggerResponse
	if err := tc.client.do(ctx, http.MethodPost, "/v1/trigger", req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}
