package croniq

import "encoding/json"

// PollRequest is the body sent to POST /v1/work/poll.
type PollRequest struct {
	RunnerID     string   `json:"runner_id"`
	Capabilities []string `json:"capabilities"`
	MaxInflight  int      `json:"max_inflight"`
	Inflight     []string `json:"inflight"`
	InstanceID   string   `json:"instance_id,omitempty"`
	Tags         []string `json:"tags,omitempty"`
}

// PollResponse is the body returned by POST /v1/work/poll.
type PollResponse struct {
	Work   []WorkAssignment `json:"work"`
	Cancel []string         `json:"cancel,omitempty"`
}

// WorkAssignment describes a single execution the server has handed to
// this runner.
type WorkAssignment struct {
	ExecutionID string `json:"execution_id"`
	JobKey      string `json:"job_key"`
	FireAt      string `json:"fire_at"`
	// ScheduledFor is the original logical fire time (RFC 3339). Empty when
	// the server predates the field — consumers must not fall back to FireAt.
	ScheduledFor string          `json:"scheduled_for"`
	Attempt      int             `json:"attempt"`
	Metadata     json.RawMessage `json:"metadata"`
	Timeout      string          `json:"timeout"`
}

// AckRequest is the body sent to POST /v1/work/ack.
type AckRequest struct {
	RunnerID    string `json:"runner_id"`
	ExecutionID string `json:"execution_id"`
	Status      string `json:"status"`
	Error       string `json:"error,omitempty"`
	DurationMs  int64  `json:"duration_ms,omitempty"`
	Attempt     int    `json:"attempt"`
}

// RenewRequest is the body sent to POST /v1/work/renew.
type RenewRequest struct {
	RunnerID    string `json:"runner_id"`
	ExecutionID string `json:"execution_id"`
}

// WorkEvent is a single structured log line. Send via
// [ExecutionContext.Log], [ExecutionContext.PushEvents], or the
// streaming [LogWriter].
type WorkEvent struct {
	Level   string            `json:"level,omitempty"`
	Message string            `json:"message"`
	Fields  map[string]string `json:"fields,omitempty"`
}

// RegisterJobRequest is the body sent to POST /v1/jobs/register.
type RegisterJobRequest struct {
	JobKey       string   `json:"job_key"`
	Schedule     string   `json:"schedule"`
	Timezone     string   `json:"timezone,omitempty"`
	Timeout      string   `json:"timeout,omitempty"`
	RunnerID     string   `json:"runner_id,omitempty"`
	Capabilities []string `json:"capabilities,omitempty"`
	Description  string   `json:"description,omitempty"`
}
