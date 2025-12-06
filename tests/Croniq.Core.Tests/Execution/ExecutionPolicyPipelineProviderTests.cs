using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Microsoft.Extensions.Logging.Abstractions;
using Polly.Timeout;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class ExecutionPolicyPipelineProviderTests
{
    private static readonly JobKey SampleJob = JobKey.Create("tenant", "dev", "ns", "job");

    [Fact]
    public async Task Retries_until_attempt_limit_is_hit()
    {
        var provider = new ExecutionPolicyPipelineProvider(NullLogger<ExecutionPolicyPipelineProvider>.Instance);
        var options = new ExecutionPolicyOptions
        {
            Timeout = { Enabled = false },
            Retry =
            {
                Enabled = true,
                MaxAttempts = 3,
                InitialDelay = TimeSpan.Zero,
                MaxDelay = TimeSpan.Zero,
                BackoffStrategy = RetryBackoffStrategy.Fixed
            }
        };

        var pipeline = provider.Get(SampleJob, options);
        var attempts = 0;

        await Assert.ThrowsAsync<InvalidOperationException>(async () =>
        {
            await pipeline.ExecuteAsync(token =>
            {
                attempts++;
                return ValueTask.FromException(new InvalidOperationException("boom"));
            }, CancellationToken.None);
        });

        Assert.Equal(3, attempts);
    }

    [Fact]
    public void Reuses_cached_pipeline_when_options_unchanged()
    {
        var provider = new ExecutionPolicyPipelineProvider(NullLogger<ExecutionPolicyPipelineProvider>.Instance);
        var baseline = new ExecutionPolicyOptions { Timeout = { Enabled = false }, Retry = { Enabled = false } };

        var first = provider.Get(SampleJob, baseline);
        var second = provider.Get(SampleJob, baseline);

        Assert.Same(first, second);

        var mutated = new ExecutionPolicyOptions
        {
            Timeout = { Enabled = false },
            Retry = { Enabled = true, MaxAttempts = 2, InitialDelay = TimeSpan.Zero, MaxDelay = TimeSpan.Zero }
        };

        var third = provider.Get(SampleJob, mutated);
        Assert.NotSame(first, third);
    }

    [Fact]
    public async Task Applies_timeout_strategy()
    {
        var provider = new ExecutionPolicyPipelineProvider(NullLogger<ExecutionPolicyPipelineProvider>.Instance);
        var options = new ExecutionPolicyOptions
        {
            Retry = { Enabled = false },
            Timeout =
            {
                Enabled = true,
                Timeout = TimeSpan.FromMilliseconds(50)
            }
        };

        var pipeline = provider.Get(SampleJob, options);

        await Assert.ThrowsAsync<TimeoutRejectedException>(async () =>
        {
            await pipeline.ExecuteAsync(async token =>
            {
                await Task.Delay(TimeSpan.FromSeconds(1), token);
            }, CancellationToken.None);
        });
    }
}
