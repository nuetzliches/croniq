using System.Text;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Hygiene for server-supplied identifiers.
/// </summary>
/// <remarks>
/// <para>
/// <c>job_key</c> and <c>execution_id</c> arrive from the Croniq server and are
/// echoed into logs and telemetry. The threat actor is a malicious or
/// compromised server — but not only: in a multi-tenant deployment anyone who
/// can name a job key in the Croniqfile controls a string that round-trips to
/// every runner unchanged. A value carrying CRLF forges log records; one
/// carrying ANSI escapes repaints the operator's terminal.
/// </para>
/// <para>
/// <see cref="RejectAssignmentReason"/> rejects a work assignment whose
/// identifiers fall outside the shape Croniq itself defines, so hostile values
/// never enter the SDK — in particular they never reach
/// <c>ILoggerFactory.CreateLogger</c>, whose category cache is permanent and
/// which some sinks map to a filename.
/// </para>
/// <para>
/// The complementary half is that every log call passes the identifiers as
/// structured state (a logging scope) rather than interpolating them into the
/// message. Rendering is the configured <c>ILogger</c> provider's job, exactly
/// as it is for every other property an application logs; the SDK deliberately
/// does not second-guess the configured formatter by escaping values a second
/// time. <see cref="PreviewForLog"/> is the one exception, used only to report
/// a value that has just been refused.
/// </para>
/// </remarks>
internal static class IdentifierGuard
{
    /// <summary>
    /// Maximum accepted <c>job_key</c> length, counted in Unicode scalar values
    /// rather than UTF-16 code units. The server stores job keys in an
    /// unbounded <c>TEXT</c> column, so this bound is the SDK's own: far above
    /// any plausible <c>namespace:name:variant</c>, while still bounding what a
    /// single log line can be made to hold.
    /// </summary>
    internal const int MaxJobKeyLength = 256;

    /// <summary>
    /// Maximum accepted <c>execution_id</c> length. The server always emits a
    /// v4 UUID (36 characters); 64 leaves room for the shorter opaque ids used
    /// by mock servers and the conformance suite.
    /// </summary>
    internal const int MaxExecutionIdLength = 64;

    private const int MaxPreviewLength = 120;

