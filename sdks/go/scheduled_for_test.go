package croniq

import (
	"encoding/json"
	"testing"
	"time"
)

func TestParseScheduledFor(t *testing.T) {
	got := parseScheduledFor("2026-06-01T06:00:00Z")
	want := time.Date(2026, 6, 1, 6, 0, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("parseScheduledFor = %v, want %v", got, want)
	}
}

func TestParseScheduledForAbsentIsZero(t *testing.T) {
	if !parseScheduledFor("").IsZero() {
		t.Fatal("empty scheduled_for should yield the zero time")
	}
}

func TestParseScheduledForUnparseableIsZero(t *testing.T) {
	if !parseScheduledFor("not-a-date").IsZero() {
		t.Fatal("unparseable scheduled_for should yield the zero time, not fire_at")
	}
}

func TestWorkAssignmentDecodesScheduledFor(t *testing.T) {
	// Present.
	var withField WorkAssignment
	if err := json.Unmarshal([]byte(`{
		"execution_id":"e","job_key":"billing:report",
		"fire_at":"2026-06-08T00:05:00Z","scheduled_for":"2026-06-01T06:00:00Z",
		"attempt":3,"metadata":{},"timeout":"15m"
	}`), &withField); err != nil {
		t.Fatal(err)
	}
	if withField.ScheduledFor != "2026-06-01T06:00:00Z" {
		t.Fatalf("ScheduledFor = %q", withField.ScheduledFor)
	}

	// Absent (older server) → empty string.
	var without WorkAssignment
	if err := json.Unmarshal([]byte(`{
		"execution_id":"e","job_key":"j","fire_at":"2026-05-23T10:00:00Z",
		"attempt":1,"metadata":{},"timeout":"5m"
	}`), &without); err != nil {
		t.Fatal(err)
	}
	if without.ScheduledFor != "" {
		t.Fatalf("expected empty ScheduledFor, got %q", without.ScheduledFor)
	}
}
