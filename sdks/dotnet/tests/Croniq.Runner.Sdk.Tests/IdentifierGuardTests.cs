using Croniq.Runner.Sdk.Internal;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Ingest validation and log hygiene for server-supplied identifiers (#441).
/// </summary>
public class IdentifierGuardTests
{
    private const string Esc = "\u001b";
    private const string CrlfKey = "billing:invoice\r\n2026-01-01 ERROR forged record";
    private static readonly string AnsiKey = $"billing:{Esc}[31minvoice{Esc}[0m";

    [Theory]
    [InlineData("billing:invoice")]
    [InlineData("ops:health:eu-west")]
    [InlineData("ops:db-dump")]
    [InlineData("a:b")]
    [InlineData("ns:name.with.dots")]
    [InlineData("ns:name_with_underscore")]
    [InlineData("ns:path/segment")]
    [InlineData("ns:*")]
    [InlineData("ns:name+variant@host")]
    [InlineData("ns:what?")]
    public void AcceptsEveryKeyTheLexerCanProduceUnquoted(string key)
    {
        Assert.True(IdentifierGuard.IsSafeJobKey(key));
    }

    /// <summary>
    /// `job "billing:monthly invoice" { … }` is legal DSL: parse_job_key accepts
    /// a QuotedString and enforces only the colon-part count, and POST /v1/jobs
    /// constrains the key not at all. An allowlist would strand these valid
    /// configurations, so interior spaces and non-ASCII text must pass.
    /// </summary>
    [Theory]
    [InlineData("billing:monthly invoice")]
    [InlineData("ops:health check:eu-west")]
    [InlineData("berichte:monatsabschluss (märz)")]
    [InlineData("ops:1С-выгрузка")]
    [InlineData("ops:日次バッチ")]
    [InlineData("ops:deploy#42")]
    [InlineData("ops:a,b;c")]
    [InlineData("ops:100%-check")]
    [InlineData("ops:emoji-🚀")]
    // A trailing or interior space cannot forge a record, so it is accepted.
    [InlineData("billing:invoice ")]
    [InlineData("billing: invoice")]
    public void AcceptsQuotedAndNonAsciiKeys(string key)
    {
        Assert.True(IdentifierGuard.IsSafeJobKey(key));
    }

    [Fact]
    public void RejectsControlCharactersInJobKeys()
    {
        Assert.False(IdentifierGuard.IsSafeJobKey(CrlfKey));
        Assert.False(IdentifierGuard.IsSafeJobKey(AnsiKey));
        Assert.False(IdentifierGuard.IsSafeJobKey("billing:in\u0000voice"));
        Assert.False(IdentifierGuard.IsSafeJobKey("billing:in\tvoice"));
        Assert.False(IdentifierGuard.IsSafeJobKey("billing:invoice\u007f"));
        Assert.False(IdentifierGuard.IsSafeJobKey("billing:invoice\u009b"));
        Assert.False(IdentifierGuard.IsSafeJobKey(""));
        Assert.False(IdentifierGuard.IsSafeJobKey(null));
    }

    [Fact]
    public void JobKeyLengthBoundIsInclusive()
    {
        Assert.True(IdentifierGuard.IsSafeJobKey(new string('a', IdentifierGuard.MaxJobKeyLength)));
        Assert.False(IdentifierGuard.IsSafeJobKey(new string('a', IdentifierGuard.MaxJobKeyLength + 1)));
    }

    /// <summary>
    /// The bound counts scalar values, not UTF-16 code units: an astral
    /// character is two units but one character, so a key of MaxJobKeyLength
    /// emoji must pass rather than be rejected at half its logical length.
    /// </summary>
    [Fact]
    public void JobKeyLengthBoundCountsScalarValues()
    {
        Assert.True(IdentifierGuard.IsSafeJobKey(
            string.Concat(Enumerable.Repeat("🚀", IdentifierGuard.MaxJobKeyLength))));
        Assert.False(IdentifierGuard.IsSafeJobKey(
            string.Concat(Enumerable.Repeat("🚀", IdentifierGuard.MaxJobKeyLength + 1))));
    }

    [Fact]
    public void ExecutionIdAcceptsUuidAndOpaqueIds()
    {
        Assert.True(IdentifierGuard.IsSafeExecutionId("6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77"));
        Assert.True(IdentifierGuard.IsSafeExecutionId("exec-001"));
    }

    [Fact]
    public void ExecutionIdRejectsHostileAndOutOfBound()
    {
        Assert.False(IdentifierGuard.IsSafeExecutionId("exec-001\r\nforged"));
        Assert.False(IdentifierGuard.IsSafeExecutionId($"exec{Esc}[2J001"));
        Assert.False(IdentifierGuard.IsSafeExecutionId(""));
        Assert.False(IdentifierGuard.IsSafeExecutionId(
            new string('a', IdentifierGuard.MaxExecutionIdLength + 1)));
    }

    [Fact]
    public void RejectAssignmentReasonNamesTheField()
    {
        Assert.Null(IdentifierGuard.RejectAssignmentReason("exec-001", "billing:invoice"));
        Assert.Null(IdentifierGuard.RejectAssignmentReason("exec-001", "billing:monthly invoice"));
        Assert.Equal("job_key", IdentifierGuard.RejectAssignmentReason("exec-001", CrlfKey));
        Assert.Equal("execution_id", IdentifierGuard.RejectAssignmentReason("exec\r\n001", "billing:invoice"));
        // execution_id is checked first: it is what addresses the server, so
        // when both are bad the assignment is unackable and must be dropped.
        Assert.Equal("execution_id", IdentifierGuard.RejectAssignmentReason("exec\r\n001", CrlfKey));
    }

    [Fact]
    public void RejectionAckErrorNamesTheFieldAndEscapesTheValue()
    {
        var message = IdentifierGuard.RejectionAckError("job_key", CrlfKey);
        Assert.Contains("job_key", message, StringComparison.Ordinal);
        Assert.DoesNotContain("\r", message, StringComparison.Ordinal);
        Assert.DoesNotContain("\n", message, StringComparison.Ordinal);
        Assert.Contains("\\u000d\\u000a", message, StringComparison.Ordinal);
    }

    [Fact]
    public void EscapeControlCharsCoversC0EscAndC1()
    {
        Assert.Equal("a\\u000d\\u000ab", IdentifierGuard.EscapeControlChars("a\r\nb"));
        Assert.Equal("\\u001b[31mred", IdentifierGuard.EscapeControlChars($"{Esc}[31mred"));
        Assert.Equal("\\u009b", IdentifierGuard.EscapeControlChars("\u009b"));
        Assert.Equal("billing:invoice — läuft", IdentifierGuard.EscapeControlChars("billing:invoice — läuft"));
    }

    [Fact]
    public void PreviewForLogEscapesAndTruncates()
    {
        Assert.DoesNotContain("\n", IdentifierGuard.PreviewForLog(CrlfKey), StringComparison.Ordinal);
        Assert.DoesNotContain(Esc, IdentifierGuard.PreviewForLog(AnsiKey), StringComparison.Ordinal);
        Assert.True(IdentifierGuard.PreviewForLog(new string('a', 500)).Length <= 121);
    }
}
