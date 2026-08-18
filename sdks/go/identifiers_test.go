package croniq

import (
	"strings"
	"testing"
)

const esc = "\x1b"

var (
	crlfKey = "billing:invoice\r\n2026-01-01 ERROR forged record"
	ansiKey = "billing:" + esc + "[31minvoice" + esc + "[0m"
)

func TestIsSafeJobKeyAcceptsEveryKeyTheLexerCanProduceUnquoted(t *testing.T) {
	for _, key := range []string{
		"billing:invoice",
		"ops:health:eu-west",
		"ops:db-dump",
		"a:b",
		"ns:name.with.dots",
		"ns:name_with_underscore",
		"ns:path/segment",
		"ns:*",
		"ns:name+variant@host",
		"ns:what?",
	} {
		if !IsSafeJobKey(key) {
			t.Errorf("IsSafeJobKey(%q) = false, want true", key)
		}
	}
}

// A quoted DSL job key, and anything POST /v1/jobs accepts, must round-trip:
// parse_job_key takes a QuotedString and enforces only the colon-part count, so
// `job "billing:monthly invoice" { … }` is legal today. An allowlist would
// strand these valid configurations.
func TestIsSafeJobKeyAcceptsQuotedAndNonASCIIKeys(t *testing.T) {
	for _, key := range []string{
		"billing:monthly invoice",
		"ops:health check:eu-west",
		"berichte:monatsabschluss (märz)",
		"ops:1С-выгрузка",
		"ops:日次バッチ",
		"ops:deploy#42",
		"ops:a,b;c",
		"ops:100%-check",
		"ops:emoji-🚀",
		// A trailing or interior space cannot forge a record, so it is kept.
		"billing:invoice ",
		"billing: invoice",
	} {
		if !IsSafeJobKey(key) {
			t.Errorf("IsSafeJobKey(%q) = false, want true", key)
		}
	}
}

func TestIsSafeJobKeyRejectsControlCharsAndOutOfBound(t *testing.T) {
	for _, key := range []string{
		crlfKey,
		ansiKey,
		"billing:in\x00voice",
		"billing:in\tvoice",
		"billing:invoice\x7f",
		"billing:invoice\u009b",
		"",
		strings.Repeat("a", maxJobKeyLength+1),
	} {
		if IsSafeJobKey(key) {
			t.Errorf("IsSafeJobKey(%q) = true, want false", key)
		}
	}
	if !IsSafeJobKey(strings.Repeat("a", maxJobKeyLength)) {
		t.Error("the length bound must be inclusive")
	}
}

// The bound counts scalar values, not bytes: an emoji is four UTF-8 bytes but
// one character, so a key of maxJobKeyLength emoji must pass rather than be
// rejected at a quarter of its logical length.
func TestJobKeyLengthBoundCountsScalarValuesNotBytes(t *testing.T) {
	if !IsSafeJobKey(strings.Repeat("🚀", maxJobKeyLength)) {
		t.Error("a key of maxJobKeyLength scalar values must be accepted")
	}
	if IsSafeJobKey(strings.Repeat("🚀", maxJobKeyLength+1)) {
		t.Error("a key over maxJobKeyLength scalar values must be rejected")
	}
}

func TestIsSafeExecutionID(t *testing.T) {
	for _, id := range []string{"6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77", "exec-001"} {
		if !IsSafeExecutionID(id) {
			t.Errorf("IsSafeExecutionID(%q) = false, want true", id)
		}
	}
	for _, id := range []string{
		"exec-001\r\nforged",
		"exec" + esc + "[2J001",
		"",
		strings.Repeat("a", maxExecutionIDLength+1),
	} {
		if IsSafeExecutionID(id) {
			t.Errorf("IsSafeExecutionID(%q) = true, want false", id)
		}
	}
}

func TestRejectAssignmentReason(t *testing.T) {
	cases := []struct {
		executionID, jobKey, want string
	}{
		{"exec-001", "billing:invoice", ""},
		{"exec-001", "billing:monthly invoice", ""},
		{"exec-001", crlfKey, "job_key"},
		{"exec\r\n001", "billing:invoice", "execution_id"},
		// execution_id is checked first: it is what addresses the server, so
		// when both are bad the assignment is unackable and must be dropped.
		{"exec\r\n001", crlfKey, "execution_id"},
	}
	for _, c := range cases {
		if got := rejectAssignmentReason(c.executionID, c.jobKey); got != c.want {
			t.Errorf("rejectAssignmentReason(%q, %q) = %q, want %q",
				c.executionID, c.jobKey, got, c.want)
		}
	}
}

func TestRejectionAckErrorNamesTheFieldAndEscapesTheValue(t *testing.T) {
	got := rejectionAckError("job_key", crlfKey)
	if !strings.Contains(got, "job_key") {
		t.Errorf("ack error does not name the field: %q", got)
	}
	if strings.ContainsAny(got, "\r\n") {
		t.Errorf("ack error carries a raw newline: %q", got)
	}
	if !strings.Contains(got, `\u000d\u000a`) {
		t.Errorf("ack error does not carry the escaped value: %q", got)
	}
}

func TestEscapeControlChars(t *testing.T) {
	cases := []struct{ in, want string }{
		{"a\r\nb", `a\u000d\u000ab`},
		{esc + "[31mred", `\u001b[31mred`},
		{"\u009b", `\u009b`},
		{"billing:invoice — läuft", "billing:invoice — läuft"},
	}
	for _, c := range cases {
		if got := escapeControlChars(c.in); got != c.want {
			t.Errorf("escapeControlChars(%q) = %q, want %q", c.in, got, c.want)
		}
	}
}

func TestPreviewForLogEscapesAndTruncates(t *testing.T) {
	if got := previewForLog(crlfKey); strings.ContainsAny(got, "\r\n") {
		t.Errorf("previewForLog left a raw newline: %q", got)
	}
	if got := previewForLog(ansiKey); strings.Contains(got, esc) {
		t.Errorf("previewForLog left a raw ESC: %q", got)
	}
	if got := previewForLog(strings.Repeat("a", 500)); len(got) > 130 {
		t.Errorf("previewForLog did not truncate: len = %d", len(got))
	}
}
