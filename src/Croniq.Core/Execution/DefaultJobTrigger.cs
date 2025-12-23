using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Croniq.Options;
using Croniq.Sdk;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Execution;

public sealed class DefaultJobTrigger : IJobTrigger
{
    private readonly IJobRegistry _registry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly IPolicyResolver _policyResolver;
    private readonly IJobPersistenceProvider _store;
    private readonly IOptions<CroniqOptions> _options;

    public DefaultJobTrigger(
        IJobRegistry registry,
        IJobExecutionPipeline pipeline,
        IPolicyResolver policyResolver,
        IJobPersistenceProvider store,
        IOptions<CroniqOptions> options)
    {
        _registry = registry ?? throw new ArgumentNullException(nameof(registry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public async Task TriggerOnceAsync(
        string jobKey,
        IReadOnlyDictionary<string, string>? metadata = null,
        TimeSpan? delay = null,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(jobKey))
        {
            throw new ArgumentException("JobKey is required.", nameof(jobKey));
        }

        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new InvalidOperationException($"JobKey '{jobKey}' is invalid.");
        }

        if (!_registry.TryGet(parsed, out var descriptor))
        {
            throw new InvalidOperationException($"JobKey '{jobKey}' is not registered.");
        }

        if (delay.HasValue && delay.Value < TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(delay), "Delay cannot be negative.");
        }

        var normalizedMetadata = NormalizeMetadata(metadata);

        if (delay.HasValue && delay.Value > TimeSpan.Zero)
        {
            await ScheduleOnceAsync(parsed, descriptor, normalizedMetadata, delay.Value, cancellationToken).ConfigureAwait(false);
            return;
        }

        await ExecuteImmediateAsync(parsed, descriptor, normalizedMetadata, cancellationToken).ConfigureAwait(false);
    }

    private async Task ExecuteImmediateAsync(
        JobKey jobKey,
        JobDescriptor descriptor,
        IReadOnlyDictionary<string, string>? metadata,
        CancellationToken cancellationToken)
    {
        var scope = GetScope();
        var executionOptions = _policyResolver.ResolveExecution(jobKey, scope);
        var executionId = Guid.NewGuid().ToString("N");
        var request = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, activitySource: null);
        await _pipeline.ExecuteAsync(request, cancellationToken).ConfigureAwait(false);
    }

    private async Task ScheduleOnceAsync(
        JobKey jobKey,
        JobDescriptor descriptor,
        IReadOnlyDictionary<string, string>? metadata,
        TimeSpan delay,
        CancellationToken cancellationToken)
    {
        var scope = GetScope();
        var existing = await _store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
        if (existing is null)
        {
            var jobDefinition = new JobDefinition(
                jobKey.Value,
                descriptor.Attribute.NamespaceSegment,
                descriptor.Attribute.JobName,
                descriptor.Attribute.Variant,
                Description: null,
                Metadata: null);

            await _store.UpsertJobAsync(jobDefinition, scope, cancellationToken).ConfigureAwait(false);
        }

        var triggerId = $"{jobKey.Value}:once-{Guid.NewGuid():N}";
        var startAtUtc = DateTimeOffset.UtcNow.Add(delay);

        var trigger = new TriggerDefinition(
            triggerId,
            jobKey.Value,
            TriggerSchedule.OnceExpression,
            scope,
            startAtUtc,
            EndAtUtc: null,
            Enabled: true,
            Metadata: metadata,
            TimeZoneId: TimeZoneInfo.Utc.Id);

        await _store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);
    }

    private PartitionScope GetScope()
    {
        var current = _options.Value ?? new CroniqOptions();
        return new PartitionScope(current.GetEffectiveTenantId(), current.EnvironmentTag);
    }

    private static IReadOnlyDictionary<string, string>? NormalizeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        return new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);
    }
}
