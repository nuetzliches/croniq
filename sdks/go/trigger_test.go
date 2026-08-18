package croniq

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

// triggerStub is an httptest server that records the last request it
// received (method, path, Authorization header values, decoded JSON body)
// and replies with a canned status + raw JSON body.
type triggerStub struct {
	srv      *httptest.Server
	calls    int
	method   string
	path     string
	authVals []string
	body     map[string]any
}

func newTriggerStub(t *testing.T, status int, respBody string) *triggerStub {
	t.Helper()
	s := &triggerStub{}
	s.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		s.calls++
		s.method = r.Method
		s.path = r.URL.Path
		s.authVals = r.Header.Values("Authorization")
		buf, _ := io.ReadAll(r.Body)
		if len(buf) > 0 {
			s.body = map[string]any{}
			if err := json.Unmarshal(buf, &s.body); err != nil {
				t.Errorf("stub: request body not valid JSON: %v", err)
			}
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = io.WriteString(w, respBody)
	}))
	t.Cleanup(s.srv.Close)
	return s
}

func TestTriggerPostsSnakeCaseBody(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-1","queued":3}`)
	tc := NewTriggerClient(stub.srv.URL).WithAPIKey("croniq_trigger_key")

	resp, err := tc.Trigger(context.Background(), &TriggerRequest{
		JobKey:         "billing:invoice-generate",
		Metadata:       map[string]any{"invoice_id": "inv_42"},
		Require:        []string{"billing"},
		Prefer:         []string{"eu-central"},
		Timeout:        "10m",
		IdempotencyKey: "evt-123",
	})
	if err != nil {
		t.Fatalf("trigger: %v", err)
	}

	if stub.method != http.MethodPost || stub.path != "/v1/trigger" {
		t.Errorf("got %s %s, want POST /v1/trigger", stub.method, stub.path)
	}
	if got := stub.body["job_key"]; got != "billing:invoice-generate" {
		t.Errorf("job_key = %v", got)
	}
	meta, ok := stub.body["metadata"].(map[string]any)
	if !ok || meta["invoice_id"] != "inv_42" {
		t.Errorf("metadata = %v", stub.body["metadata"])
	}
	if req, _ := stub.body["require"].([]any); len(req) != 1 || req[0] != "billing" {
		t.Errorf("require = %v", stub.body["require"])
	}
	if pref, _ := stub.body["prefer"].([]any); len(pref) != 1 || pref[0] != "eu-central" {
		t.Errorf("prefer = %v", stub.body["prefer"])
	}
	if got := stub.body["timeout"]; got != "10m" {
		t.Errorf("timeout = %v", got)
	}
	if got := stub.body["idempotency_key"]; got != "evt-123" {
		t.Errorf("idempotency_key = %v", got)
	}

	if resp.ExecutionID != "exec-1" || resp.Queued != 3 {
		t.Errorf("resp = %+v", resp)
	}
}

func TestTriggerOmitsUnsetOptionalFields(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-1","queued":1}`)
	tc := NewTriggerClient(stub.srv.URL).WithAPIKey("k")

	if _, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "etl:data-sync"}); err != nil {
		t.Fatalf("trigger: %v", err)
	}

	if stub.body["job_key"] != "etl:data-sync" {
		t.Errorf("job_key = %v", stub.body["job_key"])
	}
	for _, key := range []string{"metadata", "require", "prefer", "timeout", "idempotency_key"} {
		if _, present := stub.body[key]; present {
			t.Errorf("optional field %q must be omitted when unset, but was present", key)
		}
	}
}

// TestTriggerMetadataPreservesTypes pins conformance case
// 03-trigger-metadata: nested objects and non-string values survive
// serialisation as JSON (a binding that stringifies or flattens metadata
// fails here).
func TestTriggerMetadataPreservesTypes(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-3","queued":1}`)
	tc := NewTriggerClient(stub.srv.URL)

	_, err := tc.Trigger(context.Background(), &TriggerRequest{
		JobKey: "email:send",
		Metadata: map[string]any{
			"user_id": "u-42",
			"attempt": 2,
			"flags":   map[string]any{"urgent": true},
		},
	})
	if err != nil {
		t.Fatalf("trigger: %v", err)
	}

	meta, ok := stub.body["metadata"].(map[string]any)
	if !ok {
		t.Fatalf("metadata is not an object: %T", stub.body["metadata"])
	}
	if meta["user_id"] != "u-42" {
		t.Errorf("metadata.user_id = %v", meta["user_id"])
	}
	// JSON numbers decode to float64.
	if meta["attempt"] != float64(2) {
		t.Errorf("metadata.attempt = %v (%T), want 2", meta["attempt"], meta["attempt"])
	}
	flags, ok := meta["flags"].(map[string]any)
	if !ok || flags["urgent"] != true {
		t.Errorf("metadata.flags = %v", meta["flags"])
	}
}

