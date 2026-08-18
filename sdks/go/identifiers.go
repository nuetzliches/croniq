package croniq

import (
	"fmt"
	"strings"
	"unicode/utf8"
)

// Hygiene for server-supplied identifiers.
//
// job_key and execution_id arrive from the Croniq server and are echoed into
// logs and telemetry. The threat actor is a malicious or compromised server —
// but not only: in a multi-tenant deployment anyone who can name a job key in
// the Croniqfile controls a string that round-trips to every runner unchanged.
// A value carrying CRLF forges log records; one carrying ANSI escapes repaints
// the operator's terminal.
//
// This SDK already passes both values to slog as attributes rather than
// interpolating them into a message, and slog's built-in handlers escape
// control characters in rendered values — so the *logging* half of #441 was
// never a problem here. What was missing is the ingest half: rejecting a work
// assignment whose identifiers fall outside the shape Croniq itself defines,
// so hostile values never reach a handler, a trace attribute, or a custom
// slog.Handler that does not escape.

const (
	// maxJobKeyLength bounds an accepted job_key, counted in Unicode scalar
	// values rather than bytes.
	//
	// The server stores job keys in an unbounded TEXT column
	// (crates/croniq-store/src/migrations/001_initial.sql), so this bound is
	// the SDK's own. 256 is far above any plausible namespace:name:variant
	// while still bounding what a single log line can be made to hold.
	maxJobKeyLength = 256

	// maxExecutionIDLength bounds an accepted execution_id. The server always
	// emits a v4 UUID (36 characters); 64 leaves room for the shorter opaque
	// ids used by mock servers and the conformance suite.
	maxExecutionIDLength = 64
)

// isControlScalar reports whether r is a scalar value a terminal interprets
// rather than prints: the C0 range (U+0000–U+001F, covering NUL, CR, LF and the
// ESC that introduces every ANSI sequence), DEL (U+007F), and the C1 range
// (U+0080–U+009F).
//
// This is the rule for job_key, and it is a *denylist* on purpose. An allowlist
// — say, the character set Lexer::is_ident_char accepts for an unquoted key in
// crates/croniq-config/src/lexer.rs — would reject keys a legitimate server can
// issue: parse_job_key (parser.rs:687-717) also accepts a QuotedString and then
// enforces only the "two or three colon-separated parts" rule, so
// `job "billing:monthly invoice" { … }` is legal DSL today, and POST /v1/jobs
// constrains the key not at all. Dropping such an assignment would strand a
// valid configuration.
//
// Rejecting the control classes instead blocks precisely the log-forgery and
// ANSI-injection payloads without touching anything legitimate. An interior
// space is accepted; so is any other printable scalar value, in any script.
func isControlScalar(r rune) bool {
	return r <= 0x1f || (r >= 0x7f && r <= 0x9f)
}

// isExecutionIDChar reports whether b is accepted in an execution id.
//
// The server generates execution ids as v4 UUIDs (Uuid::new_v4()), a strict
// subset of this set. It is kept slightly wider so opaque ids from mock
// servers and older builds still round-trip. What it excludes is what matters:
// control characters, ESC, whitespace, and anything else a terminal or a log
// parser reacts to.
func isExecutionIDChar(b byte) bool {
	switch {
	case b >= 'a' && b <= 'z', b >= 'A' && b <= 'Z', b >= '0' && b <= '9':
		return true
	case b == '-', b == '_', b == '.', b == ':':
		return true
	default:
		return false
	}
}

// IsSafeJobKey reports whether s is a job key this runner is willing to act on:
// non-empty, within maxJobKeyLength scalar values, and free of control
// characters.
//
// Ranging over a string yields runes, so the scan and the length bound are both
// per-scalar-value rather than per-byte — a multi-byte character counts once,
// and no multi-byte sequence is ever inspected a fragment at a time. Invalid
// UTF-8 decodes to utf8.RuneError, which is printable and therefore accepted;
// it cannot forge a record.
func IsSafeJobKey(s string) bool {
	if s == "" {
		return false
	}
	scalars := 0
	for _, r := range s {
		scalars++
		if scalars > maxJobKeyLength {
			return false
		}
		if isControlScalar(r) {
			return false
		}
	}
	return true
}

// IsSafeExecutionID reports whether s is an execution id this runner is willing
// to act on.
func IsSafeExecutionID(s string) bool {
	if s == "" || len(s) > maxExecutionIDLength {
		return false
	}
	for i := 0; i < len(s); i++ {
		if !isExecutionIDChar(s[i]) {
			return false
		}
	}
	return true
}

// rejectAssignmentReason names the field that makes a work assignment
// unacceptable, or "" when both pass.
//
// The two outcomes are handled differently by the caller, and the order here is
// what makes that possible. An unsafe execution_id leaves nothing to address
// the server with, so the assignment is dropped silently. An unsafe job_key
// with a *valid* execution_id can still be acked as a failure, so the operator
// gets a dead-lettered execution naming the problem instead of a silent
// requeue loop.
func rejectAssignmentReason(executionID, jobKey string) string {
	if !IsSafeExecutionID(executionID) {
		return "execution_id"
	}
	if !IsSafeJobKey(jobKey) {
		return "job_key"
	}
	return ""
}

// rejectionAckError builds the error string acked for an assignment rejected on
// job_key. Names the field and shows the offending value escaped, so the
// dead-letter row explains itself without carrying a live payload.
func rejectionAckError(field, value string) string {
	return fmt.Sprintf("rejected by runner: unsafe %s %s", field, previewForLog(value))
}

// escapeControlChars escapes every character a terminal interprets rather than
// prints: the C0 range (ESC, 0x1b, which introduces every ANSI sequence,
// included), DEL, and the C1 range.
//
// slog's built-in handlers already do this for attribute values, so the SDK's
// ordinary logging does not need it. It exists for previewForLog, which
// renders a value we have just refused.
func escapeControlChars(s string) string {
	var b strings.Builder
	b.Grow(len(s))
	for _, r := range s {
		switch {
		case r == utf8.RuneError:
			b.WriteString("\\ufffd")
		case r < 0x20 || r == 0x7f || (r >= 0x80 && r <= 0x9f):
			fmt.Fprintf(&b, "\\u%04x", r)
		default:
			b.WriteRune(r)
		}
	}
	return b.String()
}

// previewForLog renders a *rejected* value for a diagnostic: escaped so it
// cannot forge a record, and truncated so an over-long value cannot flood the
// log either.
func previewForLog(s string) string {
	const maxPreview = 120
	// Truncate by scalar values, not bytes, so a cut cannot land mid-rune.
	if utf8.RuneCountInString(s) > maxPreview {
		runes := []rune(s)
		s = string(runes[:maxPreview]) + "…"
	}
	return escapeControlChars(s)
}