    /// <summary>
    /// Whether <paramref name="value"/> is a job key this runner will act on:
    /// non-empty, within <see cref="MaxJobKeyLength"/> scalar values, and free
    /// of control characters.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The rule rejects the scalar values a terminal interprets rather than
    /// prints — C0 (<c>U+0000</c>–<c>U+001F</c>, covering NUL, CR, LF and the
    /// ESC that introduces every ANSI sequence), DEL (<c>U+007F</c>), and C1
    /// (<c>U+0080</c>–<c>U+009F</c>) — and it is a <em>denylist</em> on purpose.
    /// An allowlist, say the set <c>Lexer::is_ident_char</c> accepts for an
    /// unquoted key in <c>crates/croniq-config/src/lexer.rs</c>, would reject
    /// keys a legitimate server can issue: <c>parse_job_key</c>
    /// (<c>parser.rs:687-717</c>) also accepts a <c>QuotedString</c> and then
    /// enforces only the "two or three colon-separated parts" rule, so
    /// <c>job "billing:monthly invoice" { … }</c> is legal DSL today, and
    /// <c>POST /v1/jobs</c> constrains the key not at all. Dropping such an
    /// assignment would strand a valid configuration.
    /// </para>
    /// <para>
    /// Iteration is over runes, not <c>char</c>, so an astral character counts
    /// once against the bound and a surrogate pair is never inspected half at a
    /// time. An interior space is accepted; so is any other printable scalar
    /// value, in any script.
    /// </para>
    /// </remarks>
    internal static bool IsSafeJobKey(string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return false;
        }
        var scalars = 0;
        foreach (var rune in value.EnumerateRunes())
        {
            if (++scalars > MaxJobKeyLength)
            {
                return false;
            }
            if (IsControlScalar(rune.Value))
            {
                return false;
            }
        }
        return true;
    }

    /// <summary>
    /// Whether <paramref name="codePoint"/> is a C0, DEL or C1 scalar value —
    /// the classes a terminal interprets rather than prints.
    /// </summary>
    private static bool IsControlScalar(int codePoint) =>
        codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);

    /// <summary>
    /// Whether <paramref name="value"/> is an execution id this runner will act
    /// on.
    /// </summary>
    /// <remarks>
    /// The server generates execution ids as v4 UUIDs (<c>Uuid::new_v4()</c>),
    /// a strict subset of the accepted set. The set is kept slightly wider so
    /// opaque ids from mock servers and older builds still round-trip. What it
    /// excludes is what matters: control characters, ESC, whitespace, and
    /// anything else a terminal or a log parser reacts to.
    /// </remarks>
    internal static bool IsSafeExecutionId(string? value)
    {
        if (string.IsNullOrEmpty(value) || value.Length > MaxExecutionIdLength)
        {
            return false;
        }
        foreach (var c in value)
        {
            var ok = c is >= 'a' and <= 'z'
                or >= 'A' and <= 'Z'
                or >= '0' and <= '9'
                or '-' or '_' or '.' or ':';
            if (!ok)
            {
                return false;
            }
        }
        return true;
    }

    /// <summary>
    /// Names the field that makes a work assignment unacceptable, or
    /// <c>null</c> when both pass.
    /// </summary>
    /// <remarks>
    /// The two outcomes are handled differently by the caller, and the order
    /// here is what makes that possible. An unsafe <c>execution_id</c> leaves
    /// nothing to address the server with, so the assignment is dropped
    /// silently. An unsafe <c>job_key</c> with a <em>valid</em>
    /// <c>execution_id</c> can still be acked as a failure, so the operator
    /// gets a dead-lettered execution naming the problem instead of a silent
    /// requeue loop.
    /// </remarks>
    internal static string? RejectAssignmentReason(string? executionId, string? jobKey)
    {
        if (!IsSafeExecutionId(executionId))
        {
            return "execution_id";
        }
        if (!IsSafeJobKey(jobKey))
        {
            return "job_key";
        }
        return null;
    }

    /// <summary>
    /// Escapes every character a terminal interprets rather than prints: the C0
    /// range (ESC, <c>0x1b</c>, which introduces every ANSI sequence, included),
    /// DEL, and the C1 range.
    /// </summary>
    internal static string EscapeControlChars(string value)
    {
        var builder = new StringBuilder(value.Length);
        foreach (var c in value)
        {
            if (c < 0x20 || (c >= 0x7f && c <= 0x9f))
            {
                builder.Append("\\u").Append(((int)c).ToString("x4", System.Globalization.CultureInfo.InvariantCulture));
            }
            else
            {
                builder.Append(c);
            }
        }
        return builder.ToString();
    }

    /// <summary>
    /// Renders a <em>rejected</em> value for a diagnostic: escaped so it cannot
    /// forge a record, and truncated so an over-long value cannot flood the log
    /// either.
    /// </summary>
    internal static string PreviewForLog(string? value)
    {
        var text = value ?? "<null>";
        // Truncate on a rune boundary so a cut cannot leave a lone surrogate.
        var scalars = 0;
        var cut = -1;
        for (var i = 0; i < text.Length; i += char.IsSurrogatePair(text, i) ? 2 : 1)
        {
            if (++scalars > MaxPreviewLength)
            {
                cut = i;
                break;
            }
        }
        if (cut >= 0)
        {
            text = string.Concat(text.AsSpan(0, cut), "…");
        }
        return EscapeControlChars(text);
    }

    /// <summary>
    /// Builds the <c>error</c> string acked for an assignment rejected on
    /// <c>job_key</c>. Names the field and shows the offending value escaped, so
    /// the dead-letter row explains itself without carrying a live payload.
    /// </summary>
    internal static string RejectionAckError(string field, string? value) =>
        $"rejected by runner: unsafe {field} {PreviewForLog(value)}";
}