func TestTriggerMissingDeduplicatedDefaultsFalse(t *testing.T) {
	// Older servers omit the deduplicated field entirely.
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-1","queued":0}`)
	tc := NewTriggerClient(stub.srv.URL)

	resp, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "etl:data-sync"})
	if err != nil {
		t.Fatalf("trigger: %v", err)
	}
	if resp.Deduplicated {
		t.Errorf("deduplicated = true, want false when the field is absent")
	}
}

func TestTriggerDeduplicatedSurfaced(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-1","queued":0,"deduplicated":true}`)
	tc := NewTriggerClient(stub.srv.URL)

	resp, err := tc.Trigger(context.Background(), &TriggerRequest{
		JobKey:         "etl:data-sync",
		IdempotencyKey: "evt-1",
	})
	if err != nil {
		t.Fatalf("trigger: %v", err)
	}
	if !resp.Deduplicated {
		t.Errorf("deduplicated = false, want true")
	}
	if resp.ExecutionID != "exec-1" {
		t.Errorf("execution_id = %q", resp.ExecutionID)
	}
}

func TestTriggerNonSuccessReturnsServerError(t *testing.T) {
	stub := newTriggerStub(t, http.StatusInternalServerError, `{"error":"boom"}`)
	tc := NewTriggerClient(stub.srv.URL)

	_, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "billing:invoice"})
	if err == nil {
		t.Fatal("expected an error, got nil")
	}
	var se *ServerError
	if !errors.As(err, &se) {
		t.Fatalf("expected *ServerError, got %T", err)
	}
	if se.Status != http.StatusInternalServerError {
		t.Errorf("status = %d, want 500", se.Status)
	}
}

// TestTriggerQueueOverflowSurfacedAsError pins conformance case
// 11-trigger-queue-overflow: the per-job max_queue_depth 429 (issue #299)
// must reach the caller as an error keyed off the status, not the body.
func TestTriggerQueueOverflowSurfacedAsError(t *testing.T) {
	stub := newTriggerStub(t, http.StatusTooManyRequests, `{"execution_id":"","queued":0,"deduplicated":false}`)
	tc := NewTriggerClient(stub.srv.URL)

	_, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "billing:invoice"})
	if err == nil {
		t.Fatal("expected an error for 429, got nil")
	}
	var se *ServerError
	if !errors.As(err, &se) {
		t.Fatalf("expected *ServerError, got %T", err)
	}
	if se.Status != http.StatusTooManyRequests {
		t.Errorf("status = %d, want 429", se.Status)
	}
}

func TestTriggerBlankJobKeyReturnsErrorWithoutCall(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"x","queued":0}`)
	tc := NewTriggerClient(stub.srv.URL)

	_, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "   "})
	if err == nil {
		t.Fatal("expected an error for blank job_key, got nil")
	}
	if stub.calls != 0 {
		t.Errorf("blank job_key must fail before any HTTP call, but server saw %d call(s)", stub.calls)
	}
}

func TestTriggerNilRequestReturnsError(t *testing.T) {
	tc := NewTriggerClient("https://example.test")
	if _, err := tc.Trigger(context.Background(), nil); err == nil {
		t.Fatal("expected an error for nil request, got nil")
	}
}

func TestTriggerAuthAPIKeyHeaderSentOnce(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-1","queued":1}`)
	tc := NewTriggerClient(stub.srv.URL).WithAPIKey("croniq_producer_only_key")

	if _, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "billing:invoice"}); err != nil {
		t.Fatalf("trigger: %v", err)
	}
	if len(stub.authVals) != 1 {
		t.Fatalf("Authorization header sent %d time(s), want exactly 1: %v", len(stub.authVals), stub.authVals)
	}
	if stub.authVals[0] != "ApiKey croniq_producer_only_key" {
		t.Errorf("Authorization = %q", stub.authVals[0])
	}
}

func TestTriggerAuthBearerHeader(t *testing.T) {
	stub := newTriggerStub(t, http.StatusOK, `{"execution_id":"exec-1","queued":1}`)
	tc := NewTriggerClient(stub.srv.URL).WithBearer("eyJ.token.value")

	if _, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "billing:invoice"}); err != nil {
		t.Fatalf("trigger: %v", err)
	}
	if len(stub.authVals) != 1 || stub.authVals[0] != "Bearer eyJ.token.value" {
		t.Errorf("Authorization = %v", stub.authVals)
	}
}

func TestTriggerRequestTimeoutAppliesWhenNoDeadline(t *testing.T) {
	// Server sleeps longer than the client's request timeout; with no
	// deadline on the caller's context, the client must bound the call.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		time.Sleep(200 * time.Millisecond)
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"execution_id":"exec-1","queued":0}`)
	}))
	defer srv.Close()

	tc := NewTriggerClient(srv.URL).WithRequestTimeout(20 * time.Millisecond)
	_, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "slow:job"})
	if err == nil {
		t.Fatal("expected a timeout error, got nil")
	}
}
