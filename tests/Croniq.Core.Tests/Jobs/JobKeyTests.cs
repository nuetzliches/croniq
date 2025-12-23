using System;
using Croniq.Core.Jobs;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Jobs;

public class JobKeyTests
{
    [Fact]
    public void TryParse_handles_variant_and_trims_segments()
    {
        var raw = " ns : job : v1 ";

        JobKey.TryParse(raw, out var jobKey).ShouldBeTrue();

        jobKey.NamespaceSegment.ShouldBe("ns");
        jobKey.JobName.ShouldBe("job");
        jobKey.Variant.ShouldBe("v1");
        jobKey.Value.ShouldBe("ns:job:v1");
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData(":")]
    [InlineData(":job")]
    [InlineData("ns:")]
    [InlineData("too:many:segments:in")]
    public void TryParse_rejects_invalid_input(string? value)
    {
        JobKey.TryParse(value ?? string.Empty, out _).ShouldBeFalse();
    }

    [Fact]
    public void Constructor_throws_on_empty_segments()
    {
        Should.Throw<ArgumentException>(() => new JobKey("", "job"));
        Should.Throw<ArgumentException>(() => new JobKey("ns", " "));
        Should.Throw<ArgumentException>(() => new JobKey(" ", "job"));
    }
}
