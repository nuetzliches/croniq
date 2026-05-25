// Package conformance holds the Go binding for the language-agnostic
// Croniq runner conformance suite at sdks/conformance/cases/.
//
// Run via `go test ./conformance/...` — each YAML case becomes a
// table-driven subtest that boots an httptest mock server, configures
// the SDK against it, and asserts the recorded request stream.
package conformance

import "strings"

// Spec is the in-memory representation of one case YAML file.
type Spec struct {
	Name            string            `yaml:"name"`
	Description     string            `yaml:"description"`
	RunnerConfig    RunnerConfigSpec  `yaml:"runner_config"`
	Handlers        []HandlerSpec     `yaml:"handlers"`
	ServerScript    []ScriptEntrySpec `yaml:"server_script"`
	ShutdownAfterMs *int              `yaml:"shutdown_after_ms,omitempty"`
	Expectations    Expectations      `yaml:"expectations"`
}

// RunnerConfigSpec maps to the SDK's runner options. Optional fields use
// pointers so we can distinguish "set to zero" from "unset".
type RunnerConfigSpec struct {
	RunnerID          string   `yaml:"runner_id"`
	RunnerIDPrefix    string   `yaml:"runner_id_prefix"`
	Capabilities      []string `yaml:"capabilities"`
	Tags              []string `yaml:"tags"`
	MaxInflight       *int     `yaml:"max_inflight"`
	APIKey            string   `yaml:"api_key"`
	BearerToken       string   `yaml:"bearer_token"`
	PollTimeoutMs     *int     `yaml:"poll_timeout_ms"`
	RenewIntervalMs   *int     `yaml:"renew_interval_ms"`
	DrainTimeoutMs    *int     `yaml:"drain_timeout_ms"`
	PollRetryDelayMs  *int     `yaml:"poll_retry_delay_ms"`
	CapacityBackoffMs *int     `yaml:"capacity_backoff_ms"`
}

// HandlerSpec describes one handler to register, identified by job_key
// and a behaviour sentinel.
type HandlerSpec struct {
	JobKey       string `yaml:"job_key"`
	IsDefault    bool   `yaml:"is_default"`
	Schedule     string `yaml:"schedule"`
	Behavior     string `yaml:"behavior"`
	ErrorMessage string `yaml:"error_message"`
	DurationMs   int    `yaml:"duration_ms"`
	Level        string `yaml:"level"`
	Message      string `yaml:"message"`
	Count        int    `yaml:"count"`
	IntervalMs   int    `yaml:"interval_ms"`
}

// ScriptEntrySpec is one rule in the mock server's response script. The
// `On` field is "METHOD /path"; helpers below split it.
type ScriptEntrySpec struct {
	On         string      `yaml:"on"`
	MatchCount *int        `yaml:"match_count,omitempty"`
	Respond    RespondSpec `yaml:"respond"`
}

// Method returns the HTTP method portion of On.
func (e ScriptEntrySpec) Method() string {
	parts := strings.SplitN(e.On, " ", 2)
	if len(parts) == 0 {
		return ""
	}
	return parts[0]
}

// Path returns the URL-path portion of On.
func (e ScriptEntrySpec) Path() string {
	parts := strings.SplitN(e.On, " ", 2)
	if len(parts) < 2 {
		return ""
	}
	return parts[1]
}

// RespondSpec is the response a script rule produces.
type RespondSpec struct {
	Status  int               `yaml:"status"`
	Body    any               `yaml:"body"`
	DelayMs *int              `yaml:"delay_ms,omitempty"`
	Headers map[string]string `yaml:"headers,omitempty"`
}

// Expectations is the case's post-hoc assertions.
type Expectations struct {
	DurationMaxMs *int              `yaml:"duration_max_ms"`
	HTTP          []HTTPExpectation `yaml:"http"`
}

// HTTPExpectation is one expected request pattern.
type HTTPExpectation struct {
	Method     string            `yaml:"method"`
	Path       string            `yaml:"path"`
	ExactCount *int              `yaml:"exact_count,omitempty"`
	MinCount   *int              `yaml:"min_count,omitempty"`
	MaxCount   *int              `yaml:"max_count,omitempty"`
	Headers    map[string]string `yaml:"headers,omitempty"`
	BodyMatch  any               `yaml:"body_match,omitempty"`
}
