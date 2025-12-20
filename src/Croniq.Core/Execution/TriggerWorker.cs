using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Execution;

/// <summary>
/// Acquires due triggers, runs jobs, and releases leases back to the store.
/// </summary>
public sealed class TriggerWorker
{
    private const string TriggerIdMetadataKey = "trigger_id";
    private const string PayloadMetadataKey = "payload";
    private static readonly HashSet<string> ReservedMetadataKeys = new(StringComparer.OrdinalIgnoreCase)
    {
        TriggerIdMetadataKey,
        PayloadMetadataKey
    };

    private readonly IJobStore _jobStore;
    private readonly IJobRegistry _registry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly ILogger<TriggerWorker> _logger;
    private readonly CroniqOptions _options;
    private readonly WorkerHostOptions _hostOptions;
    private readonly ActivitySource _activitySource;
    private readonly IMisfirePolicy _misfirePolicy;
    private readonly IPolicyResolver _policyResolver;
    private readonly IQuotaGuard _quotaGuard;
    private readonly IExecutionLogStore _executionLogStore;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public TriggerWorker(
        IJobStore jobStore,
        IJobRegistry registry,
        IJobExecutionPipeline pipeline,
        IMisfirePolicy misfirePolicy,
        IPolicyResolver policyResolver,
        IOptions<CroniqOptions> options,
        IOptions<WorkerHostOptions> hostOptions,
        IQuotaGuard quotaGuard,
        IExecutionLogStore executionLogStore,
        ILogger<TriggerWorker> logger,
        ActivitySource activitySource)
    {
        _jobStore = jobStore ?? throw new ArgumentNullException(nameof(jobStore));
        _registry = registry ?? throw new ArgumentNullException(nameof(registry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _misfirePolicy = misfirePolicy ?? throw new ArgumentNullException(nameof(misfirePolicy));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _quotaGuard = quotaGuard ?? throw new ArgumentNullException(nameof(quotaGuard));
        _executionLogStore = executionLogStore ?? throw new ArgumentNullException(nameof(executionLogStore));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _hostOptions = hostOptions?.Value ?? throw new ArgumentNullException(nameof(hostOptions));
        _activitySource = activitySource ?? new ActivitySource("Croniq.Core.TriggerWorker");
    }

    public async Task<int> ProcessBatchAsync(DateTimeOffset nowUtc, int batchSize, CancellationToken cancellationToken)
    {
        var acquireRequest = new TriggerAcquireRequest(
            new PartitionScope(_options.TenantId, _options.EnvironmentTag),
            _options.InstanceId,
            nowUtc,
            batchSize);

        using var acquireActivity = _activitySource.StartActivity("Croniq.Trigger.Acquire", ActivityKind.Internal);
        acquireActivity?.SetTag("croniq.tenant_id", _options.TenantId);
        acquireActivity?.SetTag("croniq.environment", _options.EnvironmentTag);
        acquireActivity?.SetTag("croniq.batch.size", batchSize);

        var leases = await _jobStore.AcquireAsync(acquireRequest, cancellationToken).ConfigureAwait(false);
        acquireActivity?.SetTag("croniq.trigger.leases", leases.Count);
        acquireActivity?.SetStatus(ActivityStatusCode.Ok);
        var processed = 0;

        foreach (var lease in leases)
        {
            cancellationToken.ThrowIfCancellationRequested();

            if (!JobKey.TryParse(lease.JobKey, out var jobKey) || !_registry.TryGet(jobKey, out var descriptor))
            {
                _logger.LogWarning("No job registered for JobKey {JobKey}, releasing lease {LeaseId} as dead-letter", lease.JobKey, lease.LeaseId);
                await ReleaseAsync(lease, succeeded: false, deadLetterReason: "job-not-registered", nextFireTimeUtc: null, cancellationToken).ConfigureAwait(false);
                continue;
            }

            SchedulerMetrics.AdjustQueueDepth(jobKey, 1);
            using var leaseActivity = _activitySource.StartActivity("Croniq.Trigger.Dispatch", ActivityKind.Internal);
            leaseActivity?.SetTag("croniq.job.key", jobKey.Value);
            leaseActivity?.SetTag("croniq.trigger.lease_id", lease.LeaseId);
            leaseActivity?.SetTag("croniq.trigger.fire_at", lease.FireAtUtc.ToUnixTimeMilliseconds());

            try
            {
                var misfireOptions = _policyResolver.ResolveMisfire(jobKey);
                var misfire = _misfirePolicy.Evaluate(lease, misfireOptions, nowUtc);
                if (misfire.IsMisfire)
                {
                    _logger.LogWarning("Misfire detected for lease {LeaseId} at {FireAt}, reason {Reason}", lease.LeaseId, lease.FireAtUtc, misfire.Reason);
                    SchedulerMetrics.RecordMisfire(jobKey, misfire.Reason ?? "unknown");
                    leaseActivity?.SetStatus(ActivityStatusCode.Error, "misfire");
                    await ReleaseAsync(
                        lease,
                        succeeded: false,
                        deadLetterReason: misfireOptions.DeadLetterOnMisfire ? misfire.Reason : null,
                        nextFireTimeUtc: misfireOptions.DeadLetterOnMisfire ? null : nowUtc.Add(misfireOptions.RescheduleBackoff ?? TimeSpan.FromSeconds(30)),
                        cancellationToken).ConfigureAwait(false);
                    continue;
                }

                var quotaOptions = _policyResolver.ResolveQuota(jobKey);
                if (!_quotaGuard.TryAcquire(jobKey, quotaOptions, nowUtc, out var retryAt))
                {
                    _logger.LogWarning("Quota limit reached for {JobKey}; lease {LeaseId} will be rescheduled", lease.JobKey, lease.LeaseId);
                    SchedulerMetrics.RecordQuotaReschedule(jobKey);
                    leaseActivity?.SetStatus(ActivityStatusCode.Error, "quota-limit");
                    await ReleaseAsync(
                        lease,
                        succeeded: false,
                        deadLetterReason: "quota-limit",
                        nextFireTimeUtc: retryAt ?? nowUtc.AddSeconds(5),
                        cancellationToken).ConfigureAwait(false);
                    continue;
                }

                var executionOptions = _policyResolver.ResolveExecution(jobKey);
                var metadata = BuildExecutionMetadata(lease);

                var executionId = Guid.NewGuid().ToString("N");
                leaseActivity?.SetTag("croniq.execution_id", executionId);
                await TryStoreExecutionStartedAsync(executionId, lease, jobKey, leaseActivity, cancellationToken).ConfigureAwait(false);

                Stopwatch? executionTimer = null;
                using var jobCancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                var renewTask = StartLeaseRenewalAsync(lease, jobKey, jobCancellation, cancellationToken);

                try
                {
                    var request = new JobExecutionRequest(executionId, jobKey, descriptor, executionOptions, metadata, _activitySource);
                    executionTimer = Stopwatch.StartNew();
                    await _pipeline.ExecuteAsync(request, jobCancellation.Token).ConfigureAwait(false);

                    var elapsedMs = executionTimer.Elapsed.TotalMilliseconds;
                    var leaseLost = await CompleteLeaseRenewalAsync(renewTask, jobCancellation).ConfigureAwait(false);
                    if (leaseLost)
                    {
                        SchedulerMetrics.RecordJobExecution(jobKey, succeeded: false, elapsedMs);
                        leaseActivity?.SetStatus(ActivityStatusCode.Error, "lease-lost");
                        await TryStoreExecutionCompletedAsync(executionId, ExecutionStatus.Canceled, elapsedMs, null, cancellationToken).ConfigureAwait(false);
                        _logger.LogWarning("Lease {LeaseId} for job {JobKey} was lost; skipping release.", lease.LeaseId, lease.JobKey);
                    }
                    else
                    {
                        SchedulerMetrics.RecordJobExecution(jobKey, succeeded: true, elapsedMs);
                        leaseActivity?.SetStatus(ActivityStatusCode.Ok);
                        await TryStoreExecutionCompletedAsync(executionId, ExecutionStatus.Succeeded, elapsedMs, null, cancellationToken).ConfigureAwait(false);

                        await ReleaseAsync(lease, succeeded: true, deadLetterReason: null, nextFireTimeUtc: null, cancellationToken).ConfigureAwait(false);
                        processed++;
                    }
                }
                catch (Exception ex)
                {
                    var elapsedMs = executionTimer?.Elapsed.TotalMilliseconds ?? 0d;
                    var leaseLost = await CompleteLeaseRenewalAsync(renewTask, jobCancellation).ConfigureAwait(false);
                    var canceled = IsCancellation(ex, jobCancellation.Token);
                    SchedulerMetrics.RecordJobExecution(jobKey, succeeded: false, elapsedMs);
                    if (canceled)
                    {
                        leaseActivity?.SetStatus(ActivityStatusCode.Error, "canceled");
                        _logger.LogWarning("Execution canceled for job {JobKey} (lease {LeaseId})", lease.JobKey, lease.LeaseId);
                    }
                    else
                    {
                        leaseActivity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                        _logger.LogError(ex, "Error executing job {JobKey} for lease {LeaseId}", lease.JobKey, lease.LeaseId);
                    }

                    var releaseReason = "execution-error";
                    var status = canceled ? ExecutionStatus.Canceled : ExecutionStatus.Failed;
                    await TryStoreExecutionCompletedAsync(executionId, status, elapsedMs, canceled ? null : ex, cancellationToken).ConfigureAwait(false);

                    if (executionOptions.DeadLetter.Enabled && !canceled && !leaseLost)
                    {
                        var occurredAtUtc = DateTimeOffset.UtcNow;
                        var metadataSnapshot = CloneMetadata(metadata);
                        metadataSnapshot["exception.type"] = ex.GetType().FullName ?? ex.GetType().Name;
                        metadataSnapshot["exception.message"] = ex.Message;
                        metadataSnapshot["execution.reason"] = "policy-deadletter";
                        if (!string.IsNullOrWhiteSpace(executionOptions.DeadLetter.OperatorHint))
                        {
                            metadataSnapshot["deadletter.hint"] = executionOptions.DeadLetter.OperatorHint!;
                        }

                        var deadLetterPayload = BuildDeadLetterPayload(lease, metadataSnapshot, executionOptions, ex, occurredAtUtc);
                        var metadataView = metadataSnapshot.Count == 0 ? null : metadataSnapshot;
                        var deadLetter = new DeadLetterRequest(
                            lease,
                            releaseReason,
                            occurredAtUtc,
                            executionOptions.DeadLetter.Retention,
                            deadLetterPayload,
                            metadataView);

                        await TryDeadLetterAsync(jobKey, deadLetter, cancellationToken).ConfigureAwait(false);
                        releaseReason = null;
                    }

                    if (!leaseLost)
                    {
                        await ReleaseAsync(lease, succeeded: false, deadLetterReason: releaseReason, nextFireTimeUtc: null, cancellationToken).ConfigureAwait(false);
                    }
                }
                finally
                {
                    _quotaGuard.Release(jobKey);
                }
            }
            finally
            {
                SchedulerMetrics.AdjustQueueDepth(jobKey, -1);
            }
        }

        return processed;
    }

    private Task ReleaseAsync(TriggerLease lease, bool succeeded, string? deadLetterReason, DateTimeOffset? nextFireTimeUtc, CancellationToken cancellationToken)
    {
        var release = new TriggerReleaseRequest(lease, succeeded, nextFireTimeUtc, deadLetterReason);
        return _jobStore.ReleaseAsync(release, cancellationToken);
    }

    private async Task TryDeadLetterAsync(JobKey jobKey, DeadLetterRequest request, CancellationToken cancellationToken)
    {
        try
        {
            await _jobStore.MoveToDeadLetterAsync(request, cancellationToken).ConfigureAwait(false);
            var reason = string.IsNullOrWhiteSpace(request.Reason) ? "unknown" : request.Reason;
            _logger.LogWarning(
                "Policy transition {Policy} for job {JobKey}: routed lease {LeaseId} to dead-letter (reason: {Reason})",
                "dead-letter",
                jobKey.Value,
                request.Lease.LeaseId,
                reason);
            PolicyMetrics.RecordDeadLetter(jobKey, reason);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to persist dead-letter for lease {LeaseId}", request.Lease.LeaseId);
        }
    }

    private string BuildDeadLetterPayload(
        TriggerLease lease,
        IReadOnlyDictionary<string, string>? metadata,
        ExecutionPolicyOptions executionOptions,
        Exception exception,
        DateTimeOffset occurredAtUtc)
    {
        var envelope = new DeadLetterEnvelope(
            lease.JobKey,
            lease.TriggerId,
            lease.LeaseId,
            lease.Scope.TenantId,
            lease.Scope.EnvironmentTag,
            _options.InstanceId,
            lease.FireAtUtc,
            occurredAtUtc,
            lease.Payload,
            metadata,
            new DeadLetterExceptionSnapshot(
                exception.GetType().FullName ?? exception.GetType().Name,
                exception.Message,
                exception.ToString()),
            BuildPolicySnapshot(executionOptions));

        return JsonSerializer.Serialize(envelope, _jsonOptions);
    }

    private static DeadLetterPolicySnapshot BuildPolicySnapshot(ExecutionPolicyOptions options)
    {
        var retryExceptions = options.Retry.RetryableExceptions ?? Array.Empty<string>();
        return new DeadLetterPolicySnapshot(
            new DeadLetterRetrySnapshot(
                options.Retry.Enabled,
                options.Retry.MaxAttempts,
                options.Retry.BackoffStrategy,
                options.Retry.InitialDelay,
                options.Retry.MaxDelay,
                options.Retry.JitterFactor,
                retryExceptions),
            new DeadLetterTimeoutSnapshot(
                options.Timeout.Enabled,
                options.Timeout.Timeout,
                options.Timeout.CancelExecutionOnTimeout),
            new DeadLetterCircuitBreakerSnapshot(
                options.CircuitBreaker.Enabled,
                options.CircuitBreaker.FailureThreshold,
                options.CircuitBreaker.SamplingWindow,
                options.CircuitBreaker.BreakDuration,
                options.CircuitBreaker.MinimumThroughput),
            new DeadLetterDeadLetterOptionsSnapshot(
                options.DeadLetter.Enabled,
                options.DeadLetter.Retention,
                options.DeadLetter.OperatorHint));
    }

    private static Dictionary<string, string> CloneMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        }

        return new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);
    }

    private Dictionary<string, string> BuildExecutionMetadata(TriggerLease lease)
    {
        var metadata = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            { TriggerIdMetadataKey, lease.TriggerId }
        };

        if (!string.IsNullOrWhiteSpace(lease.Payload))
        {
            if (!TryMergePayloadMetadata(metadata, lease.Payload))
            {
                metadata[PayloadMetadataKey] = lease.Payload;
            }
        }

        return metadata;
    }

    private bool TryMergePayloadMetadata(Dictionary<string, string> metadata, string payload)
    {
        Dictionary<string, string>? parsed;
        try
        {
            parsed = JsonSerializer.Deserialize<Dictionary<string, string>>(payload, _jsonOptions);
        }
        catch (JsonException)
        {
            return false;
        }

        if (parsed is null || parsed.Count == 0)
        {
            return false;
        }

        var merged = false;
        foreach (var pair in parsed)
        {
            if (string.IsNullOrWhiteSpace(pair.Key) || ReservedMetadataKeys.Contains(pair.Key))
            {
                continue;
            }

            metadata[pair.Key] = pair.Value ?? string.Empty;
            merged = true;
        }

        return merged;
    }

    private Task<bool>? StartLeaseRenewalAsync(
        TriggerLease lease,
        JobKey jobKey,
        CancellationTokenSource jobCancellation,
        CancellationToken hostCancellation)
    {
        if (_hostOptions.LeaseRenewalLeadTime <= TimeSpan.Zero)
        {
            return null;
        }

        return MaintainLeaseAsync(lease, jobKey, jobCancellation, hostCancellation);
    }

    private async Task<bool> CompleteLeaseRenewalAsync(Task<bool>? renewTask, CancellationTokenSource jobCancellation)
    {
        if (renewTask is null)
        {
            return false;
        }

        if (!jobCancellation.IsCancellationRequested)
        {
            jobCancellation.Cancel();
        }

        try
        {
            return await renewTask.ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return false;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Lease renewal task failed.");
            return false;
        }
    }

    private async Task<bool> MaintainLeaseAsync(
        TriggerLease lease,
        JobKey jobKey,
        CancellationTokenSource jobCancellation,
        CancellationToken hostCancellation)
    {
        var leadTime = _hostOptions.LeaseRenewalLeadTime;
        if (leadTime <= TimeSpan.Zero)
        {
            return false;
        }

        var warned = false;
        var currentLease = lease;

        while (!jobCancellation.IsCancellationRequested && !hostCancellation.IsCancellationRequested)
        {
            var nowUtc = DateTimeOffset.UtcNow;
            var delay = GetRenewalDelay(nowUtc, currentLease.LeaseExpiresAtUtc, leadTime, ref warned, jobKey);
            if (delay > TimeSpan.Zero)
            {
                try
                {
                    await Task.Delay(delay, jobCancellation.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
            }

            if (jobCancellation.IsCancellationRequested || hostCancellation.IsCancellationRequested)
            {
                break;
            }

            var renewRequest = new TriggerLeaseRenewRequest(currentLease, _options.InstanceId, DateTimeOffset.UtcNow);
            TriggerLease? renewed;
            try
            {
                renewed = await _jobStore.TryRenewLeaseAsync(renewRequest, hostCancellation).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (hostCancellation.IsCancellationRequested)
            {
                return false;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Lease {LeaseId} for job {JobKey} could not be renewed; canceling execution.", currentLease.LeaseId, jobKey.Value);
                jobCancellation.Cancel();
                return true;
            }

            if (renewed is null)
            {
                _logger.LogWarning("Lease {LeaseId} for job {JobKey} could not be renewed; canceling execution.", currentLease.LeaseId, jobKey.Value);
                jobCancellation.Cancel();
                return true;
            }

            currentLease = renewed;
        }

        return false;
    }

    private TimeSpan GetRenewalDelay(DateTimeOffset nowUtc, DateTimeOffset leaseExpiresAtUtc, TimeSpan leadTime, ref bool warned, JobKey jobKey)
    {
        var remaining = leaseExpiresAtUtc - nowUtc;
        if (remaining <= TimeSpan.Zero)
        {
            return TimeSpan.Zero;
        }

        var delay = remaining - leadTime;
        if (delay <= TimeSpan.Zero)
        {
            if (!warned)
            {
                _logger.LogWarning(
                    "LeaseRenewalLeadTime {LeadTime} is >= remaining lease {Remaining} for job {JobKey}; using a conservative renewal cadence.",
                    leadTime,
                    remaining,
                    jobKey.Value);
                warned = true;
            }

            delay = TimeSpan.FromTicks(Math.Max(remaining.Ticks / 2, 0));
        }

        return delay;
    }

    private static bool IsCancellation(Exception exception, CancellationToken cancellationToken)
        => cancellationToken.IsCancellationRequested && exception is OperationCanceledException;

    private async Task TryStoreExecutionStartedAsync(string executionId, TriggerLease lease, JobKey jobKey, Activity? activity, CancellationToken cancellationToken)
    {
        try
        {
            var record = new ExecutionRecord(
                executionId,
                ExecutionKind.Job,
                null,
                jobKey.Value,
                lease.Scope.TenantId,
                lease.Scope.EnvironmentTag,
                lease.TriggerId,
                lease.FireAtUtc,
                DateTimeOffset.UtcNow,
                _options.InstanceId,
                activity?.TraceId.ToString(),
                activity?.SpanId.ToString(),
                TryGetCorrelationId(activity));

            await _executionLogStore.OnExecutionStartedAsync(record, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to persist execution start for {ExecutionId}", executionId);
        }
    }

    private async Task TryStoreExecutionCompletedAsync(string executionId, ExecutionStatus status, double? durationMs, Exception? error, CancellationToken cancellationToken)
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

    private static string? TryGetCorrelationId(Activity? activity)
    {
        if (activity?.GetBaggageItem("croniq.correlation_id") is { Length: > 0 } baggageCorrelation)
        {
            return baggageCorrelation;
        }

        if (activity?.GetTagItem("croniq.correlation_id") is string tagCorrelation && !string.IsNullOrWhiteSpace(tagCorrelation))
        {
            return tagCorrelation;
        }

        return null;
    }

    private sealed record DeadLetterEnvelope(
        string JobKey,
        string TriggerKey,
        string LeaseId,
        string TenantId,
        string Environment,
        string? InstanceId,
        DateTimeOffset FireAtUtc,
        DateTimeOffset OccurredAtUtc,
        string? TriggerPayload,
        IReadOnlyDictionary<string, string>? Metadata,
        DeadLetterExceptionSnapshot Exception,
        DeadLetterPolicySnapshot Policy);

    private sealed record DeadLetterExceptionSnapshot(string Type, string Message, string Details);

    private sealed record DeadLetterPolicySnapshot(
        DeadLetterRetrySnapshot Retry,
        DeadLetterTimeoutSnapshot Timeout,
        DeadLetterCircuitBreakerSnapshot CircuitBreaker,
        DeadLetterDeadLetterOptionsSnapshot DeadLetter);

    private sealed record DeadLetterRetrySnapshot(
        bool Enabled,
        int MaxAttempts,
        RetryBackoffStrategy BackoffStrategy,
        TimeSpan InitialDelay,
        TimeSpan MaxDelay,
        double JitterFactor,
        IReadOnlyCollection<string> RetryableExceptions);

    private sealed record DeadLetterTimeoutSnapshot(bool Enabled, TimeSpan Timeout, bool CancelExecutionOnTimeout);

    private sealed record DeadLetterCircuitBreakerSnapshot(bool Enabled, int FailureThreshold, TimeSpan SamplingWindow, TimeSpan BreakDuration, int MinimumThroughput);

    private sealed record DeadLetterDeadLetterOptionsSnapshot(bool Enabled, TimeSpan Retention, string? OperatorHint);
}
