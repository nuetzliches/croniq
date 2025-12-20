using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using NSubstitute;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class TriggerWorkerTests
{
    private static readonly CroniqJobAttribute SampleJobAttribute = new("samples", "demo");
    private static readonly JobKey SampleJobKey = JobKey.Create("t1", "dev", "samples", "demo");
    private static readonly JobDescriptor SampleDescriptor = new(typeof(SampleJob), SampleJobAttribute, SampleJobKey);

    [Fact]
    public async Task Releases_unknown_job_as_deadletter()
    {
        var store = new FakeJobStore(new[]
        {
            NewLease(jobKey: "t1:dev:samples:missing")
        });

        var registry = Substitute.For<IJobRegistry>();
        registry.TryGet(Arg.Any<JobKey>(), out Arg.Any<JobDescriptor>()).Returns(false);

        var worker = CreateWorker(store, registry);

        var processed = await worker.ProcessBatchAsync(DateTimeOffset.UtcNow, batchSize: 1, CancellationToken.None);

        processed.ShouldBe(0);
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].Succeeded.ShouldBeFalse();
        store.Releases[0].DeadLetterReason.ShouldBe("job-not-registered");
    }

    [Fact]
    public async Task Deadletters_misfire_when_policy_demands()
    {
        var lease = NewLease(jobKey: SampleJobKey.Value);
        var store = new FakeJobStore(new[] { lease });

        var registry = Substitute.For<IJobRegistry>();
        registry.TryGet(SampleJobKey, out Arg.Any<JobDescriptor>()).Returns(ci =>
        {
            ci[1] = SampleDescriptor;
            return true;
        });

        var misfirePolicy = Substitute.For<IMisfirePolicy>();
        misfirePolicy.Evaluate(Arg.Any<TriggerLease>(), Arg.Any<MisfirePolicyOptions>(), Arg.Any<DateTimeOffset>())
            .Returns(new MisfireDecision(true, "late"));

        var policyResolver = Substitute.For<IPolicyResolver>();
        policyResolver.ResolveMisfire(SampleJobKey).Returns(new MisfirePolicyOptions { DeadLetterOnMisfire = true });
        policyResolver.ResolveQuota(SampleJobKey).Returns(new QuotaOptions());
        policyResolver.ResolveExecution(SampleJobKey).Returns(new ExecutionPolicyOptions());

        var quotaGuard = Substitute.For<IQuotaGuard>();
        quotaGuard.TryAcquire(Arg.Any<JobKey>(), Arg.Any<QuotaOptions>(), Arg.Any<DateTimeOffset>(), out Arg.Any<DateTimeOffset?>())
            .Returns(true);

        var pipeline = Substitute.For<IJobExecutionPipeline>();

        var worker = CreateWorker(store, registry, policyResolver, misfirePolicy, quotaGuard, pipeline);

        var processed = await worker.ProcessBatchAsync(DateTimeOffset.UtcNow, batchSize: 1, CancellationToken.None);

        await pipeline.DidNotReceiveWithAnyArgs().ExecuteAsync(default!, default);
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].DeadLetterReason.ShouldBe("late");
        processed.ShouldBe(0); // misfire prevented execution
    }

    [Fact]
    public async Task Executes_pipeline_and_releases_on_success()
    {
        var lease = NewLease(jobKey: SampleJobKey.Value);
        var store = new FakeJobStore(new[] { lease });
        var jobLogStore = Substitute.For<IExecutionLogStore>();

        var registry = Substitute.For<IJobRegistry>();
        registry.TryGet(SampleJobKey, out Arg.Any<JobDescriptor>()).Returns(ci =>
        {
            ci[1] = SampleDescriptor;
            return true;
        });

        var misfirePolicy = Substitute.For<IMisfirePolicy>();
        misfirePolicy.Evaluate(lease, Arg.Any<MisfirePolicyOptions>(), Arg.Any<DateTimeOffset>())
            .Returns(new MisfireDecision(false, null));

        var policyResolver = Substitute.For<IPolicyResolver>();
        policyResolver.ResolveMisfire(SampleJobKey).Returns(new MisfirePolicyOptions());
        policyResolver.ResolveQuota(SampleJobKey).Returns(new QuotaOptions());
        policyResolver.ResolveExecution(SampleJobKey).Returns(new ExecutionPolicyOptions());

        var quotaGuard = Substitute.For<IQuotaGuard>();
        quotaGuard.TryAcquire(Arg.Any<JobKey>(), Arg.Any<QuotaOptions>(), Arg.Any<DateTimeOffset>(), out Arg.Any<DateTimeOffset?>())
            .Returns(true);

        var pipeline = Substitute.For<IJobExecutionPipeline>();
        JobExecutionRequest? capturedRequest = null;
        pipeline.ExecuteAsync(Arg.Do<JobExecutionRequest>(r => capturedRequest = r), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);

        var worker = CreateWorker(store, registry, policyResolver, misfirePolicy, quotaGuard, pipeline, jobLogStore);

        var processed = await worker.ProcessBatchAsync(DateTimeOffset.UtcNow, batchSize: 1, CancellationToken.None);

        processed.ShouldBe(1);
        await pipeline.Received(1).ExecuteAsync(Arg.Any<JobExecutionRequest>(), Arg.Any<CancellationToken>());
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].Succeeded.ShouldBeTrue();
        capturedRequest.ShouldNotBeNull();
        capturedRequest!.ExecutionId.ShouldNotBeNullOrWhiteSpace();
        capturedRequest.Metadata.ShouldContainKeyAndValue("trigger_id", lease.TriggerId);
        await jobLogStore.Received(1).OnExecutionStartedAsync(Arg.Any<ExecutionRecord>(), Arg.Any<CancellationToken>());
        await jobLogStore.Received(1).OnExecutionCompletedAsync(Arg.Is<ExecutionCompletion>(c => c.Status == ExecutionStatus.Succeeded), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Applies_trigger_metadata_from_payload()
    {
        var payloadMetadata = new Dictionary<string, string>
        {
            { "initiator", "seed" },
            { "custom", "value" },
            { "trigger_id", "override" }
        };
        var payload = JsonSerializer.Serialize(payloadMetadata, new JsonSerializerOptions(JsonSerializerDefaults.Web));
        var lease = NewLease(jobKey: SampleJobKey.Value, payload: payload);
        var store = new FakeJobStore(new[] { lease });

        var registry = Substitute.For<IJobRegistry>();
        registry.TryGet(SampleJobKey, out Arg.Any<JobDescriptor>()).Returns(ci =>
        {
            ci[1] = SampleDescriptor;
            return true;
        });

        var pipeline = Substitute.For<IJobExecutionPipeline>();
        JobExecutionRequest? capturedRequest = null;
        pipeline.ExecuteAsync(Arg.Do<JobExecutionRequest>(r => capturedRequest = r), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);

        var worker = CreateWorker(store, registry, pipeline: pipeline);

        var processed = await worker.ProcessBatchAsync(DateTimeOffset.UtcNow, batchSize: 1, CancellationToken.None);

        processed.ShouldBe(1);
        capturedRequest.ShouldNotBeNull();
        capturedRequest!.Metadata["trigger_id"].ShouldBe(lease.TriggerId);
        capturedRequest.Metadata["initiator"].ShouldBe("seed");
        capturedRequest.Metadata["custom"].ShouldBe("value");
        capturedRequest.Metadata.ContainsKey("payload").ShouldBeFalse();
    }

    [Fact]
    public async Task Persists_completion_on_failure()
    {
        var lease = NewLease(jobKey: SampleJobKey.Value);
        var store = new FakeJobStore(new[] { lease });
        var jobLogStore = Substitute.For<IExecutionLogStore>();

        var registry = Substitute.For<IJobRegistry>();
        registry.TryGet(SampleJobKey, out Arg.Any<JobDescriptor>()).Returns(ci =>
        {
            ci[1] = SampleDescriptor;
            return true;
        });

        var misfirePolicy = Substitute.For<IMisfirePolicy>();
        misfirePolicy.Evaluate(lease, Arg.Any<MisfirePolicyOptions>(), Arg.Any<DateTimeOffset>())
            .Returns(new MisfireDecision(false, null));

        var policyResolver = Substitute.For<IPolicyResolver>();
        policyResolver.ResolveMisfire(SampleJobKey).Returns(new MisfirePolicyOptions());
        policyResolver.ResolveQuota(SampleJobKey).Returns(new QuotaOptions());
        policyResolver.ResolveExecution(SampleJobKey).Returns(new ExecutionPolicyOptions());

        var quotaGuard = Substitute.For<IQuotaGuard>();
        quotaGuard.TryAcquire(Arg.Any<JobKey>(), Arg.Any<QuotaOptions>(), Arg.Any<DateTimeOffset>(), out Arg.Any<DateTimeOffset?>())
            .Returns(true);

        var pipeline = Substitute.For<IJobExecutionPipeline>();
        pipeline.ExecuteAsync(Arg.Any<JobExecutionRequest>(), Arg.Any<CancellationToken>())
            .Returns(_ => Task.FromException(new InvalidOperationException("boom")));

        var worker = CreateWorker(store, registry, policyResolver, misfirePolicy, quotaGuard, pipeline, jobLogStore);

        var processed = await worker.ProcessBatchAsync(DateTimeOffset.UtcNow, batchSize: 1, CancellationToken.None);

        processed.ShouldBe(0);
        await jobLogStore.Received(1).OnExecutionStartedAsync(Arg.Any<ExecutionRecord>(), Arg.Any<CancellationToken>());
        await jobLogStore.Received(1).OnExecutionCompletedAsync(Arg.Is<ExecutionCompletion>(c => c.Status == ExecutionStatus.Failed && c.ErrorMessage == "boom"), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Log_store_failures_are_swallowed()
    {
        var lease = NewLease(jobKey: SampleJobKey.Value);
        var store = new FakeJobStore(new[] { lease });
        var jobLogStore = Substitute.For<IExecutionLogStore>();
        jobLogStore.OnExecutionStartedAsync(Arg.Any<ExecutionRecord>(), Arg.Any<CancellationToken>())
            .Returns(_ => Task.FromException(new InvalidOperationException("start-fail")));
        jobLogStore.OnExecutionCompletedAsync(Arg.Any<ExecutionCompletion>(), Arg.Any<CancellationToken>())
            .Returns(_ => Task.FromException(new InvalidOperationException("complete-fail")));

        var registry = Substitute.For<IJobRegistry>();
        registry.TryGet(SampleJobKey, out Arg.Any<JobDescriptor>()).Returns(ci =>
        {
            ci[1] = SampleDescriptor;
            return true;
        });

        var misfirePolicy = Substitute.For<IMisfirePolicy>();
        misfirePolicy.Evaluate(lease, Arg.Any<MisfirePolicyOptions>(), Arg.Any<DateTimeOffset>())
            .Returns(new MisfireDecision(false, null));

        var policyResolver = Substitute.For<IPolicyResolver>();
        policyResolver.ResolveMisfire(SampleJobKey).Returns(new MisfirePolicyOptions());
        policyResolver.ResolveQuota(SampleJobKey).Returns(new QuotaOptions());
        policyResolver.ResolveExecution(SampleJobKey).Returns(new ExecutionPolicyOptions());

        var quotaGuard = Substitute.For<IQuotaGuard>();
        quotaGuard.TryAcquire(Arg.Any<JobKey>(), Arg.Any<QuotaOptions>(), Arg.Any<DateTimeOffset>(), out Arg.Any<DateTimeOffset?>())
            .Returns(true);

        var pipeline = Substitute.For<IJobExecutionPipeline>();
        pipeline.ExecuteAsync(Arg.Any<JobExecutionRequest>(), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);

        var worker = CreateWorker(store, registry, policyResolver, misfirePolicy, quotaGuard, pipeline, jobLogStore);

        var processed = await worker.ProcessBatchAsync(DateTimeOffset.UtcNow, batchSize: 1, CancellationToken.None);

        processed.ShouldBe(1);
        store.Releases.ShouldHaveSingleItem();
        store.Releases[0].Succeeded.ShouldBeTrue();
    }

    private static TriggerLease NewLease(string jobKey, string? payload = null)
    {
        return new TriggerLease(
            LeaseId: Guid.NewGuid().ToString("N"),
            TriggerId: "trigger-1",
            JobKey: jobKey,
            Scope: new PartitionScope("t1", "dev"),
            FireAtUtc: DateTimeOffset.UtcNow,
            LeaseExpiresAtUtc: DateTimeOffset.UtcNow.AddMinutes(5),
            Payload: payload);
    }

    private static TriggerWorker CreateWorker(
        IJobStore store,
        IJobRegistry registry,
        IPolicyResolver? policyResolver = null,
        IMisfirePolicy? misfirePolicy = null,
        IQuotaGuard? quotaGuard = null,
        IJobExecutionPipeline? pipeline = null,
        IExecutionLogStore? jobLogStore = null)
    {
        if (policyResolver is null)
        {
            policyResolver = Substitute.For<IPolicyResolver>();
            policyResolver.ResolveMisfire(Arg.Any<JobKey>()).Returns(new MisfirePolicyOptions());
            policyResolver.ResolveQuota(Arg.Any<JobKey>()).Returns(new QuotaOptions());
            policyResolver.ResolveExecution(Arg.Any<JobKey>()).Returns(new ExecutionPolicyOptions());
        }

        if (misfirePolicy is null)
        {
            misfirePolicy = Substitute.For<IMisfirePolicy>();
            misfirePolicy.Evaluate(Arg.Any<TriggerLease>(), Arg.Any<MisfirePolicyOptions>(), Arg.Any<DateTimeOffset>())
                .Returns(new MisfireDecision(false, null));
        }

        if (quotaGuard is null)
        {
            quotaGuard = Substitute.For<IQuotaGuard>();
            quotaGuard.TryAcquire(Arg.Any<JobKey>(), Arg.Any<QuotaOptions>(), Arg.Any<DateTimeOffset>(), out Arg.Any<DateTimeOffset?>())
                .Returns(true);
        }

        if (pipeline is null)
        {
            pipeline = Substitute.For<IJobExecutionPipeline>();
            pipeline.ExecuteAsync(Arg.Any<JobExecutionRequest>(), Arg.Any<CancellationToken>())
                .Returns(Task.CompletedTask);
        }

        jobLogStore ??= Substitute.For<IExecutionLogStore>();

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t1", EnvironmentTag = "dev", InstanceId = "test" });

        return new TriggerWorker(
            store,
            registry,
            pipeline,
            misfirePolicy,
            policyResolver,
            options,
            quotaGuard,
            jobLogStore,
            NullLogger<TriggerWorker>.Instance,
            new ActivitySource("Croniq.Core.Tests.TriggerWorker"));
    }

    [CroniqJob("samples", "demo")]
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
        {
            return Task.CompletedTask;
        }
    }

    private sealed class FakeJobStore : IJobStore
    {
        private readonly IReadOnlyCollection<TriggerLease> _leases;

        public FakeJobStore(IReadOnlyCollection<TriggerLease> leases)
        {
            _leases = leases;
        }

        public List<TriggerReleaseRequest> Releases { get; } = new();
        public List<DeadLetterRequest> DeadLetters { get; } = new();

        public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
        {
            return Task.FromResult(_leases);
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
}
