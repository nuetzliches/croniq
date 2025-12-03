using System;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
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

        Assert.True(guard.TryAcquire(Job, options, now, out var retry1));
        Assert.Null(retry1);

        Assert.True(guard.TryAcquire(Job, options, now.AddSeconds(1), out var retry2));
        Assert.Null(retry2);

        Assert.False(guard.TryAcquire(Job, options, now.AddSeconds(2), out var retry3));
        Assert.NotNull(retry3);
        Assert.InRange(retry3!.Value, now.AddMinutes(1).AddSeconds(-1), now.AddMinutes(1).AddSeconds(1));
    }

    [Fact]
    public void Enforces_concurrency_limit()
    {
        var guard = new InMemoryQuotaGuard();
        var options = new QuotaOptions { MaxTriggersPerMinute = 10, MaxParallelExecutionsPerJob = 1 };
        var now = DateTimeOffset.UtcNow;

        Assert.True(guard.TryAcquire(Job, options, now, out var retry1));
        Assert.Null(retry1);

        Assert.False(guard.TryAcquire(Job, options, now.AddMilliseconds(10), out var retry2));
        Assert.NotNull(retry2);

        guard.Release(Job);

        Assert.True(guard.TryAcquire(Job, options, now.AddMilliseconds(20), out var retry3));
        Assert.Null(retry3);
    }
}
