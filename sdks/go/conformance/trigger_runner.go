package conformance

import (
	"context"
	"testing"
	"time"

	croniq "github.com/nuetzliches/croniq/sdks/go"
)

// RunTrigger executes a single trigger (producer) conformance TriggerSpec
// against a fresh mock server: it configures a [croniq.TriggerClient] from
// trigger_config, makes each trigger_calls[] invocation in order, asserts
// the surfaced result/error against the call's `expect`, then asserts the
// recorded request stream against `expectations.http`.
func RunTrigger(t *testing.T, spec *TriggerSpec) {
	t.Helper()

	mock := StartMockServer(spec.ServerScript)
	defer mock.Close()

	tc := croniq.NewTriggerClient(mock.BaseURL())
	switch {
	case spec.TriggerConfig.APIKey != "":
		tc.WithAPIKey(spec.TriggerConfig.APIKey)
	case spec.TriggerConfig.BearerToken != "":
		tc.WithBearer(spec.TriggerConfig.BearerToken)
	}

	// duration_max_ms bounds the whole case (all calls share the budget),
	// mirroring how the runner harness treats it as a deadline rather than
	// a hard-asserted duration.
	deadline := time.Duration(deref(spec.Expectations.DurationMaxMs, 5000)) * time.Millisecond
	ctx, cancel := context.WithTimeout(context.Background(), deadline)
	defer cancel()

	for i, call := range spec.TriggerCalls {
		req := &croniq.TriggerRequest{
			JobKey:         call.Request.JobKey,
			Metadata:       call.Request.Metadata,
			Require:        call.Request.Require,
			Prefer:         call.Request.Prefer,
			Timeout:        call.Request.Timeout,
			IdempotencyKey: call.Request.IdempotencyKey,
		}
		resp, err := tc.Trigger(ctx, req)
		assertTriggerCall(t, i, call.Expect, resp, err)
	}

	assertHTTP(t, spec.Expectations.HTTP, mock.Recorded())
}

// assertTriggerCall verifies a single call's surfaced outcome against its
// `expect`: an error when expect.error is set, otherwise a subset match on
// the returned TriggerResponse.
func assertTriggerCall(t *testing.T, idx int, expect TriggerCallExpect, resp *croniq.TriggerResponse, err error) {
	t.Helper()

	if expect.Error {
		if err == nil {
			t.Errorf("call[%d]: expected an error but got response %+v", idx, resp)
		}
		return
	}

	if err != nil {
		t.Errorf("call[%d]: unexpected error: %v", idx, err)
		return
	}
	if resp == nil {
		t.Errorf("call[%d]: expected a response but got nil", idx)
		return
	}
	if expect.Response == nil {
		// No response subset specified — a successful call is enough.
		return
	}

	exp := expect.Response
	if exp.ExecutionID != nil {
		switch {
		case *exp.ExecutionID == "*":
			if resp.ExecutionID == "" {
				t.Errorf("call[%d]: execution_id expected non-empty, got empty", idx)
			}
		case resp.ExecutionID != *exp.ExecutionID:
			t.Errorf("call[%d]: execution_id = %q, want %q", idx, resp.ExecutionID, *exp.ExecutionID)
		}
	}
	if exp.Queued != nil && resp.Queued != *exp.Queued {
		t.Errorf("call[%d]: queued = %d, want %d", idx, resp.Queued, *exp.Queued)
	}
	if exp.Deduplicated != nil && resp.Deduplicated != *exp.Deduplicated {
		t.Errorf("call[%d]: deduplicated = %v, want %v", idx, resp.Deduplicated, *exp.Deduplicated)
	}
}
