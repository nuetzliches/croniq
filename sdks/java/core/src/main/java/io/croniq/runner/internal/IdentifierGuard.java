package io.croniq.runner.internal;

/**
 * Hygiene for server-supplied identifiers.
 *
 * <p>{@code job_key} and {@code execution_id} arrive from the Croniq server and
 * are echoed into logs and telemetry. The threat actor is a malicious or
 * compromised server — but not only: in a multi-tenant deployment anyone who can
 * name a job key in the Croniqfile controls a string that round-trips to every
 * runner unchanged. A value carrying CRLF forges log records; one carrying ANSI
 * escapes repaints the operator's terminal.
 *
 * <p>{@link #rejectAssignmentReason} rejects a work assignment whose identifiers
 * fall outside the shape Croniq itself defines, so hostile values never enter
 * the SDK. The complementary half is that every log call puts the identifiers
 * into the SLF4J {@code MDC} rather than interpolating them into the message.
 * Rendering is the configured logging backend's job, exactly as it is for every
 * other MDC entry an application sets; the SDK deliberately does not
 * second-guess the configured layout by escaping values a second time.
 * {@link #previewForLog} is the one exception, used only to report a value that
 * has just been refused.
 */
public final class IdentifierGuard {

    /**
     * Maximum accepted {@code job_key} length, counted in Unicode code points
     * rather than {@code char} units. The server stores job keys in an unbounded
     * {@code TEXT} column, so this bound is the SDK's own: far above any
     * plausible {@code namespace:name:variant}, while still bounding what a
     * single log line can be made to hold.
     */
    public static final int MAX_JOB_KEY_LENGTH = 256;

    /**
     * Maximum accepted {@code execution_id} length. The server always emits a v4
     * UUID (36 characters); 64 leaves room for the shorter opaque ids used by
     * mock servers and the conformance suite.
     */
    public static final int MAX_EXECUTION_ID_LENGTH = 64;

    private static final int MAX_PREVIEW_LENGTH = 120;

    private IdentifierGuard() {}

    /**
     * Whether {@code value} is a job key this runner will act on: non-empty,
     * within {@link #MAX_JOB_KEY_LENGTH} code points, and free of control
     * characters.
     *
     * <p>The rule rejects the scalar values a terminal interprets rather than
     * prints — C0 ({@code U+0000}-{@code U+001F}, covering NUL, CR, LF and the
     * ESC that introduces every ANSI sequence), DEL ({@code U+007F}), and C1
     * ({@code U+0080}-{@code U+009F}) — and it is a <em>denylist</em> on purpose.
     * An allowlist, say the set {@code Lexer::is_ident_char} accepts for an
     * unquoted key in {@code crates/croniq-config/src/lexer.rs}, would reject
     * keys a legitimate server can issue: {@code parse_job_key}
     * ({@code parser.rs:687-717}) also accepts a {@code QuotedString} and then
     * enforces only the "two or three colon-separated parts" rule, so
     * {@code job "billing:monthly invoice" { … }} is legal DSL today, and
     * {@code POST /v1/jobs} constrains the key not at all. Dropping such an
     * assignment would strand a valid configuration.
     *
     * <p>Iteration is over code points, not {@code char}, so a supplementary
     * character counts once against the bound and a surrogate pair is never
     * inspected half at a time. An interior space is accepted; so is any other
     * printable scalar value, in any script.
     */
    public static boolean isSafeJobKey(String value) {
        if (value == null || value.isEmpty()) {
            return false;
        }
        if (value.codePointCount(0, value.length()) > MAX_JOB_KEY_LENGTH) {
            return false;
        }
        return value.codePoints().noneMatch(IdentifierGuard::isControlScalar);
    }

    /**
     * Whether {@code codePoint} is a C0, DEL or C1 scalar value — the classes a
     * terminal interprets rather than prints.
     */
    private static boolean isControlScalar(int codePoint) {
        return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);
    }

    /**
     * Whether {@code value} is an execution id this runner will act on.
     *
     * <p>The server generates execution ids as v4 UUIDs ({@code Uuid::new_v4()}),
     * a strict subset of the accepted set. The set is kept slightly wider so
     * opaque ids from mock servers and older builds still round-trip. What it
     * excludes is what matters: control characters, ESC, whitespace, and
     * anything else a terminal or a log parser reacts to.
     */
    public static boolean isSafeExecutionId(String value) {
        if (value == null || value.isEmpty() || value.length() > MAX_EXECUTION_ID_LENGTH) {
            return false;
        }
        for (int i = 0; i < value.length(); i++) {
            if (!isExecutionIdChar(value.charAt(i))) {
                return false;
            }
        }
        return true;
    }

    /**
     * Names the field that makes a work assignment unacceptable, or {@code null}
     * when both pass.
     *
     * <p>The two outcomes are handled differently by the caller, and the order
     * here is what makes that possible. An unsafe {@code execution_id} leaves
     * nothing to address the server with, so the assignment is dropped silently.
     * An unsafe {@code job_key} with a <em>valid</em> {@code execution_id} can
     * still be acked as a failure, so the operator gets a dead-lettered
     * execution naming the problem instead of a silent requeue loop.
     */
    public static String rejectAssignmentReason(String executionId, String jobKey) {
        if (!isSafeExecutionId(executionId)) {
            return "execution_id";
        }
        if (!isSafeJobKey(jobKey)) {
            return "job_key";
        }
        return null;
    }

    /**
     * Escapes every character a terminal interprets rather than prints: the C0
     * range (ESC, {@code 0x1b}, which introduces every ANSI sequence, included),
     * DEL, and the C1 range.
     */
    public static String escapeControlChars(String value) {
        StringBuilder out = new StringBuilder(value.length());
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (c < 0x20 || (c >= 0x7f && c <= 0x9f)) {
                out.append(String.format("\\u%04x", (int) c));
            } else {
                out.append(c);
            }
        }
        return out.toString();
    }

    /**
     * Renders a <em>rejected</em> value for a diagnostic: escaped so it cannot
     * forge a record, and truncated so an over-long value cannot flood the log
     * either.
     */
    public static String previewForLog(String value) {
        String text = value == null ? "<null>" : value;
        // Truncate on a code-point boundary so a cut cannot leave a lone
        // surrogate behind.
        if (text.codePointCount(0, text.length()) > MAX_PREVIEW_LENGTH) {
            int end = text.offsetByCodePoints(0, MAX_PREVIEW_LENGTH);
            text = text.substring(0, end) + "…";
        }
        return escapeControlChars(text);
    }

    /**
     * Builds the {@code error} string acked for an assignment rejected on
     * {@code job_key}. Names the field and shows the offending value escaped, so
     * the dead-letter row explains itself without carrying a live payload.
     */
    public static String rejectionAckError(String field, String value) {
        return "rejected by runner: unsafe " + field + " " + previewForLog(value);
    }

    private static boolean isExecutionIdChar(char c) {
        return (c >= 'a' && c <= 'z')
                || (c >= 'A' && c <= 'Z')
                || (c >= '0' && c <= '9')
                || c == '-'
                || c == '_'
                || c == '.'
                || c == ':';
    }
}
