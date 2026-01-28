using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Croniq.Options;
using Croniq.Sdk;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Execution;

public sealed class DefaultJobTrigger : IJobTrigger
{
    private const string TriggerIdMetadataKey = "trigger_id";
    private const string CorrelationIdMetadataKey = "correlation_id";

    private readonly IJobRegistry _registry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly IPolicyResolver _policyResolver;
    private readonly IJobPersistenceProvider _store;
    private readonly IOptions<CroniqOptions> _options;
    private readonly IExecutionLogStore _executionLogStore;
    private readonly ILogger<DefaultJobTrigger> _logger;

    public DefaultJobTrigger(
        IJobRegistry registry,
        IJobExecutionPipeline pipeline,
        IPolicyResolver policyResolver,
        IJobPersistenceProvider store,
        IOptions<CroniqOptions> options,
        IExecutionLogStore executionLogStore,
        ILogger<DefaultJobTrigger> logger)
    {
        _registry = registry ?? throw new ArgumentNullException(nameof(registry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _options = options ?? throw new ArgumentNullException(nameof(options));
        _executionLogStore = executionLogStore ?? throw new ArgumentNullException(nameof(executionLogStore));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task TriggerOnceAsync(
        string jobKey,
        IReadOnlyDictionary<string, string>? metadata = null,
        TimeSpan? delay = null,
        string? executionMode = null,
        string? invocationSource = null,
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

        var normalizedExecutionMode = NormalizeExecutionMode(executionMode);
        var normalizedInvocationSource = NormalizeInvocationSource(invocationSource, ExecutionIntent.InvocationSources.Manual);

        if (delay.HasValue && delay.Value > TimeSpan.Zero)
        {
            await ScheduleOnceAsync(parsed, descriptor, normalizedMetadata, delay.Value, normalizedExecutionMode, normalizedInvocationSource, cancellationToken).ConfigureAwait(false);
            return;
        }

        await ExecuteImmediateAsync(parsed, descriptor, normalizedMetadata, normalizedExecutionMode, normalizedInvocationSource, cancellationToken).ConfigureAwait(false);
    }

    private async Task ExecuteImmediateAsync(
        JobKey jobKey,
        JobDescriptor descriptor,
        IReadOnlyDictionary<string, string>? metadata,
        string executionMode,
        string invocationSource,
        CancellationToken cancellationToken)
    {
        var scope = GetScope();
        var executionOptions = _policyResolver.ResolveExecution(jobKey, scope);
        var executionId = Guid.NewGuid().ToString("N");
        var startedAtUtc = DateTimeOffset.UtcNow;
        var triggerId = ResolveMetadataValue(metadata, TriggerIdMetadataKey);
        var correlationId = TryGetCorrelationId(Activity.Current, metadata);
        var request = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, activitySource: null);
        await TryStoreExecutionStartedAsync(
            new ExecutionRecord(
                executionId,
                ExecutionKind.Job,
                WorkflowId: null,
                jobKey.Value,
                scope.TenantId,
                scope.EnvironmentTag,
                triggerId,
                FireAtUtc: startedAtUtc,
                StartedAtUtc: startedAtUtc,
                _options.Value.InstanceId,
                Activity.Current?.TraceId.ToString(),
                Activity.Current?.SpanId.ToString(),
                correlationId,
                executionMode,
                invocationSource),
            cancellationToken).ConfigureAwait(false);

        var stopwatch = Stopwatch.StartNew();
        try
        {
            await _pipeline.ExecuteAsync(request, cancellationToken).ConfigureAwait(false);
            stopwatch.Stop();
            await TryStoreExecutionCompletedAsync(
                executionId,
                ExecutionStatus.Succeeded,
                stopwatch.Elapsed.TotalMilliseconds,
                error: null,
                cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            stopwatch.Stop();
            var canceled = IsCancellation(ex, cancellationToken);
            await TryStoreExecutionCompletedAsync(
                executionId,
                canceled ? ExecutionStatus.Canceled : ExecutionStatus.Failed,
                stopwatch.Elapsed.TotalMilliseconds,
                canceled ? null : ex,
                cancellationToken).ConfigureAwait(false);
            throw;
        }
    }

    private async Task ScheduleOnceAsync(
        JobKey jobKey,
        JobDescriptor descriptor,
        IReadOnlyDictionary<string, string>? metadata,
        TimeSpan delay,
        string executionMode,
        string invocationSource,
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
                    Metadata: null,
                    AssignedRunnerId: _options.Value.InstanceId,
                    AssignedBy: _options.Value.InstanceId,
                    AssignedAtUtc: DateTimeOffset.UtcNow,
                    AssignmentSource: "system");

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
            TimeZoneId: TimeZoneInfo.Utc.Id,
            CalendarId: null,
            ExecutionMode: executionMode,
            InvocationSource: invocationSource);

        await _store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);
    }

    private PartitionScope GetScope()
    {
        var current = _options.Value ?? new CroniqOptions();
        return new PartitionScope(current.TenantId.Trim(), current.EnvironmentTag);
    }

    private static bool IsCancellation(Exception exception, CancellationToken cancellationToken)
        => cancellationToken.IsCancellationRequested && exception is OperationCanceledException;

    private static string? ResolveMetadataValue(IReadOnlyDictionary<string, string>? metadata, string key)
    {
        if (metadata is null || string.IsNullOrWhiteSpace(key))
        {
            return null;
        }

        if (!metadata.TryGetValue(key, out var value))
        {
            return null;
        }

        return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
    }

    private static string? TryGetCorrelationId(Activity? activity, IReadOnlyDictionary<string, string>? metadata)
    {
        if (activity?.GetBaggageItem("croniq.correlation_id") is { Length: > 0 } baggageCorrelation)
        {
            return baggageCorrelation;
        }

        if (activity?.GetTagItem("croniq.correlation_id") is string tagCorrelation && !string.IsNullOrWhiteSpace(tagCorrelation))
        {
            return tagCorrelation;
        }

        return ResolveMetadataValue(metadata, CorrelationIdMetadataKey);
    }

    private async Task TryStoreExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken)
    {
        try
        {
            await _executionLogStore.OnExecutionStartedAsync(record, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to persist execution start for {ExecutionId}", record.ExecutionId);
        }
    }

    private async Task TryStoreExecutionCompletedAsync(
        string executionId,
        ExecutionStatus status,
        double? durationMs,
        Exception? error,
        CancellationToken cancellationToken)
    {
        try
        {
            var completion = new ExecutionCompletion(
                executionId,
                DateTimeOffset.UtcNow,
                status,
                durationMs,
                error?.GetType().FullName ?? error?.GetType().Name,
                error?.Message);

            await _executionLogStore.OnExecutionCompletedAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to persist execution completion for {ExecutionId}", executionId);
        }
    }

    private static IReadOnlyDictionary<string, string>? NormalizeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        return new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);
    }

    private static string NormalizeExecutionMode(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return ExecutionIntent.ExecutionModes.Normal;
        }

        return value.Trim().ToLowerInvariant();
    }

    private static string NormalizeInvocationSource(string? value, string fallback)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return fallback;
        }

        return value.Trim().ToLowerInvariant();
    }
}
