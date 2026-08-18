package conformance

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// The guards in this file exist because a loader that silently drops keys has
// no failure mode of its own: the suite stays green precisely when the
// contract stops being enforced (#460). Each test provokes the silence and
// asserts that it is now noisy.

const minimalCase = `
name: strictness probe
runner_config:
  capabilities: ["work"]
handlers:
  - job_key: "work:probe"
    behavior: noop
server_script:
  - on: "POST /v1/work/poll"
    respond:
      status: 200
      body: { work: [], cancel: [] }
expectations:
  duration_max_ms: 500
  http:
    - method: POST
      path: /v1/work/poll
      min_count: 1
`

const minimalTriggerCase = `
name: strictness probe
trigger_config:
  api_key: "croniq_testkey"
trigger_calls:
  - request:
      job_key: "work:probe"
    expect:
      response:
        execution_id: "*"
server_script:
  - on: "POST /v1/trigger"
    respond:
      status: 200
      body: { execution_id: "exec-001", queued: 1, deduplicated: false }
expectations:
  duration_max_ms: 500
  http:
    - method: POST
      path: /v1/trigger
      exact_count: 1
`

// TestLoadFileRejectsUnknownKey pins the load-time failure for a key this
// binding does not model. Without it a schema addition — a new assertion key,
// say — would load cleanly here and simply never be asserted.
func TestLoadFileRejectsUnknownKey(t *testing.T) {
	cases := map[string]string{
		"top level":         strings.Replace(minimalCase, "name: strictness probe", "name: strictness probe\nnot_a_real_key: 1", 1),
		"runner_config":     strings.Replace(minimalCase, `  capabilities: ["work"]`, `  capabilities: ["work"]`+"\n  not_a_real_key: 1", 1),
		"http expectation":  strings.Replace(minimalCase, "      min_count: 1", "      min_count: 1\n      not_a_real_key: 1", 1),
		"handler":           strings.Replace(minimalCase, "    behavior: noop", "    behavior: noop\n    not_a_real_key: 1", 1),
		"nested in respond": strings.Replace(minimalCase, "      status: 200", "      status: 200\n      not_a_real_key: 1", 1),
	}
	for where, yaml := range cases {
		t.Run(where, func(t *testing.T) {
			_, err := LoadFile(writeTemp(t, yaml))
			if err == nil {
				t.Fatalf("expected a load error for an unknown key in %s, got none", where)
			}
			if !strings.Contains(err.Error(), "not_a_real_key") {
				t.Errorf("error should name the offending key, got: %v", err)
			}
		})
	}
}

// TestLoadTriggerFileRejectsUnknownKey is the trigger-side mirror.
func TestLoadTriggerFileRejectsUnknownKey(t *testing.T) {
	cases := map[string]string{
		"top level":    strings.Replace(minimalTriggerCase, "name: strictness probe", "name: strictness probe\nnot_a_real_key: 1", 1),
		"request":      strings.Replace(minimalTriggerCase, `      job_key: "work:probe"`, `      job_key: "work:probe"`+"\n      not_a_real_key: 1", 1),
		"expect":       strings.Replace(minimalTriggerCase, `        execution_id: "*"`, `        execution_id: "*"`+"\n        not_a_real_key: 1", 1),
		"expectations": strings.Replace(minimalTriggerCase, "      exact_count: 1", "      exact_count: 1\n      not_a_real_key: 1", 1),
	}
	for where, yaml := range cases {
		t.Run(where, func(t *testing.T) {
			_, err := LoadTriggerFile(writeTemp(t, yaml))
			if err == nil {
				t.Fatalf("expected a load error for an unknown key in %s, got none", where)
			}
			if !strings.Contains(err.Error(), "not_a_real_key") {
				t.Errorf("error should name the offending key, got: %v", err)
			}
		})
	}
}

// TestLoadFileAcceptsTheKnownVocabulary is the counterweight: strictness must
// reject the unknown without rejecting anything the corpus legitimately uses.
// The corpus itself covers that far better, but this keeps the probe fixtures
// above honest — a fixture that failed to load would make the negative tests
// pass for the wrong reason.
func TestLoadFileAcceptsTheKnownVocabulary(t *testing.T) {
	if _, err := LoadFile(writeTemp(t, minimalCase)); err != nil {
		t.Fatalf("minimal runner case must load: %v", err)
	}
	if _, err := LoadTriggerFile(writeTemp(t, minimalTriggerCase)); err != nil {
		t.Fatalf("minimal trigger case must load: %v", err)
	}
}

// TestBodyAbsentIsAsserted proves the body_absent assertion is wired, not
// merely parsed. Go modelled the key only in a comment until #460 — four
// trigger cases declared it and the suite stayed green.
func TestBodyAbsentIsAsserted(t *testing.T) {
	exp := HTTPExpectation{
		Method:     "POST",
		Path:       "/v1/trigger",
		BodyAbsent: []string{"metadata", "timeout"},
	}
	recorded := RecordedRequest{
		Method: "POST",
		Path:   "/v1/trigger",
		Body:   `{"job_key":"work:probe","metadata":{"a":1}}`,
	}

	spy := &testing.T{}
	assertBodyAbsent(spy, exp, recorded)
	if !spy.Failed() {
		t.Error("body_absent must fail when a listed key is present")
	}

	recorded.Body = `{"job_key":"work:probe"}`
	spy = &testing.T{}
	assertBodyAbsent(spy, exp, recorded)
	if spy.Failed() {
		t.Error("body_absent must pass when every listed key is omitted")
	}
}

func writeTemp(t *testing.T, body string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "case.yaml")
	if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
		t.Fatalf("write temp case: %v", err)
	}
	return path
}
