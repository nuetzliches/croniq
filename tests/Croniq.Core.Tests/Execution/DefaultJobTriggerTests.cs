using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public sealed class DefaultJobTriggerTests
{
    [Fact]
    public async Task TriggerOnceAsync_WritesExecutionLogEntries()
    {
        var jobKey = JobKey.Create("ops", "smoke");
        var descriptor = new JobDescriptor(typeof(TestJob), new CroniqJobAttribute("ops", "smoke"), jobKey);
        var registry = new SingleJobRegistry(descriptor);
        var pipeline = new RecordingPipeline();
        var policyResolver = new TestPolicyResolver();
        var store = new ThrowingJobPersistenceProvider();
        var options = Microsoft.Extensions.Options.Options.Create(new Croniq.Options.CroniqOptions
        {
            TenantId = "default",
            EnvironmentTag = "dev",
            InstanceId = "test-instance"
        });
        var logStore = new RecordingExecutionLogStore();

        var trigger = new DefaultJobTrigger(
            registry,
            pipeline,
            policyResolver,
            store,
            options,
            logStore,
            NullLogger<DefaultJobTrigger>.Instance);

        await trigger.TriggerOnceAsync(jobKey.Value, cancellationToken: CancellationToken.None);

        logStore.Starts.Count.ShouldBe(1);
        logStore.Completions.Count.ShouldBe(1);
        logStore.Starts[0].ExecutionId.ShouldBe(logStore.Completions[0].ExecutionId);
        logStore.Completions[0].Status.ShouldBe(ExecutionStatus.Succeeded);
    }

    [CroniqJob("ops", "smoke")]
    private sealed class TestJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken) => Task.CompletedTask;
    }

    private sealed class SingleJobRegistry : IJobRegistry
    {
        private readonly JobDescriptor _descriptor;

        public SingleJobRegistry(JobDescriptor descriptor)
        {
            _descriptor = descriptor ?? throw new ArgumentNullException(nameof(descriptor));
            Descriptors = new[] { _descriptor };
        }

        public IReadOnlyCollection<JobDescriptor> Descriptors { get; }

        public bool TryGet(JobKey jobKey, out JobDescriptor descriptor)
        {
            if (string.Equals(jobKey.Value, _descriptor.JobKey.Value, StringComparison.OrdinalIgnoreCase))
            {
                descriptor = _descriptor;
                return true;
            }

            descriptor = null!;
            return false;
        }
    }

    private sealed class RecordingPipeline : IJobExecutionPipeline
    {
        public Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken) => Task.CompletedTask;
    }

    private sealed class RecordingExecutionLogStore : IExecutionLogStore
    {
        public List<ExecutionRecord> Starts { get; } = new();
        public List<ExecutionCompletion> Completions { get; } = new();

        public Task OnExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken)
        {
            Starts.Add(record);
            return Task.CompletedTask;
        }

        public Task AppendAsync(string executionId, IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken)
        {
            return Task.CompletedTask;
        }

        public Task OnExecutionCompletedAsync(ExecutionCompletion completion, CancellationToken cancellationToken)
        {
            Completions.Add(completion);
            return Task.CompletedTask;
        }
    }

    private sealed class TestPolicyResolver : IPolicyResolver
    {
        public MisfirePolicyOptions ResolveMisfire(JobKey jobKey, PartitionScope? scope = null) => new();

        public QuotaOptions ResolveQuota(JobKey jobKey, PartitionScope? scope = null) => new();

        public ExecutionPolicyOptions ResolveExecution(JobKey jobKey, PartitionScope? scope = null) => new();
    }

    private sealed class ThrowingJobPersistenceProvider : IJobPersistenceProvider
    {
        public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
            => throw new NotSupportedException("AcquireAsync should not be called in this test.");

        public Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken)
            => throw new NotSupportedException("TryRenewLeaseAsync should not be called in this test.");

        public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
            => throw new NotSupportedException("ReleaseAsync should not be called in this test.");

        public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken)
            => throw new NotSupportedException("MoveToDeadLetterAsync should not be called in this test.");

        public Task UpsertJobAsync(JobDefinition job, PartitionScope scope, CancellationToken cancellationToken)
            => throw new NotSupportedException("UpsertJobAsync should not be called in this test.");

        public Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken)
            => throw new NotSupportedException("ListJobsAsync should not be called in this test.");

        public Task<JobDefinition?> GetJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
            => throw new NotSupportedException("GetJobAsync should not be called in this test.");

        public Task DeleteJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
            => throw new NotSupportedException("DeleteJobAsync should not be called in this test.");

        public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
            => throw new NotSupportedException("UpsertTriggerAsync should not be called in this test.");

        public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
            => throw new NotSupportedException("ListTriggersAsync should not be called in this test.");

        public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken)
            => throw new NotSupportedException("DeleteTriggerAsync should not be called in this test.");
    }
}
