using Croniq.Runner.Sdk.Internal;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

public class ParseScheduledForTests
{
    [Fact]
    public void ParsesRfc3339()
    {
        var got = ExecutionDispatcher.ParseScheduledFor("2026-06-01T06:00:00Z");
        got.ShouldBe(new DateTimeOffset(2026, 6, 1, 6, 0, 0, TimeSpan.Zero));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    public void AbsentIsNull(string? raw)
    {
        ExecutionDispatcher.ParseScheduledFor(raw).ShouldBeNull();
    }

    [Fact]
    public void UnparseableIsNullNotFireAt()
    {
        ExecutionDispatcher.ParseScheduledFor("not-a-date").ShouldBeNull();
    }
}
