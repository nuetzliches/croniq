using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Shouldly;
using Microsoft.Extensions.Logging.Abstractions;
using Polly.Timeout;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class ExecutionPolicyPipelineProviderTests
{
    private static readonly JobKey SampleJob = JobKey.Create("ns", "job");

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

        var act = async () =>
        {
            await pipeline.ExecuteAsync(token =>
            {
                attempts++;
                return ValueTask.FromException(new InvalidOperationException("boom"));
            }, CancellationToken.None);
        };

        await Should.ThrowAsync<InvalidOperationException>(act);
        attempts.ShouldBe(3);
    }

    [Fact]
    public void Reuses_cached_pipeline_when_options_unchanged()
    {
        var provider = new ExecutionPolicyPipelineProvider(NullLogger<ExecutionPolicyPipelineProvider>.Instance);
        var baseline = new ExecutionPolicyOptions { Timeout = { Enabled = false }, Retry = { Enabled = false } };

        var first = provider.Get(SampleJob, baseline);
        var second = provider.Get(SampleJob, baseline);

        first.ShouldBeSameAs(second);

        var mutated = new ExecutionPolicyOptions
        {
            Timeout = { Enabled = false },
            Retry = { Enabled = true, MaxAttempts = 2, InitialDelay = TimeSpan.Zero, MaxDelay = TimeSpan.Zero }
        };

        var third = provider.Get(SampleJob, mutated);
        first.ShouldNotBeSameAs(third);
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

        var timeoutAct = async () =>
        {
            await pipeline.ExecuteAsync(async token =>
            {
                await Task.Delay(TimeSpan.FromSeconds(1), token);
            }, CancellationToken.None);
        };

        await Should.ThrowAsync<TimeoutRejectedException>(timeoutAct);
    }

    [Fact]
    public async Task Retries_only_retryable_exceptions()
    {
        var provider = new ExecutionPolicyPipelineProvider(NullLogger<ExecutionPolicyPipelineProvider>.Instance);
        var options = new ExecutionPolicyOptions
        {
            Timeout = { Enabled = false },
            Retry =
            {
                Enabled = true,
                MaxAttempts = 2,
                InitialDelay = TimeSpan.Zero,
                MaxDelay = TimeSpan.Zero,
                BackoffStrategy = RetryBackoffStrategy.Fixed,
                RetryableExceptions = new[] { typeof(InvalidOperationException).FullName! }
            }
        };

        var pipeline = provider.Get(SampleJob, options);
        var attempts = 0;

        await Should.ThrowAsync<InvalidOperationException>(async () =>
        {
            await pipeline.ExecuteAsync(_ =>
            {
                attempts++;
                return ValueTask.FromException(new InvalidOperationException("retryable"));
            }, CancellationToken.None);
        });
        attempts.ShouldBe(2); // first try + one retry

        attempts = 0;
        await Should.ThrowAsync<ArgumentException>(async () =>
        {
            await pipeline.ExecuteAsync(_ =>
            {
                attempts++;
                return ValueTask.FromException(new ArgumentException("non-retryable"));
            }, CancellationToken.None);
        });
        attempts.ShouldBe(1); // no retries because not in allow-list
    }

    [Fact]
    public async Task Opens_circuit_after_failures()
    {
        var provider = new ExecutionPolicyPipelineProvider(NullLogger<ExecutionPolicyPipelineProvider>.Instance);
        var options = new ExecutionPolicyOptions
        {
            Timeout = { Enabled = false },
            Retry = { Enabled = false },
            CircuitBreaker =
            {
                Enabled = true,
                FailureThreshold = 50, // 50%
                SamplingWindow = TimeSpan.FromSeconds(5),
                BreakDuration = TimeSpan.FromSeconds(30),
                MinimumThroughput = 2
            }
        };

        var pipeline = provider.Get(SampleJob, options);

        // two failures should open the circuit (failure ratio 100% > 50% with throughput 2)
        await Should.ThrowAsync<InvalidOperationException>(async () =>
        {
            await pipeline.ExecuteAsync(_ => ValueTask.FromException(new InvalidOperationException("boom1")), CancellationToken.None);
        });
        await Should.ThrowAsync<InvalidOperationException>(async () =>
        {
            await pipeline.ExecuteAsync(_ => ValueTask.FromException(new InvalidOperationException("boom2")), CancellationToken.None);
        });

        await Should.ThrowAsync<Polly.CircuitBreaker.BrokenCircuitException>(async () =>
        {
            await pipeline.ExecuteAsync(_ => ValueTask.FromResult(true), CancellationToken.None);
        });
    }
}
