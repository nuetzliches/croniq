using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Shouldly;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class TriggerWorkerMisfireTests
{
    [CroniqJob("test", "job")]
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken) => Task.CompletedTask;
    }

    private sealed class StubPipeline : IJobExecutionPipeline
    {
        public int Executions { get; private set; }

        public Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken)
        {
            Executions++;
            return Task.CompletedTask;
        }
    }

    private sealed class FailingPipeline : IJobExecutionPipeline
    {
        private readonly Exception _exception;

        public FailingPipeline(Exception exception)
        {
            _exception = exception;
        }

        public Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken)
            => Task.FromException(_exception);
    }

    private sealed class StubJobStore : IJobStore
    {
        private readonly IReadOnlyCollection<TriggerLease> _leases;
        public List<TriggerReleaseRequest> Releases { get; } = new();
        public List<DeadLetterRequest> DeadLetters { get; } = new();

        public StubJobStore(IReadOnlyCollection<TriggerLease> leases)
        {
            _leases = leases;
        }

        public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
        {
            return Task.FromResult(_leases);
        }

        public Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken)
        {
            return Task.FromResult<TriggerLease?>(null);
        }

        public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
        {
            Releases.Add(request);
            return Task.CompletedTask;
        }

        public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken)
        {
            DeadLetters.Add(request);
            return Task.CompletedTask;
        }
    }

    private static IJobRegistry BuildRegistry()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantReference = "t", EnvironmentTag = "dev" });
        var registrations = new[] { new JobRegistration(typeof(SampleJob)) };
        return new JobRegistry(options, registrations);
    }

    [Fact]
    public async Task Skips_misfire_and_deadletters()
    {
        var now = DateTimeOffset.UtcNow;
        var lease = new TriggerLease("l1", "tr1", "test:job", new PartitionScope("t", "dev"), now.AddMinutes(-10), now, null);
        var store = new StubJobStore(new[] { lease });
        var pipeline = new StubPipeline();
        var worker = new TriggerWorker(
            store,
            BuildRegistry(),
            pipeline,
            new DefaultMisfirePolicy(Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) })),
            new PolicyResolver(
                Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5), DeadLetterOnMisfire = true }),
                Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions()),
                Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions())),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantReference = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions { LeaseRenewalLeadTime = TimeSpan.Zero }),
            new InMemoryQuotaGuard(),
            new NoOpExecutionLogStore(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 10, CancellationToken.None);

        processed.ShouldBe(0);
        store.Releases.FindAll(r => r.Succeeded).ShouldBeEmpty();
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].DeadLetterReason.ShouldBe("misfire-max-delay");
        pipeline.Executions.ShouldBe(0);
    }

    [Fact]
    public async Task Executes_when_not_misfired()
    {
        var now = DateTimeOffset.UtcNow;
        var lease = new TriggerLease("l1", "tr1", "test:job", new PartitionScope("t", "dev"), now.AddSeconds(-10), now.AddMinutes(1), null);
        var store = new StubJobStore(new[] { lease });
        var pipeline = new StubPipeline();
        var worker = new TriggerWorker(
            store,
            BuildRegistry(),
            pipeline,
            new DefaultMisfirePolicy(Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) })),
            new PolicyResolver(
                Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) }),
                Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions()),
                Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions())),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantReference = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions { LeaseRenewalLeadTime = TimeSpan.Zero }),
            new InMemoryQuotaGuard(),
            new NoOpExecutionLogStore(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 10, CancellationToken.None);

        processed.ShouldBe(1);
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].Succeeded.ShouldBeTrue();
        store.Releases[0].DeadLetterReason.ShouldBeNull();
        pipeline.Executions.ShouldBe(1);
    }

    [Fact]
    public async Task Applies_quota_limit_and_reschedules()
    {
        var now = DateTimeOffset.UtcNow;
        var lease1 = new TriggerLease("l1", "tr1", "test:job", new PartitionScope("t", "dev"), now.AddSeconds(-5), now, null);
        var lease2 = new TriggerLease("l2", "tr2", "test:job", new PartitionScope("t", "dev"), now.AddSeconds(-5), now, null);
        var store = new StubJobStore(new[] { lease1, lease2 });
        var pipeline = new StubPipeline();

        var quotaOverrides = new PolicyOverrideOptions
        {
            Quotas =
            {
                new QuotaOverride
                {
                    TenantId = "t",
                    EnvironmentTag = "dev",
                    NamespaceSegment = "test",
                    JobName = "job",
                    Options = new QuotaOptions { MaxTriggersPerMinute = 1, MaxParallelExecutionsPerJob = 1 }
                }
            }
        };

        var worker = new TriggerWorker(
            store,
            BuildRegistry(),
            pipeline,
            new DefaultMisfirePolicy(Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) })),
            new PolicyResolver(
                Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) }),
                Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions()),
                Microsoft.Extensions.Options.Options.Create(quotaOverrides)),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantReference = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions { LeaseRenewalLeadTime = TimeSpan.Zero }),
            new InMemoryQuotaGuard(),
            new NoOpExecutionLogStore(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 10, CancellationToken.None);

        processed.ShouldBe(1);
        store.Releases.Count.ShouldBe(2);
        pipeline.Executions.ShouldBe(1);

        var rescheduled = store.Releases.Find(r => r.DeadLetterReason == "quota-limit");
        rescheduled.ShouldNotBeNull();
        rescheduled!.Succeeded.ShouldBeFalse();
        rescheduled.NextFireTimeUtc.ShouldNotBeNull();
        rescheduled.NextFireTimeUtc!.Value.ShouldBeInRange(
            now.AddMinutes(1).AddSeconds(-1),
            now.AddMinutes(1).AddSeconds(2));
    }

    [Fact]
    public async Task Deadletters_when_execution_pipeline_fails()
    {
        var now = DateTimeOffset.UtcNow;
        var lease = new TriggerLease("l-fail", "tr-fail", "test:job", new PartitionScope("t", "dev"), now.AddSeconds(-5), now, "{\"input\":true}");
        var store = new StubJobStore(new[] { lease });
        var pipeline = new FailingPipeline(new InvalidOperationException("boom"));

        var overrides = new PolicyOverrideOptions
        {
            Execution =
            {
                new ExecutionPolicyOverride
                {
                    TenantId = "t",
                    EnvironmentTag = "dev",
                    NamespaceSegment = "test",
                    JobName = "job",
                    Options = new ExecutionPolicyOptions
                    {
                        DeadLetter = new DeadLetterPolicyOptions
                        {
                            Enabled = true,
                            Retention = TimeSpan.FromDays(7),
                            OperatorHint = "check job payload"
                        }
                    }
                }
            }
        };

        var worker = new TriggerWorker(
            store,
            BuildRegistry(),
            pipeline,
            new DefaultMisfirePolicy(Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) })),
            new PolicyResolver(
                Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5) }),
                Microsoft.Extensions.Options.Options.Create(new ExecutionPolicyOptions { DeadLetter = new DeadLetterPolicyOptions { Enabled = true } }),
                Microsoft.Extensions.Options.Options.Create(overrides)),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantReference = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions { LeaseRenewalLeadTime = TimeSpan.Zero }),
            new InMemoryQuotaGuard(),
            new NoOpExecutionLogStore(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 1, CancellationToken.None);

        processed.ShouldBe(0);
        store.DeadLetters.ShouldHaveSingleItem();
        var deadLetter = store.DeadLetters[0];
        deadLetter.Reason.ShouldBe("execution-error");
        deadLetter.Payload.ShouldNotBeNull();
        deadLetter.Metadata.ShouldNotBeNull();
        deadLetter.Metadata!.ContainsKey("deadletter.hint").ShouldBeTrue();
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].DeadLetterReason.ShouldBeNull();
    }
}
