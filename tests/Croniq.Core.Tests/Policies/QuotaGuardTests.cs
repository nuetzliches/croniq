using System;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using FluentAssertions;
using Xunit;

namespace Croniq.Core.Tests.Policies;

public class QuotaGuardTests
{
    private static readonly JobKey Job = new("t", "dev", "ns", "job");

    [Fact]
    public void Enforces_rate_limit_per_minute()
    {
        var guard = new InMemoryQuotaGuard();
        var options = new QuotaOptions { MaxTriggersPerMinute = 2, MaxParallelExecutionsPerJob = 5 };
        var now = DateTimeOffset.UtcNow;

        guard.TryAcquire(Job, options, now, out var retry1).Should().BeTrue();
        retry1.Should().BeNull();

        guard.TryAcquire(Job, options, now.AddSeconds(1), out var retry2).Should().BeTrue();
        retry2.Should().BeNull();

        guard.TryAcquire(Job, options, now.AddSeconds(2), out var retry3).Should().BeFalse();
        retry3.Should().NotBeNull();
        retry3!.Value.Should()
            .BeOnOrAfter(now.AddMinutes(1).AddSeconds(-1))
            .And.BeOnOrBefore(now.AddMinutes(1).AddSeconds(1));
    }

    [Fact]
    public void Enforces_concurrency_limit()
    {
        var guard = new InMemoryQuotaGuard();
        var options = new QuotaOptions { MaxTriggersPerMinute = 10, MaxParallelExecutionsPerJob = 1 };
        var now = DateTimeOffset.UtcNow;

        guard.TryAcquire(Job, options, now, out var retry1).Should().BeTrue();
        retry1.Should().BeNull();

        guard.TryAcquire(Job, options, now.AddMilliseconds(10), out var retry2).Should().BeFalse();
        retry2.Should().NotBeNull();

        guard.Release(Job);

        guard.TryAcquire(Job, options, now.AddMilliseconds(20), out var retry3).Should().BeTrue();
        retry3.Should().BeNull();
    }
}
