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
        var raw = " t1 : dev : ns : job : v1 ";

        JobKey.TryParse(raw, out var jobKey).ShouldBeTrue();

        jobKey.TenantId.ShouldBe("t1");
        jobKey.EnvironmentTag.ShouldBe("dev");
        jobKey.NamespaceSegment.ShouldBe("ns");
        jobKey.JobName.ShouldBe("job");
        jobKey.Variant.ShouldBe("v1");
        jobKey.Value.ShouldBe("t1:dev:ns:job:v1");
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("too:few:segments")]
    [InlineData("too:many:segments:in:this:string:extra")]
    public void TryParse_rejects_invalid_input(string? value)
    {
        JobKey.TryParse(value ?? string.Empty, out _).ShouldBeFalse();
    }

    [Fact]
    public void Constructor_throws_on_empty_segments()
    {
        Should.Throw<ArgumentException>(() => new JobKey("", "dev", "ns", "job"));
        Should.Throw<ArgumentException>(() => new JobKey("t1", " ", "ns", "job"));
        Should.Throw<ArgumentException>(() => new JobKey("t1", "dev", null!, "job"));
        Should.Throw<ArgumentException>(() => new JobKey("t1", "dev", "ns", ""));
    }
}
