using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;

namespace Croniq.Api.Tests.Infrastructure;

public sealed class RecordingJobExecutionPipeline : IJobExecutionPipeline
{
    private readonly List<JobExecutionRequest> _executions = new();

    public IReadOnlyList<JobExecutionRequest> Executions => _executions;

    public Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken)
    {
        _executions.Add(request);
        return Task.CompletedTask;
    }

    public void Clear() => _executions.Clear();
}

public sealed class FakeJobRegistry : IJobRegistry
{
    private readonly Dictionary<JobKey, JobDescriptor> _entries = new();

    public IReadOnlyCollection<JobDescriptor> Descriptors => _entries.Values.ToList();

    public bool TryGet(JobKey jobKey, out JobDescriptor descriptor)
    {
        return _entries.TryGetValue(jobKey, out descriptor!);
    }

    public JobDescriptor EnsureJob(string jobKey)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new ArgumentException("Invalid JobKey", nameof(jobKey));
        }

        return EnsureJob(parsed);
    }

    public JobDescriptor EnsureJob(JobKey jobKey)
    {
        if (_entries.TryGetValue(jobKey, out var descriptor))
        {
            return descriptor;
        }

        var attribute = new CroniqJobAttribute(jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant);
        descriptor = new JobDescriptor(typeof(FakeWebhookJob), attribute, jobKey);
        _entries[jobKey] = descriptor;
        return descriptor;
    }

    public void Clear() => _entries.Clear();

    [CroniqJob("test", "webhook")]
    private sealed class FakeWebhookJob
    {
    }
}

public sealed class FakePolicyResolver : IPolicyResolver
{
    public ExecutionPolicyOptions ResolveExecution(JobKey jobKey) => new();

    public MisfirePolicyOptions ResolveMisfire(JobKey jobKey) => new();

    public QuotaOptions ResolveQuota(JobKey jobKey) => new();

    public void Reset()
    {
        // nothing to reset for now
    }
}

public sealed class NoopJobPersistenceProvider : IJobPersistenceProvider, IPersistenceHealth
{
    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        return Task.FromResult<IReadOnlyCollection<TriggerLease>>(Array.Empty<TriggerLease>());
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        return Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(Array.Empty<TriggerDefinition>());
    }

    public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task<PersistenceHealthResult> CheckAsync(CancellationToken cancellationToken = default)
    {
        return Task.FromResult(new PersistenceHealthResult(true, "noop"));
    }
}

public sealed class TestCallerContextFactory : ICallerContextFactory
{
    public const string ApiKey = "ak_itest.default";
    public const string DefaultTenantId = "tenant-itest";
    public const string DefaultEnvironment = "dev";

    private Dictionary<string, ICallerContext> _contexts;

    public TestCallerContextFactory()
    {
        _contexts = new Dictionary<string, ICallerContext>(StringComparer.Ordinal);
        Reset();
    }

    public Task<ICallerContext?> FromApiKeyAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(presentedKey))
        {
            return Task.FromResult<ICallerContext?>(null);
        }

        _contexts.TryGetValue(presentedKey, out var context);
        return Task.FromResult(context);
    }

    public Task<ICallerContext?> FromBearerTokenAsync(string bearerToken, CancellationToken cancellationToken = default)
    {
        return Task.FromResult<ICallerContext?>(null);
    }

    public void Reset()
    {
        var defaultContext = new CallerContext(
            DefaultTenantId,
            DefaultEnvironment,
            CallerType.ApiKey,
            CallerId: "itest-client",
            Scopes: new[]
            {
                CroniqScopes.WebhooksRead,
                CroniqScopes.WebhooksWrite,
                CroniqScopes.WebhooksRotate,
                CroniqScopes.WebhooksDeadLetter
            });

        _contexts = new Dictionary<string, ICallerContext>(StringComparer.Ordinal)
        {
            [ApiKey] = defaultContext
        };
    }

    public void AddContext(string apiKey, ICallerContext context)
    {
        if (string.IsNullOrWhiteSpace(apiKey)) throw new ArgumentNullException(nameof(apiKey));
        _contexts[apiKey] = context ?? throw new ArgumentNullException(nameof(context));
    }
}
