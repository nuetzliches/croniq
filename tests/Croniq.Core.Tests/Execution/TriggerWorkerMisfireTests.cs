using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
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

    private sealed class StubJobStore : IJobStore
    {
        private readonly IReadOnlyCollection<TriggerLease> _leases;
        public List<TriggerReleaseRequest> Releases { get; } = new();

        public StubJobStore(IReadOnlyCollection<TriggerLease> leases)
        {
            _leases = leases;
        }

        public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
        {
            return Task.FromResult(_leases);
        }

        public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
        {
            Releases.Add(request);
            return Task.CompletedTask;
        }
    }

    private static IJobRegistry BuildRegistry()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t", EnvironmentTag = "dev" });
        var registrations = new[] { new JobRegistration(typeof(SampleJob)) };
        return new JobRegistry(options, registrations);
    }

    [Fact]
    public async Task Skips_misfire_and_deadletters()
    {
        var now = DateTimeOffset.UtcNow;
        var lease = new TriggerLease("l1", "tr1", "t:dev:test:job", new PartitionScope("t", "dev"), now.AddMinutes(-10), now, null);
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
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            new InMemoryQuotaGuard(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 10, CancellationToken.None);

        Assert.Equal(0, processed);
        Assert.Empty(store.Releases.FindAll(r => r.Succeeded));
        Assert.Single(store.Releases);
        Assert.Equal("misfire-max-delay", store.Releases[0].DeadLetterReason);
        Assert.Equal(0, pipeline.Executions);
    }

    [Fact]
    public async Task Executes_when_not_misfired()
    {
        var now = DateTimeOffset.UtcNow;
        var lease = new TriggerLease("l1", "tr1", "t:dev:test:job", new PartitionScope("t", "dev"), now.AddSeconds(-10), now.AddMinutes(1), null);
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
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            new InMemoryQuotaGuard(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 10, CancellationToken.None);

        Assert.Equal(1, processed);
        Assert.Single(store.Releases);
        Assert.True(store.Releases[0].Succeeded);
        Assert.Null(store.Releases[0].DeadLetterReason);
        Assert.Equal(1, pipeline.Executions);
    }

    [Fact]
    public async Task Applies_quota_limit_and_reschedules()
    {
        var now = DateTimeOffset.UtcNow;
        var lease1 = new TriggerLease("l1", "tr1", "t:dev:test:job", new PartitionScope("t", "dev"), now.AddSeconds(-5), now, null);
        var lease2 = new TriggerLease("l2", "tr2", "t:dev:test:job", new PartitionScope("t", "dev"), now.AddSeconds(-5), now, null);
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
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t", EnvironmentTag = "dev", InstanceId = "i1" }),
            new InMemoryQuotaGuard(),
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("test"));

        var processed = await worker.ProcessBatchAsync(now, 10, CancellationToken.None);

        Assert.Equal(1, processed);
        Assert.Equal(2, store.Releases.Count);
        Assert.Equal(1, pipeline.Executions);

        var rescheduled = store.Releases.Find(r => r.DeadLetterReason == "quota-limit");
        Assert.NotNull(rescheduled);
        Assert.False(rescheduled!.Succeeded);
        Assert.NotNull(rescheduled.NextFireTimeUtc);
        Assert.InRange(rescheduled.NextFireTimeUtc!.Value, now.AddMinutes(1).AddSeconds(-1), now.AddMinutes(1).AddSeconds(2));
    }
}
