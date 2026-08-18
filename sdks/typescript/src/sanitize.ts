/**
 * Hygiene for server-supplied strings.
 *
 * `job_key` and `execution_id` arrive from the Croniq server and are echoed
 * into logs and telemetry. The threat actor is a malicious or compromised
 * server — but not only: in a multi-tenant deployment anyone who can name a
 * job key in the Croniqfile controls a string that round-trips to every
 * runner unchanged. A value carrying CRLF forges log records; one carrying
 * ANSI escapes repaints the operator's terminal.
 *
 * Two layers defend against that:
 *
 * 1. {@link assertSafeAssignmentIdentifiers} rejects an assignment whose
 *    identifiers fall outside the shape Croniq itself defines, so hostile
 *    values never enter the SDK.
 * 2. {@link escapeControlChars} escapes anything a terminal would interpret,
 *    applied wherever this SDK renders text itself (see `logger.ts`).
 */

/**
 * Maximum accepted `job_key` length, counted in Unicode scalar values.
 *
 * The server stores job keys in an unbounded `TEXT` column
 * (`crates/croniq-store/src/migrations/001_initial.sql`), so this bound is the
 * SDK's own. 256 is far above any plausible `namespace:name:variant` while
 * still bounding what a single log line can be made to hold.
 */
export const MAX_JOB_KEY_LENGTH = 256;

/**
 * Maximum accepted `execution_id` length. The server always emits a v4 UUID
 * (36 characters); 64 leaves room for the shorter opaque ids used by mock
 * servers and the conformance suite.
 */
export const MAX_EXECUTION_ID_LENGTH = 64;

/**
 * Whether `codePoint` is one a terminal interprets rather than prints: the C0
 * range (`U+0000`–`U+001F`, covering NUL, CR, LF and the ESC that introduces
 * every ANSI sequence), DEL (`U+007F`), and the C1 range
 * (`U+0080`–`U+009F`).
 *
 * This is the rule for `job_key`, and it is a *denylist* on purpose. An
 * allowlist — say, the character set `Lexer::is_ident_char` accepts for an
 * unquoted key in `crates/croniq-config/src/lexer.rs` — would reject keys a
 * legitimate server can issue: `parse_job_key` (`parser.rs:687-717`) also
 * accepts a `QuotedString` and then enforces only the "two or three
 * colon-separated parts" rule, so `job "billing:monthly invoice" { … }` is
 * legal DSL today, and `POST /v1/jobs` constrains the key not at all. Dropping
 * such an assignment would strand a valid configuration.
 *
 * Rejecting the control classes instead blocks precisely the log-forgery and
 * ANSI-injection payloads without touching anything legitimate. An interior
 * space is accepted; so is any other printable scalar value, in any script.
 */
function isControlScalar(codePoint: number): boolean {
  return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);
}

/**
 * Accepted `execution_id` shape. The server generates these as v4 UUIDs
 * (`Uuid::new_v4()`), which is a strict subset of this pattern; the pattern is
 * kept slightly wider so opaque ids from mock servers and older builds still
 * round-trip. What it excludes is what matters: control characters, ESC,
 * whitespace, and anything else a terminal or log parser reacts to.
 */
const EXECUTION_ID_PATTERN = /^[A-Za-z0-9._:-]+$/;

/**
 * Escape every character a terminal interprets rather than prints: the C0
 * range (including ESC, `0x1B`, which introduces every ANSI sequence), DEL,
 * and the C1 range.
 *
 * This mirrors what Go's `slog` handlers do for attribute values, and is
 * applied here because this SDK's console logger writes the message itself.
 * Field maps do not need it — they are rendered with `JSON.stringify`, which
 * already escapes these.
 */
export function escapeControlChars(value: string): string {
  // eslint-disable-next-line no-control-regex
  return value.replace(/[\u0000-\u001F\u007F-\u009F]/g, (ch) => {
    switch (ch) {
      case '\n':
        return '\\n';
      case '\r':
        return '\\r';
      case '\t':
        return '\\t';
      default:
        return `\\u${ch.charCodeAt(0).toString(16).padStart(4, '0')}`;
    }
  });
}

/**
 * Render a rejected value for a diagnostic: escaped so it cannot forge a
 * record, and truncated so an over-long value cannot flood the log either.
 */
export function previewForLog(value: unknown, maxLength = 120): string {
  const text = typeof value === 'string' ? value : String(value);
  // Slice by scalar values, not UTF-16 code units, so truncation cannot leave
  // a lone surrogate behind.
  const scalars = [...text];
  const clipped = scalars.length > maxLength
    ? `${scalars.slice(0, maxLength).join('')}…`
    : text;
  return escapeControlChars(clipped);
}

/**
 * True when `value` is a job key this runner is willing to act on: non-empty,
 * within {@link MAX_JOB_KEY_LENGTH} scalar values, and free of control
 * characters. Iteration is over scalar values (`for…of`), not UTF-16 code
 * units, so an astral character counts once and a surrogate pair is never
 * inspected half at a time.
 */
export function isSafeJobKey(value: unknown): value is string {
  if (typeof value !== 'string' || value.length === 0) return false;
  let scalars = 0;
  for (const ch of value) {
    if (++scalars > MAX_JOB_KEY_LENGTH) return false;
    if (isControlScalar(ch.codePointAt(0)!)) return false;
  }
  return true;
}

/** True when `value` is an execution id this runner is willing to act on. */
export function isSafeExecutionId(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= MAX_EXECUTION_ID_LENGTH &&
    EXECUTION_ID_PATTERN.test(value)
  );
}

/** The identifier that makes a work assignment unacceptable. */
export type RejectedField = 'execution_id' | 'job_key';

/**
 * Names the identifier that makes a work assignment unacceptable, or
 * `undefined` when both pass. Only the field name is returned — the offending
 * value is reported separately, escaped, so the diagnostic cannot itself become
 * the injection vector.
 *
 * The two outcomes are handled differently by the caller, and the order here
 * is what makes that possible. An unsafe `execution_id` leaves nothing to
 * address the server with, so the assignment is dropped silently. An unsafe
 * `job_key` with a *valid* `execution_id` can still be acked as a failure, so
 * the operator gets a dead-lettered execution naming the problem instead of a
 * silent requeue loop.
 */
export function rejectAssignmentReason(
  executionId: unknown,
  jobKey: unknown,
): RejectedField | undefined {
  if (!isSafeExecutionId(executionId)) return 'execution_id';
  if (!isSafeJobKey(jobKey)) return 'job_key';
  return undefined;
}

/**
 * The `error` string acked for an assignment rejected on `job_key`. Names the
 * field and shows the offending value escaped, so the dead-letter row explains
 * itself without carrying a live payload.
 */
export function rejectionAckError(field: RejectedField, value: unknown): string {
  return `rejected by runner: unsafe ${field} ${previewForLog(value)}`;
}
