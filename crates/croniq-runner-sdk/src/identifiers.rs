//! Hygiene for server-supplied identifiers (issue #441; review 2026-10-24,
//! R-7 / K-2).
//!
//! `job_key` and `execution_id` arrive from the Croniq server and are echoed
//! into logs, into the ack's error text, and — for `execution_id` — into the
//! path of the log-events POST. The threat actor is a malicious or compromised
//! server, but not only: anyone who can name a job key in the Croniqfile
//! controls a string that round-trips to every runner unchanged. A value
//! carrying CRLF forges log records; one carrying ANSI escapes repaints the
//! operator's terminal; an `execution_id` of `..` steers the authenticated
//! events POST onto another path of the same origin.
//!
//! The five other SDKs reject such assignments on ingest (conformance cases 13
//! and 14); this is the same rule, ported from `sdks/go/identifiers.go`, so the
//! reference SDK and `croniq-shell-runner` no longer diverge there.

/// Upper bound on an accepted `job_key`, in Unicode scalar values. The server
/// stores keys in an unbounded TEXT column, so this bound is the SDK's own:
/// far above any plausible `namespace:name:variant`, while still bounding what
/// one log line can be made to hold.
const MAX_JOB_KEY_LENGTH: usize = 256;

/// Upper bound on an accepted `execution_id`. The server always emits a v4
/// UUID (36 characters); 64 leaves room for the shorter opaque ids of mock
/// servers and the conformance suite.
const MAX_EXECUTION_ID_LENGTH: usize = 64;

/// Scalar values a terminal interprets rather than prints: C0 (NUL, CR, LF and
/// the ESC that introduces every ANSI sequence), DEL, and C1.
///
/// A denylist on purpose: `job "billing:monthly invoice" { … }` is legal DSL,
/// and an allowlist modelled on the unquoted-key lexer would strand it.
fn is_control_scalar(c: char) -> bool {
    c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c)
}

/// Bytes accepted in an execution id: a superset of a v4 UUID, wide enough for
/// the opaque ids of mock servers, and free of anything a terminal, a log
/// parser or a URL path reacts to — except `.`, which is why `.` and `..` are
/// refused as whole values in [`is_safe_execution_id`].
fn is_execution_id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':')
}

/// Whether `s` is a job key this runner is willing to act on: non-empty,
/// within [`MAX_JOB_KEY_LENGTH`] scalar values, and free of control characters.
pub fn is_safe_job_key(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_JOB_KEY_LENGTH && !s.chars().any(is_control_scalar)
}

/// Whether `s` is an execution id this runner is willing to act on.
///
/// `.` and `..` are refused although every byte is allowed: as a path segment
/// they are dot-segments, which URL normalisation resolves — percent-encoded
/// or not — so `/v1/work/../events` would leave the execution's own path.
/// The Go rule accepts them; the Rust SDK is the one that puts the id into a
/// URL path, so it is the one that has to refuse them.
pub fn is_safe_execution_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_EXECUTION_ID_LENGTH
        && s != "."
        && s != ".."
        && s.bytes().all(is_execution_id_byte)
}

