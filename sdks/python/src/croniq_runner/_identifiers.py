"""Hygiene for server-supplied identifiers.

``job_key`` and ``execution_id`` arrive from the Croniq server and are echoed
into logs and telemetry. The threat actor is a malicious or compromised
server — but not only: in a multi-tenant deployment anyone who can name a job
key in the Croniqfile controls a string that round-trips to every runner
unchanged. A value carrying CRLF forges log records; one carrying ANSI escapes
repaints the operator's terminal.

Two layers defend against that:

1. :func:`reject_assignment_reason` rejects a work assignment whose identifiers
   fall outside the shape Croniq itself defines, so hostile values never enter
   the SDK — and, in particular, never reach ``logging.getLogger``.
2. Every remaining log call passes the identifiers as structured fields
   (``extra=…``) rather than interpolating them into the message. Rendering is
   the host ``logging`` configuration's job, exactly as it is for every other
   field an application logs; the SDK deliberately does not second-guess the
   configured formatter by escaping values a second time. :func:`preview_for_log`
   is the one exception, used only for reporting a value we have just refused.
"""

from __future__ import annotations

import re

#: Maximum accepted ``job_key`` length, counted in Unicode scalar values.
#:
#: The server stores job keys in an unbounded ``TEXT`` column
#: (``crates/croniq-store/src/migrations/001_initial.sql``), so this bound is
#: the SDK's own. 256 is far above any plausible ``namespace:name:variant``
#: while still bounding what a single log line can be made to hold.
MAX_JOB_KEY_LENGTH = 256

#: Maximum accepted ``execution_id`` length. The server always emits a v4 UUID
#: (36 characters); 64 leaves room for the shorter opaque ids used by mock
#: servers and the conformance suite.
MAX_EXECUTION_ID_LENGTH = 64

# The rule for ``job_key``: reject the scalar values a terminal interprets
# rather than prints — C0 (``U+0000``-``U+001F``, covering NUL, CR, LF and the
# ESC that introduces every ANSI sequence), DEL (``U+007F``), and C1
# (``U+0080``-``U+009F``).
#
# This is a *denylist* on purpose. An allowlist — say, the character set
# ``Lexer::is_ident_char`` accepts for an unquoted key in
# ``crates/croniq-config/src/lexer.rs`` — would reject keys a legitimate server
# can issue: ``parse_job_key`` (``parser.rs:687-717``) also accepts a
# ``QuotedString`` and then enforces only the "two or three colon-separated
# parts" rule, so ``job "billing:monthly invoice" { … }`` is legal DSL today,
# and ``POST /v1/jobs`` constrains the key not at all. Dropping such an
# assignment would strand a valid configuration.
#
# Rejecting the control classes instead blocks precisely the log-forgery and
# ANSI-injection payloads without touching anything legitimate. An interior
# space is accepted; so is any other printable scalar value, in any script.
_CONTROL_RE = re.compile(r"[\x00-\x1f\x7f-\x9f]")

# The server generates execution ids as v4 UUIDs (``Uuid::new_v4()``), a strict
# subset of this pattern. The pattern is kept slightly wider so opaque ids from
# mock servers and older builds still round-trip. What it excludes is what
# matters: control characters, ESC, whitespace, and anything else a terminal or
# a log parser reacts to.
_EXECUTION_ID_RE = re.compile(r"^[A-Za-z0-9._:-]+$")


def is_safe_job_key(value: object) -> bool:
    """True when ``value`` is a job key this runner is willing to act on.

    Non-empty, within :data:`MAX_JOB_KEY_LENGTH` scalar values, and free of
    control characters. ``str`` iterates over scalar values in Python, so the
    length bound and the scan are both per-character rather than per-byte.
    """
    return (
        isinstance(value, str)
        and 0 < len(value) <= MAX_JOB_KEY_LENGTH
        and _CONTROL_RE.search(value) is None
    )


def is_safe_execution_id(value: object) -> bool:
    """True when ``value`` is an execution id this runner is willing to act on."""
    return (
        isinstance(value, str)
        and 0 < len(value) <= MAX_EXECUTION_ID_LENGTH
        and _EXECUTION_ID_RE.match(value) is not None
    )


def reject_assignment_reason(execution_id: object, job_key: object) -> str | None:
    """Name the field that makes a work assignment unacceptable, else ``None``.

    The two outcomes are handled differently by the caller, and the order here
    is what makes that possible. An unsafe ``execution_id`` leaves nothing to
    address the server with, so the assignment is dropped silently. An unsafe
    ``job_key`` with a *valid* ``execution_id`` can still be acked as a failure,
    so the operator gets a dead-lettered execution naming the problem instead of
    a silent requeue loop.
    """
    if not is_safe_execution_id(execution_id):
        return "execution_id"
    if not is_safe_job_key(job_key):
        return "job_key"
    return None


def rejection_ack_error(field: str, value: object) -> str:
    """Build the ``error`` string acked for an assignment rejected on ``job_key``.

    Names the field and shows the offending value escaped, so the dead-letter
    row explains itself without carrying a live payload.
    """
    return f"rejected by runner: unsafe {field} {preview_for_log(value)}"


def escape_control_chars(value: str) -> str:
    """Escape every character a terminal interprets rather than prints.

    Covers the C0 range (ESC, ``0x1b``, which introduces every ANSI sequence,
    included), DEL, and the C1 range.
    """
    return _CONTROL_RE.sub(lambda m: f"\\u{ord(m.group(0)):04x}", value)


def preview_for_log(value: object, max_length: int = 120) -> str:
    """Render a *rejected* value for a diagnostic.

    Escaped so it cannot forge a record, and truncated so an over-long value
    cannot flood the log either. This is the only place the SDK escapes on the
    host logger's behalf, because it is the only place it knowingly logs a
    hostile string.
    """
    text = value if isinstance(value, str) else str(value)
    if len(text) > max_length:
        text = text[:max_length] + "…"
    return escape_control_chars(text)
