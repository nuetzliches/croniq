using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using Croniq.Auth.Abstractions;
using Croniq.Core.Execution;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

internal sealed class WorkerGrpcService : Worker.WorkerBase
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Api.Grpc.Worker");
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly IJobStore _jobStore;
    private readonly IExecutionLogStore _executionLogStore;
    private readonly IWorkItemStore _workItemStore;
    private readonly ILoggerFactory _loggerFactory;
    private readonly ILogger<WorkerGrpcService> _logger;

    public WorkerGrpcService(
        ICallerContextAccessor callerAccessor,
        IJobStore jobStore,
        IExecutionLogStore executionLogStore,
        IWorkItemStore workItemStore,
        ILoggerFactory loggerFactory,
        ILogger<WorkerGrpcService> logger)
    {
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _jobStore = jobStore ?? throw new ArgumentNullException(nameof(jobStore));
        _executionLogStore = executionLogStore ?? throw new ArgumentNullException(nameof(executionLogStore));
        _workItemStore = workItemStore ?? throw new ArgumentNullException(nameof(workItemStore));
        _loggerFactory = loggerFactory ?? throw new ArgumentNullException(nameof(loggerFactory));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public override async Task Connect(
        IAsyncStreamReader<RunnerMessage> requestStream,
        IServerStreamWriter<ServerMessage> responseStream,
        ServerCallContext context)
    {
        using var activity = ActivitySource.StartActivity("Croniq.Grpc.Worker.Connect", ActivityKind.Server);

        var caller = _callerAccessor.Current;
        if (caller is null)
        {
            throw new RpcException(new Status(StatusCode.Unauthenticated, "caller context is not available."));
        }

        var environment = caller.EnvironmentTag;
        if (string.IsNullOrWhiteSpace(environment))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "environment_tag is required for Worker.Connect."));
        }

        var environmentTag = environment.Trim();
        EnsureTenantOrThrow(TenantGuard.EnsureTenant(_callerAccessor, caller.TenantId, environmentTag, CroniqScopes.WorkPoll, CroniqScopes.WorkAck));

        if (!await requestStream.MoveNext().ConfigureAwait(false))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "runner hello required."));
        }

        var hello = requestStream.Current?.Hello;
        if (hello is null)
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "first message must be hello."));
        }

        if (string.IsNullOrWhiteSpace(hello.RunnerId))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "runner_id is required."));
        }

        var runnerId = hello.RunnerId.Trim();
        EnsureRunnerIdentityOrThrow(caller, runnerId);

        activity?.SetTag("croniq.tenant_id", caller.TenantId);
        activity?.SetTag("croniq.environment", environmentTag);
        activity?.SetTag("croniq.runner_id", runnerId);

        await responseStream.WriteAsync(new ServerMessage
        {
            Hello = new ServerHello
            {
                ServerId = Environment.MachineName,
                TenantId = caller.TenantId,
                EnvironmentTag = environmentTag,
                ServerTimeUtc = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
            }
        }).ConfigureAwait(false);

        var maxInflight = NormalizeMaxInflight(hello.MaxInflight);
        var scope = new PartitionScope(caller.TenantId, environmentTag);
        var inflight = new ConcurrentDictionary<string, TriggerLease>(StringComparer.OrdinalIgnoreCase);
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(context.CancellationToken);
        var assignmentLoop = Task.Run(() => AssignWorkLoopAsync(
            responseStream,
            inflight,
            runnerId,
            scope,
            maxInflight,
            cts.Token), cts.Token);

        try
        {
            while (await requestStream.MoveNext().ConfigureAwait(false))
            {
                var message = requestStream.Current;
                if (message is null)
                {
                    continue;
                }

                await HandleRunnerMessageAsync(
                    message,
                    inflight,
                    runnerId,
                    scope,
                    cts.Token).ConfigureAwait(false);
            }

            activity?.SetStatus(ActivityStatusCode.Ok);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Worker.Connect stream ended unexpectedly.");
            activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
        }
        finally
        {
            cts.Cancel();
            try
            {
                await assignmentLoop.ConfigureAwait(false);
            }
            catch
            {
                // ignore background assignment errors on shutdown
            }
        }
    }

    private static void EnsureTenantOrThrow(IResult? failure)
    {
        if (failure is null)
        {
            return;
        }

        var status = StatusCode.PermissionDenied;
        var detail = "tenant_mismatch";

        if (failure is IStatusCodeHttpResult statusResult)
        {
            if (statusResult.StatusCode == StatusCodes.Status401Unauthorized)
            {
                status = StatusCode.Unauthenticated;
                detail = "unauthenticated";
            }
        }

        throw new RpcException(new Status(status, detail));
    }

    private static void EnsureRunnerIdentityOrThrow(ICallerContext caller, string runnerId)
    {
        if (!string.Equals(caller.CallerId, runnerId, StringComparison.OrdinalIgnoreCase))
        {
            throw new RpcException(new Status(StatusCode.PermissionDenied, "runner-mismatch"));
        }
    }

    private async Task AssignWorkLoopAsync(
        IServerStreamWriter<ServerMessage> responseStream,
        ConcurrentDictionary<string, TriggerLease> inflight,
        string runnerId,
        PartitionScope scope,
        int maxInflight,
        CancellationToken cancellationToken)
    {
        var pollInterval = TimeSpan.FromMilliseconds(250);

        while (!cancellationToken.IsCancellationRequested)
        {
            try
            {
                var available = maxInflight - inflight.Count;
                if (available > 0)
                {
                    var request = new TriggerAcquireRequest(scope, runnerId, DateTimeOffset.UtcNow, Math.Min(available, 250));
                    var leases = await _jobStore.AcquireAsync(request, cancellationToken).ConfigureAwait(false);

                    foreach (var lease in leases)
                    {
                        var normalized = EnsureExecutionId(lease);
                        if (!inflight.TryAdd(normalized.LeaseId, normalized))
                        {
                            continue;
                        }

                        await TryStoreExecutionStartedAsync(normalized, runnerId, cancellationToken).ConfigureAwait(false);
                        await TryTrackAssignmentAsync(normalized, runnerId, scope, cancellationToken).ConfigureAwait(false);

                        await responseStream.WriteAsync(new ServerMessage
                        {
                            Assigned = new WorkAssigned
                            {
                                ExecutionId = normalized.ExecutionId ?? string.Empty,
                                LeaseId = normalized.LeaseId,
                                TriggerId = normalized.TriggerId,
                                JobKey = normalized.JobKey,
                                FireAtUtc = normalized.FireAtUtc.ToUnixTimeMilliseconds(),
                                LeaseExpiresAtUtc = normalized.LeaseExpiresAtUtc.ToUnixTimeMilliseconds(),
                                Payload = normalized.Payload ?? string.Empty
                            }
                        }).ConfigureAwait(false);
                    }
                }
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Worker.Assign loop failed.");
            }

            try
            {
                await Task.Delay(pollInterval, cancellationToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private async Task HandleRunnerMessageAsync(
        RunnerMessage message,
        ConcurrentDictionary<string, TriggerLease> inflight,
        string runnerId,
        PartitionScope scope,
        CancellationToken cancellationToken)
    {
        if (message is null)
        {
            return;
        }

        if (message.AckSuccess is not null)
        {
            await HandleAckAsync(
                message.AckSuccess.ExecutionId,
                message.AckSuccess.LeaseId,
                succeeded: true,
                deadLetterReason: null,
                nextFireTimeUtc: null,
                inflight,
                runnerId,
                cancellationToken).ConfigureAwait(false);
            return;
        }

        if (message.AckFailure is not null)
        {
            var nextFireTimeUtc = message.AckFailure.NextFireTimeUtc > 0
                ? DateTimeOffset.FromUnixTimeMilliseconds(message.AckFailure.NextFireTimeUtc)
                : (DateTimeOffset?)null;
            var reason = nextFireTimeUtc.HasValue
                ? null
                : (string.IsNullOrWhiteSpace(message.AckFailure.DeadLetterReason)
                    ? "work-failed"
                    : message.AckFailure.DeadLetterReason);
            await HandleAckAsync(
                message.AckFailure.ExecutionId,
                message.AckFailure.LeaseId,
                succeeded: false,
                deadLetterReason: reason,
                nextFireTimeUtc: nextFireTimeUtc,
                inflight,
                runnerId,
                cancellationToken).ConfigureAwait(false);
            return;
        }

        if (message.Events is not null)
        {
            EnsureTenantOrThrow(TenantGuard.EnsureTenant(_callerAccessor, scope.TenantId, scope.EnvironmentTag, CroniqScopes.WorkEvents));
            await HandleEventsAsync(message.Events, inflight, runnerId, scope, cancellationToken).ConfigureAwait(false);
        }
    }

    private async Task HandleAckAsync(
        string executionId,
        string leaseId,
        bool succeeded,
        string? deadLetterReason,
        DateTimeOffset? nextFireTimeUtc,
        ConcurrentDictionary<string, TriggerLease> inflight,
        string runnerId,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(leaseId))
        {
            return;
        }

        if (!inflight.TryRemove(leaseId, out var lease))
        {
            _logger.LogWarning("Ack received for unknown lease {LeaseId}.", leaseId);
            return;
        }

        if (!string.IsNullOrWhiteSpace(executionId)
            && !string.Equals(executionId, lease.ExecutionId, StringComparison.OrdinalIgnoreCase))
        {
            _logger.LogWarning("Ack execution id mismatch for lease {LeaseId}.", leaseId);
            return;
        }

        var release = new TriggerReleaseRequest(lease, runnerId, succeeded, nextFireTimeUtc, deadLetterReason);
        try
        {
            await _jobStore.ReleaseAsync(release, cancellationToken).ConfigureAwait(false);
            await TryStoreExecutionCompletedAsync(lease.ExecutionId, succeeded, deadLetterReason, cancellationToken).ConfigureAwait(false);
            await TryTrackCompletionAsync(lease, runnerId, succeeded, deadLetterReason, cancellationToken).ConfigureAwait(false);
        }
        catch (InvalidOperationException ex)
        {
            _logger.LogWarning(ex, "Failed to release lease {LeaseId}.", leaseId);
        }
    }

    private async Task HandleEventsAsync(
        WorkEvents events,
        ConcurrentDictionary<string, TriggerLease> inflight,
        string runnerId,
        PartitionScope scope,
        CancellationToken cancellationToken)
    {
        if (events is null || events.Events is null || events.Events.Count == 0)
        {
            return;
        }

        if (!inflight.TryGetValue(events.LeaseId, out var lease))
        {
            _logger.LogWarning("Events received for unknown lease {LeaseId}.", events.LeaseId);
            return;
        }

        if (!string.IsNullOrWhiteSpace(events.ExecutionId)
            && !string.Equals(events.ExecutionId, lease.ExecutionId, StringComparison.OrdinalIgnoreCase))
        {
            _logger.LogWarning("Event execution id mismatch for lease {LeaseId}.", events.LeaseId);
            return;
        }

        await TryTrackRenewalAsync(lease, runnerId, cancellationToken).ConfigureAwait(false);

        var logger = _loggerFactory.CreateLogger("Croniq.Api.WorkerEvents");
        var baseScope = BuildWorkEventScope(lease, runnerId, scope);

        foreach (var entry in events.Events)
        {
            if (entry is null || string.IsNullOrWhiteSpace(entry.Message))
            {
                continue;
            }

            using var scopeHandle = logger.BeginScope(MergeEventScope(baseScope, entry));
            logger.Log(ParseLogLevel(entry.Level), "{WorkerEvent}", entry.Message);
        }
    }

    private async Task TryStoreExecutionStartedAsync(TriggerLease lease, string runnerId, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return;
        }

        var nowUtc = DateTimeOffset.UtcNow;
        var activity = Activity.Current;
        var record = new ExecutionRecord(
            lease.ExecutionId,
            ExecutionKind.Job,
            null,
            lease.JobKey,
            lease.Scope.TenantId,
            lease.Scope.EnvironmentTag,
            lease.TriggerId,
            lease.FireAtUtc,
            nowUtc,
            runnerId,
            activity?.TraceId.ToString(),
            activity?.SpanId.ToString(),
            null);

        try
        {
            await _executionLogStore.OnExecutionStartedAsync(record, cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            // best-effort: assignment should not fail if logging storage is unavailable
        }
    }

    private async Task TryStoreExecutionCompletedAsync(
        string? executionId,
        bool succeeded,
        string? deadLetterReason,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(executionId))
        {
            return;
        }

        var completion = new ExecutionCompletion(
            executionId,
            DateTimeOffset.UtcNow,
            succeeded ? ExecutionStatus.Succeeded : ExecutionStatus.Failed,
            DurationMs: null,
            ErrorType: succeeded ? null : "work-failed",
            ErrorMessage: succeeded ? null : deadLetterReason);

        try
        {
            await _executionLogStore.OnExecutionCompletedAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            // best-effort: ack should not fail if logging storage is unavailable
        }
    }

    private static TriggerLease EnsureExecutionId(TriggerLease lease)
    {
        if (!string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return lease;
        }

        return lease with { ExecutionId = Guid.NewGuid().ToString("N") };
    }

    private static int NormalizeMaxInflight(int maxInflight)
    {
        if (maxInflight <= 0)
        {
            return 1;
        }

        return Math.Min(maxInflight, 250);
    }

    private static Dictionary<string, object?> BuildWorkEventScope(TriggerLease lease, string runnerId, PartitionScope scope)
    {
        return new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase)
        {
            ["croniq.execution_id"] = lease.ExecutionId,
            ["croniq.job.key"] = lease.JobKey,
            ["croniq.trigger.id"] = lease.TriggerId,
            ["croniq.tenant_id"] = scope.TenantId,
            ["croniq.environment"] = scope.EnvironmentTag,
            ["croniq.runner_id"] = runnerId
        };
    }

    private static Dictionary<string, object?> MergeEventScope(Dictionary<string, object?> baseScope, WorkEvent entry)
    {
        var scope = new Dictionary<string, object?>(baseScope, StringComparer.OrdinalIgnoreCase);

        if (!string.IsNullOrWhiteSpace(entry.EventType))
        {
            scope["croniq.event.type"] = entry.EventType;
        }

        if (entry.TimestampUtc > 0)
        {
            scope["event.timestamp_utc"] = DateTimeOffset.FromUnixTimeMilliseconds(entry.TimestampUtc);
        }

        if (entry.Properties is not null)
        {
            foreach (var pair in entry.Properties)
            {
                if (string.IsNullOrWhiteSpace(pair.Key))
                {
                    continue;
                }

                scope[$"event.{pair.Key}"] = pair.Value;
            }
        }

        return scope;
    }

    private static LogLevel ParseLogLevel(string? level)
    {
        if (string.IsNullOrWhiteSpace(level))
        {
            return LogLevel.Information;
        }

        return Enum.TryParse<LogLevel>(level, ignoreCase: true, out var parsed)
            ? parsed
            : LogLevel.Information;
    }

    private async Task TryTrackAssignmentAsync(TriggerLease lease, string runnerId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return;
        }

        var assignment = new WorkAssignment(
            scope,
            lease.ExecutionId,
            lease.JobKey,
            lease.TriggerId,
            Attempt: 1,
            RunnerId: runnerId,
            LeaseId: lease.LeaseId,
            LeaseExpiresAtUtc: lease.LeaseExpiresAtUtc,
            Payload: lease.Payload,
            AssignedAtUtc: DateTimeOffset.UtcNow);

        try
        {
            await _workItemStore.UpsertAssignmentAsync(assignment, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to track work assignment {ExecutionId}.", lease.ExecutionId);
        }
    }

    private async Task TryTrackRenewalAsync(TriggerLease lease, string runnerId, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return;
        }

        var renewal = new WorkLeaseRenewal(
            lease.LeaseId,
            runnerId,
            lease.LeaseExpiresAtUtc,
            DateTimeOffset.UtcNow,
            lease.ExecutionId);

        try
        {
            await _workItemStore.TryRenewAsync(renewal, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to track work lease renewal {LeaseId}.", lease.LeaseId);
        }
    }

    private async Task TryTrackCompletionAsync(
        TriggerLease lease,
        string runnerId,
        bool succeeded,
        string? deadLetterReason,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return;
        }

        var completion = new WorkCompletion(
            lease.LeaseId,
            runnerId,
            succeeded,
            DateTimeOffset.UtcNow,
            deadLetterReason,
            lease.ExecutionId);

        try
        {
            await _workItemStore.TryCompleteAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to track work completion {LeaseId}.", lease.LeaseId);
        }
    }
}
