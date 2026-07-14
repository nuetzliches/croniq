package conformance

// TriggerSpec is the in-memory representation of one trigger (producer)
// case YAML under sdks/conformance/cases-trigger/. It maps to
// trigger-case-schema.json: a producer case declares trigger_config +
// trigger_calls (explicit trigger(...) invocations) where a runner [Spec]
// declares runner_config + handlers. server_script, expectations and their
// nested types are shared with the runner harness.
type TriggerSpec struct {
	Name          string            `yaml:"name"`
	Description   string            `yaml:"description"`
	TriggerConfig TriggerConfigSpec `yaml:"trigger_config"`
	TriggerCalls  []TriggerCallSpec `yaml:"trigger_calls"`
	ServerScript  []ScriptEntrySpec `yaml:"server_script"`
	Expectations  Expectations      `yaml:"expectations"`
}

// TriggerConfigSpec maps to the trigger client's options. server_url is NOT
// listed — the binding injects the mock server's base URL, exactly as
// runner cases omit it from runner_config. The trigger client authenticates
// with its own credentials, independent of any runner.
type TriggerConfigSpec struct {
	APIKey      string `yaml:"api_key"`
	BearerToken string `yaml:"bearer_token"`
}

// TriggerCallSpec is one trigger(...) invocation plus the outcome the client
// must surface to the caller.
type TriggerCallSpec struct {
	Request TriggerCallRequest `yaml:"request"`
	Expect  TriggerCallExpect  `yaml:"expect"`
}

// TriggerCallRequest holds the arguments passed to the trigger client for a
// single call. Fields absent here MUST NOT appear in the outbound JSON body
// (asserted via expectations.http[].body_absent — the runner materialises
// them as an omitempty request struct so unset optionals never serialise).
type TriggerCallRequest struct {
	JobKey         string         `yaml:"job_key"`
	Require        []string       `yaml:"require"`
	Prefer         []string       `yaml:"prefer"`
	Metadata       map[string]any `yaml:"metadata"`
	Timeout        string         `yaml:"timeout"`
	IdempotencyKey string         `yaml:"idempotency_key"`
}

// TriggerCallExpect is the outcome a call must produce. By convention
// exactly one of Response (the call succeeds) or Error (the call raises /
// returns an error) is set.
type TriggerCallExpect struct {
	Response *TriggerExpectResponse `yaml:"response"`
	Error    bool                   `yaml:"error"`
}

// TriggerExpectResponse is a subset match on the parsed TriggerResponse the
// client returns. Only non-nil fields are asserted; ExecutionID accepts "*"
// for any non-empty value.
type TriggerExpectResponse struct {
	ExecutionID  *string `yaml:"execution_id"`
	Queued       *int    `yaml:"queued"`
	Deduplicated *bool   `yaml:"deduplicated"`
}
