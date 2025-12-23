using System;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Policies;

public class QuotaGuardTests
{
    private static readonly JobKey Job = new("ns", "job");

    [Fact]
    public void Enforces_rate_limit_per_minute()
    {
        var guard = new InMemoryQuotaGuard();
        var options = new QuotaOptions { MaxTriggersPerMinute = 2, MaxParallelExecutionsPerJob = 5 };
        var now = DateTimeOffset.UtcNow;

        guard.TryAcquire(Job, options, now, out var retry1).ShouldBeTrue();
        retry1.ShouldBeNull();

        guard.TryAcquire(Job, options, now.AddSeconds(1), out var retry2).ShouldBeTrue();
        retry2.ShouldBeNull();

        guard.TryAcquire(Job, options, now.AddSeconds(2), out var retry3).ShouldBeFalse();
        retry3.ShouldNotBeNull();
        retry3!.Value.ShouldBeInRange(
            now.AddMinutes(1).AddSeconds(-1),
            now.AddMinutes(1).AddSeconds(1));
    }

    [Fact]
    public void Enforces_concurrency_limit()
    {
        var guard = new InMemoryQuotaGuard();
        var options = new QuotaOptions { MaxTriggersPerMinute = 10, MaxParallelExecutionsPerJob = 1 };
        var now = DateTimeOffset.UtcNow;

        guard.TryAcquire(Job, options, now, out var retry1).ShouldBeTrue();
        retry1.ShouldBeNull();

        guard.TryAcquire(Job, options, now.AddMilliseconds(10), out var retry2).ShouldBeFalse();
        retry2.ShouldNotBeNull();

        guard.Release(Job);

        guard.TryAcquire(Job, options, now.AddMilliseconds(20), out var retry3).ShouldBeTrue();
        retry3.ShouldBeNull();
    }
}
