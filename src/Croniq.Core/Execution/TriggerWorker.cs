using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
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
    private readonly IJobStore _jobStore;
    private readonly IJobRegistry _registry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly ILogger<TriggerWorker> _logger;
    private readonly CroniqOptions _options;
    private readonly ActivitySource _activitySource;
    private readonly IMisfirePolicy _misfirePolicy;
    private readonly IPolicyResolver _policyResolver;

    public TriggerWorker(
        IJobStore jobStore,
        IJobRegistry registry,
        IJobExecutionPipeline pipeline,
        IMisfirePolicy misfirePolicy,
        IPolicyResolver policyResolver,
        IOptions<CroniqOptions> options,
        ILogger<TriggerWorker> logger,
        ActivitySource activitySource)
    {
        _jobStore = jobStore ?? throw new ArgumentNullException(nameof(jobStore));
        _registry = registry ?? throw new ArgumentNullException(nameof(registry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _misfirePolicy = misfirePolicy ?? throw new ArgumentNullException(nameof(misfirePolicy));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _activitySource = activitySource ?? new ActivitySource("Croniq.Core.TriggerWorker");
    }

    public async Task<int> ProcessBatchAsync(DateTimeOffset nowUtc, int batchSize, CancellationToken cancellationToken)
    {
        var acquireRequest = new TriggerAcquireRequest(
            new PartitionScope(_options.TenantId, _options.EnvironmentTag),
            _options.InstanceId,
            nowUtc,
            batchSize);

        var leases = await _jobStore.AcquireAsync(acquireRequest, cancellationToken).ConfigureAwait(false);
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

            var misfireOptions = _policyResolver.ResolveMisfire(jobKey);
            var misfire = _misfirePolicy.Evaluate(lease, misfireOptions, nowUtc);
            if (misfire.IsMisfire)
            {
                _logger.LogWarning("Misfire detected for lease {LeaseId} at {FireAt}, reason {Reason}", lease.LeaseId, lease.FireAtUtc, misfire.Reason);
                await ReleaseAsync(
                    lease,
                    succeeded: false,
                    deadLetterReason: misfireOptions.DeadLetterOnMisfire ? misfire.Reason : null,
                    nextFireTimeUtc: misfireOptions.DeadLetterOnMisfire ? null : nowUtc.Add(misfireOptions.RescheduleBackoff ?? TimeSpan.FromSeconds(30)),
                    cancellationToken).ConfigureAwait(false);
                continue;
            }

            try
            {
                var metadata = lease.Payload is null
                    ? null
                    : new Dictionary<string, string> { { "payload", lease.Payload } };

                var request = new JobExecutionRequest(jobKey, descriptor, metadata, _activitySource);
                await _pipeline.ExecuteAsync(request, cancellationToken).ConfigureAwait(false);

                await ReleaseAsync(lease, succeeded: true, deadLetterReason: null, nextFireTimeUtc: null, cancellationToken).ConfigureAwait(false);
                processed++;
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Error executing job {JobKey} for lease {LeaseId}", lease.JobKey, lease.LeaseId);
                await ReleaseAsync(lease, succeeded: false, deadLetterReason: "execution-error", nextFireTimeUtc: null, cancellationToken).ConfigureAwait(false);
            }
        }

        return processed;
    }

    private Task ReleaseAsync(TriggerLease lease, bool succeeded, string? deadLetterReason, DateTimeOffset? nextFireTimeUtc, CancellationToken cancellationToken)
    {
        var release = new TriggerReleaseRequest(lease, succeeded, nextFireTimeUtc, deadLetterReason);
        return _jobStore.ReleaseAsync(release, cancellationToken);
    }
}
