package croniq

import (
	"encoding/json"
	"testing"
)

func TestEnrichEventInjectsAllThreeFieldsWhenTagsPresent(t *testing.T) {
	tags := serializeTags([]string{"env=prod", "team=ops"})
	out := enrichEvent(WorkEvent{Level: "info", Message: "hello"}, "billing:invoice", "shell-runner-1", tags)

	if got := out.Fields["job_key"]; got != "billing:invoice" {
		t.Errorf("job_key = %q, want billing:invoice", got)
	}
	if got := out.Fields["runner_id"]; got != "shell-runner-1" {
		t.Errorf("runner_id = %q, want shell-runner-1", got)
	}
	if got := out.Fields["runner_tags"]; got != `["env=prod","team=ops"]` {
		t.Errorf("runner_tags = %q, want JSON array", got)
	}
}

func TestEnrichEventSkipsRunnerTagsWhenNone(t *testing.T) {
	out := enrichEvent(WorkEvent{Level: "info", Message: "hello"}, "billing:invoice", "shell-runner-1", "")
	if _, ok := out.Fields["runner_tags"]; ok {
		t.Errorf("runner_tags should not be set when runner has no tags")
	}
	if got := out.Fields["runner_id"]; got != "shell-runner-1" {
		t.Errorf("runner_id = %q, want shell-runner-1", got)
	}
}

func TestEnrichEventPreservesCallerProvidedFields(t *testing.T) {
	in := WorkEvent{
		Level:   "warn",
		Message: "hi",
		Fields: map[string]string{
			"job_key":   "explicit:value",
			"runner_id": "explicit-runner",
		},
	}
	tags := serializeTags([]string{"env=prod"})
	out := enrichEvent(in, "auto:job", "auto-runner", tags)

	if got := out.Fields["job_key"]; got != "explicit:value" {
		t.Errorf("job_key was overwritten: got %q", got)
	}
	if got := out.Fields["runner_id"]; got != "explicit-runner" {
		t.Errorf("runner_id was overwritten: got %q", got)
	}
	if got := out.Fields["runner_tags"]; got != `["env=prod"]` {
		t.Errorf("runner_tags = %q", got)
	}

	// Original event must not be mutated.
	if _, ok := in.Fields["runner_tags"]; ok {
		t.Errorf("input event fields were mutated")
	}
}

func TestSerializeTagsEmptyReturnsBlank(t *testing.T) {
	if got := serializeTags(nil); got != "" {
		t.Errorf("got %q, want empty", got)
	}
	if got := serializeTags([]string{}); got != "" {
		t.Errorf("got %q, want empty", got)
	}
}

func TestSerializeTagsRoundTrips(t *testing.T) {
	s := serializeTags([]string{"a=1", "b=2"})
	var back []string
	if err := json.Unmarshal([]byte(s), &back); err != nil {
		t.Fatalf("unmarshal failed: %v", err)
	}
	if len(back) != 2 || back[0] != "a=1" || back[1] != "b=2" {
		t.Errorf("round-trip mismatch: %v", back)
	}
}