/// Which field makes a work assignment unacceptable, if any.
///
/// The order matters to the caller. An unsafe `execution_id` leaves nothing to
/// address the server with, so the assignment is dropped (case 14). An unsafe
/// `job_key` with a valid `execution_id` is acked as a failure, so the operator
/// gets a dead-lettered execution that explains itself instead of a silent
/// requeue loop (case 13).
pub(crate) fn reject_reason(execution_id: &str, job_key: &str) -> Option<RejectedField> {
    if !is_safe_execution_id(execution_id) {
        Some(RejectedField::ExecutionId)
    } else if !is_safe_job_key(job_key) {
        Some(RejectedField::JobKey)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RejectedField {
    ExecutionId,
    JobKey,
}

/// The ack error for an assignment refused on its `job_key`: names the field
/// and shows the value escaped, so the dead-letter row carries no live payload.
pub(crate) fn rejection_ack_error(job_key: &str) -> String {
    format!(
        "rejected by runner: unsafe job_key {}",
        preview_for_log(job_key)
    )
}

/// Render a *refused* value for a diagnostic: control characters escaped as
/// `\uXXXX`, so it cannot forge a record, and truncated so it cannot flood one.
pub(crate) fn preview_for_log(s: &str) -> String {
    const MAX_PREVIEW: usize = 120;
    let mut out = String::with_capacity(s.len().min(MAX_PREVIEW * 2));
    for (i, c) in s.chars().enumerate() {
        if i == MAX_PREVIEW {
            out.push('…');
            break;
        }
        if is_control_scalar(c) {
            out.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The values of conformance cases 13 and 14.
    const HOSTILE_KEY: &str = "billing:\x1b[31minvoice\r\n2026-01-01 ERROR forged record\x1b[0m";
    const HOSTILE_ID: &str = "exec\x1b[2J\r\n001";

    #[test]
    fn ordinary_identifiers_pass() {
        assert!(is_safe_job_key("billing:invoice"));
        assert!(is_safe_job_key("billing:monthly invoice"));
        assert!(is_safe_job_key("räumung:büro"));
        assert!(is_safe_execution_id("4f9c1c0e-8a5b-4d4e-9f0a-1b2c3d4e5f60"));
        assert!(is_safe_execution_id("exec-drain"));
        assert!(is_safe_execution_id("a.b"));
        assert_eq!(reject_reason("exec-1", "billing:invoice"), None);
    }

    #[test]
    fn hostile_job_key_is_refused_but_acked() {
        assert!(!is_safe_job_key(HOSTILE_KEY));
        assert_eq!(
            reject_reason("exec-hostile", HOSTILE_KEY),
            Some(RejectedField::JobKey)
        );
        let msg = rejection_ack_error(HOSTILE_KEY);
        assert!(
            !msg.contains('\x1b') && !msg.contains('\r') && !msg.contains('\n'),
            "{msg:?}"
        );
        assert!(msg.contains("\\u001b"), "{msg:?}");
    }

    #[test]
    fn hostile_execution_id_is_refused_first() {
        assert!(!is_safe_execution_id(HOSTILE_ID));
        // Even with a hostile job key too: there is nothing to ack with.
        assert_eq!(
            reject_reason(HOSTILE_ID, HOSTILE_KEY),
            Some(RejectedField::ExecutionId)
        );
    }

    #[test]
    fn path_steering_execution_ids_are_refused() {
        for id in ["", ".", "..", "../x", "a/b", "a%2e", "a b", "a?b", "a#b"] {
            assert!(!is_safe_execution_id(id), "{id:?} must be refused");
        }
    }

    #[test]
    fn lengths_are_bounded() {
        assert!(is_safe_job_key(&"k".repeat(MAX_JOB_KEY_LENGTH)));
        assert!(!is_safe_job_key(&"k".repeat(MAX_JOB_KEY_LENGTH + 1)));
        // Counted in scalar values, not bytes.
        assert!(is_safe_job_key(&"ä".repeat(MAX_JOB_KEY_LENGTH)));
        assert!(is_safe_execution_id(&"e".repeat(MAX_EXECUTION_ID_LENGTH)));
        assert!(!is_safe_execution_id(
            &"e".repeat(MAX_EXECUTION_ID_LENGTH + 1)
        ));
    }

    #[test]
    fn c1_and_del_are_control_characters() {
        assert!(!is_safe_job_key("a\u{7f}b"));
        assert!(!is_safe_job_key("a\u{9b}31mb")); // C1 CSI
        assert!(is_safe_job_key("a\u{a0}b")); // NBSP is printable
    }

    #[test]
    fn preview_truncates() {
        let p = preview_for_log(&"x".repeat(500));
        assert_eq!(p.chars().count(), 121);
        assert!(p.ends_with('…'));
    }
}
