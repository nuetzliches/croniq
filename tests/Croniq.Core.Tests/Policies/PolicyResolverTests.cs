using System;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Policies;

public class PolicyResolverTests
{
    [Fact]
    public void Picks_most_specific_misfire_override()
    {
        var resolver = new PolicyResolver(
            Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5), DeadLetterOnMisfire = false }),
            Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions()),
            Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions
            {
                Misfire =
                {
                    new MisfirePolicyOverride
                    {
                        TenantId = "t1",
                        Options = new MisfirePolicyOptions
                        {
                            MaxMisfireDelay = TimeSpan.FromMinutes(2),
                            DeadLetterOnMisfire = false,
                            RescheduleBackoff = TimeSpan.FromSeconds(10)
                        }
                    },
                    new MisfirePolicyOverride
                    {
                        TenantId = "t1",
                        EnvironmentTag = "dev",
                        NamespaceSegment = "billing",
                        Options = new MisfirePolicyOptions
                        {
                            MaxMisfireDelay = TimeSpan.FromMinutes(1),
                            DeadLetterOnMisfire = true,
                            RescheduleBackoff = TimeSpan.FromSeconds(5)
                        }
                    }
                }
            }));

        var scope = new PartitionScope("t1", "dev");
        var jobKey = new JobKey("billing", "invoice");
        var resolved = resolver.ResolveMisfire(jobKey, scope);

        resolved.MaxMisfireDelay.ShouldBe(TimeSpan.FromMinutes(1));
        resolved.DeadLetterOnMisfire.ShouldBeTrue();
        resolved.RescheduleBackoff.ShouldBe(TimeSpan.FromSeconds(5));
    }

    [Fact]
    public void Applies_most_restrictive_quota_from_overrides()
    {
        var resolver = new PolicyResolver(
            Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions()),
            Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions()),
            Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions
            {
                Quotas =
                {
                    new QuotaOverride
                    {
                        TenantId = "t1",
                        Options = new QuotaOptions { MaxTriggersPerMinute = 80, MaxParallelExecutionsPerJob = 4 }
                    },
                    new QuotaOverride
                    {
                        TenantId = "t1",
                        EnvironmentTag = "dev",
                        NamespaceSegment = "billing",
                        Options = new QuotaOptions { MaxTriggersPerMinute = 50, MaxParallelExecutionsPerJob = 3 }
                    }
                }
            }));

        var scope = new PartitionScope("t1", "dev");
        var jobKey = new JobKey("billing", "invoice");
        var resolved = resolver.ResolveQuota(jobKey, scope);

        resolved.MaxTriggersPerMinute.ShouldBe(50);
        resolved.MaxParallelExecutionsPerJob.ShouldBe(3);
    }

    [Fact]
    public void Resolves_execution_override()
    {
        var resolver = new PolicyResolver(
            Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions()),
            Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions
            {
                Retry = new RetryPolicyOptions
                {
                    Enabled = true,
                    MaxAttempts = 3,
                    BackoffStrategy = RetryBackoffStrategy.Linear,
                    InitialDelay = TimeSpan.FromSeconds(2),
                    MaxDelay = TimeSpan.FromSeconds(10),
                    JitterFactor = 0.1d
                },
                Timeout = new TimeoutPolicyOptions
                {
                    Enabled = true,
                    Timeout = TimeSpan.FromMinutes(2),
                    CancelExecutionOnTimeout = true
                },
                CircuitBreaker = new CircuitBreakerPolicyOptions
                {
                    Enabled = false,
                    FailureThreshold = 5,
                    SamplingWindow = TimeSpan.FromMinutes(1),
                    BreakDuration = TimeSpan.FromMinutes(1),
                    MinimumThroughput = 10
                },
                DeadLetter = new DeadLetterPolicyOptions
                {
                    Enabled = true,
                    Retention = TimeSpan.FromDays(14),
                    OperatorHint = "default"
                }
            }),
            Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions
            {
                Execution =
                {
                    new ExecutionPolicyOverride
                    {
                        TenantId = "t1",
                        EnvironmentTag = "prod",
                        NamespaceSegment = "billing",
                        JobName = "invoice",
                        Options = new ExecutionPolicyOptions
                        {
                            Retry = new RetryPolicyOptions
                            {
                                Enabled = true,
                                MaxAttempts = 6,
                                BackoffStrategy = RetryBackoffStrategy.Exponential,
                                InitialDelay = TimeSpan.FromSeconds(1),
                                MaxDelay = TimeSpan.FromSeconds(30),
                                JitterFactor = 0.2d
                            },
                            Timeout = new TimeoutPolicyOptions
                            {
                                Enabled = true,
                                Timeout = TimeSpan.FromSeconds(45),
                                CancelExecutionOnTimeout = false
                            },
                            CircuitBreaker = new CircuitBreakerPolicyOptions
                            {
                                Enabled = true,
                                FailureThreshold = 10,
                                SamplingWindow = TimeSpan.FromMinutes(2),
                                BreakDuration = TimeSpan.FromMinutes(5),
                                MinimumThroughput = 4
                            },
                            DeadLetter = new DeadLetterPolicyOptions
                            {
                                Enabled = true,
                                Retention = TimeSpan.FromDays(7),
                                OperatorHint = "investigate"
                            }
                        }
                    }
                }
            }));

        var scope = new PartitionScope("t1", "prod");
        var jobKey = new JobKey("billing", "invoice");
        var resolved = resolver.ResolveExecution(jobKey, scope);

        resolved.Retry.MaxAttempts.ShouldBe(6);
        resolved.Retry.BackoffStrategy.ShouldBe(RetryBackoffStrategy.Exponential);
        resolved.Retry.InitialDelay.ShouldBe(TimeSpan.FromSeconds(1));
        resolved.Retry.MaxDelay.ShouldBe(TimeSpan.FromSeconds(30));
        resolved.Retry.JitterFactor.ShouldBe(0.2d);
        resolved.Timeout.Timeout.ShouldBe(TimeSpan.FromSeconds(45));
        resolved.Timeout.CancelExecutionOnTimeout.ShouldBeFalse();
        resolved.CircuitBreaker.Enabled.ShouldBeTrue();
        resolved.CircuitBreaker.BreakDuration.ShouldBe(TimeSpan.FromMinutes(5));
        resolved.CircuitBreaker.MinimumThroughput.ShouldBe(4);
        resolved.DeadLetter.Retention.ShouldBe(TimeSpan.FromDays(7));
        resolved.DeadLetter.OperatorHint.ShouldBe("investigate");
    }
}
