using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Api.Telemetry;
using Croniq.Auth.Abstractions;
using Croniq.Core.Observability;
using Croniq.Core.Execution;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapWorkEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/tenants/{tenantId}/work/poll", async (
            string tenantId,
            string? environment,
            WorkPollRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IRunnerStore runnerStore,
            [FromServices] IJobStore jobStore,
            [FromServices] IExecutionLogStore executionLogStore,
            [FromServices] IWorkItemStore workItemStore,
            [FromServices] ILoggerFactory loggerFactory,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(request.EnvironmentTag ?? environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (string.IsNullOrWhiteSpace(request.RunnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var runnerId = request.RunnerId.Trim();
            var runnerFailure = EnsureRunnerIdentity(callerContextAccessor, runnerId);
            if (runnerFailure is not null)
            {
                return runnerFailure;
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var runnerInstanceId = RunnerInstanceGuard.ResolveRunnerInstanceId(request.RunnerInstanceId, metadataJson: null);
            var metadataUpdates = RunnerInstanceGuard.BuildMetadataUpdates(
                runnerInstanceId,
                transportState: "polling",
                allowTestExecutions: request.AllowTestExecutions,
                maxInflight: request.MaxInflight,
                capabilities: request.Capabilities);
            var nowUtc = DateTimeOffset.UtcNow;
            var (runnerConflict, _) = await RunnerInstanceGuard.EnsureRunnerInstanceAvailableAsync(
                runnerStore,
                scope,
                runnerId,
                runnerInstanceId,
                metadataJson: null,
                metadataUpdates,
                nowUtc,
                nowUtc,
                cancellationToken).ConfigureAwait(false);
            if (runnerConflict is not null)
            {
                return runnerConflict;
            }

            var previousTransport = ApiMetrics.RecordRunnerTransportSelection(
                tenantId.Trim(),
                resolvedEnvironment,
                runnerId,
                "polling");
            if (!string.IsNullOrWhiteSpace(previousTransport)
                && !string.Equals(previousTransport, "polling", StringComparison.OrdinalIgnoreCase))
            {
                ApiMetrics.RecordRunnerTransportTransition(tenantId.Trim(), resolvedEnvironment, previousTransport, "polling");
                var transportLogger = loggerFactory.CreateLogger("Croniq.Api.WorkTransport");
                transportLogger.LogInformation(
                    "Runner transport switched from {PreviousTransport} to polling (tenant {Tenant}, environment {Environment}, runner {RunnerId}).",
                    previousTransport,
                    IdentifierHashing.HashTenantId(tenantId) ?? string.Empty,
                    resolvedEnvironment,
                    runnerId);
            }

            var batchSize = request.BatchSize
                ?? (request.MaxInflight > 0 ? request.MaxInflight.Value : 1);
            if (batchSize <= 0 || batchSize > 250)
            {
                return Results.BadRequest(new { error = "invalid-batch-size", message = "BatchSize must be between 1 and 250." });
            }

            if (request.MaxInflight.HasValue && (request.MaxInflight.Value <= 0 || request.MaxInflight.Value > 250))
            {
                return Results.BadRequest(new { error = "invalid-max-inflight", message = "MaxInflight must be between 1 and 250." });
            }

            var waitForMs = request.WaitForMs.GetValueOrDefault(0);
            if (waitForMs < 0 || waitForMs > 30_000)
            {
                return Results.BadRequest(new { error = "invalid-wait", message = "WaitForMs must be between 0 and 30000." });
            }

            var deadlineUtc = waitForMs > 0
                ? DateTimeOffset.UtcNow.AddMilliseconds(waitForMs)
                : DateTimeOffset.UtcNow;

            IReadOnlyCollection<TriggerLease> leases = Array.Empty<TriggerLease>();
            while (true)
            {
                var acquire = new TriggerAcquireRequest(
                    scope,
                    runnerId,
                    DateTimeOffset.UtcNow,
                    batchSize,
                    request.AllowTestExecutions.GetValueOrDefault(false));
                leases = await jobStore.AcquireAsync(acquire, cancellationToken).ConfigureAwait(false);

                if (leases.Count > 0 || waitForMs <= 0)
                {
                    break;
                }

                var remaining = deadlineUtc - DateTimeOffset.UtcNow;
                if (remaining <= TimeSpan.Zero)
                {
                    break;
                }

                var delay = remaining < TimeSpan.FromMilliseconds(250)
                    ? remaining
                    : TimeSpan.FromMilliseconds(250);

                await Task.Delay(delay, cancellationToken).ConfigureAwait(false);
            }

            foreach (var lease in leases)
            {
                if (string.Equals(lease.ExecutionMode, ExecutionIntent.ExecutionModes.Test, StringComparison.OrdinalIgnoreCase))
                {
                    ApiMetrics.RecordRunnerTestDecision(
                        scope.TenantId,
                        scope.EnvironmentTag,
                        "polling",
                        "accepted",
                        lease.ExecutionMode,
                        lease.InvocationSource);
                }
            }

            var payload = leases
                .Select(ToToken)
                .ToArray();

            var trackingLogger = loggerFactory.CreateLogger("Croniq.Api.WorkTracking");
            await TryTrackAssignmentsAsync(payload, scope, runnerId, workItemStore, trackingLogger, cancellationToken).ConfigureAwait(false);
            await TryStoreExecutionStartsAsync(payload, scope, runnerId, executionLogStore, httpContext, cancellationToken).ConfigureAwait(false);
            return Results.Ok(new WorkPollResponse(payload));
        })
        .WithDocs("Work_Poll", "Poll work", "Claims due trigger leases for execution (HTTP long-poll style).")
        .Produces<WorkPollResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkPollRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkPoll);

        app.MapPost("/tenants/{tenantId}/work/renew", async (
            string tenantId,
            string? environment,
            WorkRenewRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobStore jobStore,
            [FromServices] IWorkItemStore workItemStore,
            [FromServices] ILoggerFactory loggerFactory,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(request.EnvironmentTag ?? environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (string.IsNullOrWhiteSpace(request.RunnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var runnerId = request.RunnerId.Trim();
            var runnerFailure = EnsureRunnerIdentity(callerContextAccessor, runnerId);
            if (runnerFailure is not null)
            {
                return runnerFailure;
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var lease = FromToken(scope, request.Lease);
            var renew = new TriggerLeaseRenewRequest(lease, runnerId, DateTimeOffset.UtcNow);
            var updated = await jobStore.TryRenewLeaseAsync(renew, cancellationToken).ConfigureAwait(false);

            if (updated is null)
            {
                return Results.NotFound(new WorkRenewResponse(Renewed: false, Lease: null));
            }

            var token = ToToken(updated);
            var trackingLogger = loggerFactory.CreateLogger("Croniq.Api.WorkTracking");
            var renewal = new WorkLeaseRenewal(
                token.LeaseId,
                runnerId,
                token.LeaseExpiresAtUtc,
                DateTimeOffset.UtcNow,
                token.ExecutionId);
            await TryTrackRenewalAsync(renewal, workItemStore, trackingLogger, cancellationToken).ConfigureAwait(false);

            return Results.Ok(new WorkRenewResponse(Renewed: true, Lease: token));
        })
        .WithDocs("Work_Renew", "Renew work lease", "Renews an existing trigger lease for a running work item.")
        .Produces<WorkRenewResponse>(StatusCodes.Status200OK)
        .Produces<WorkRenewResponse>(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkRenewRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkRenew);

        app.MapPost("/tenants/{tenantId}/work/ack", async (
            string tenantId,
            string? environment,
            WorkAckRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobStore jobStore,
            [FromServices] IExecutionLogStore executionLogStore,
            [FromServices] IWorkItemStore workItemStore,
            [FromServices] ILoggerFactory loggerFactory,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(request.EnvironmentTag ?? environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (string.IsNullOrWhiteSpace(request.RunnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var runnerId = request.RunnerId.Trim();
            var runnerFailure = EnsureRunnerIdentity(callerContextAccessor, runnerId);
            if (runnerFailure is not null)
            {
                return runnerFailure;
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var lease = FromToken(scope, request.Lease);
            var reschedule = !request.Succeeded && request.NextFireTimeUtc.HasValue;
            var deadLetterReason = request.Succeeded || reschedule
                ? null
                : (string.IsNullOrWhiteSpace(request.DeadLetterReason) ? "work-failed" : request.DeadLetterReason);

            var release = new TriggerReleaseRequest(
                lease,
                runnerId,
                request.Succeeded,
                request.NextFireTimeUtc,
                deadLetterReason);

            try
            {
                await jobStore.ReleaseAsync(release, cancellationToken).ConfigureAwait(false);
                await TryStoreExecutionCompletionAsync(lease.ExecutionId, request.Succeeded, deadLetterReason, executionLogStore, cancellationToken).ConfigureAwait(false);
                var trackingLogger = loggerFactory.CreateLogger("Croniq.Api.WorkTracking");
                var completion = new WorkCompletion(
                    lease.LeaseId,
                    runnerId,
                    request.Succeeded,
                    DateTimeOffset.UtcNow,
                    deadLetterReason,
                    lease.ExecutionId);
                await TryTrackCompletionAsync(completion, workItemStore, trackingLogger, cancellationToken).ConfigureAwait(false);
                if (!request.Succeeded && string.Equals(deadLetterReason, WorkRejectionReasons.TestNotAllowed, StringComparison.OrdinalIgnoreCase))
                {
                    ApiMetrics.RecordRunnerTestDecision(
                        scope.TenantId,
                        scope.EnvironmentTag,
                        "polling",
                        "rejected",
                        lease.ExecutionMode,
                        lease.InvocationSource);
                    var warningLogger = loggerFactory.CreateLogger("Croniq.Api.WorkTestRejection");
                    await TryStoreTestRejectionAsync(lease, runnerId, executionLogStore, warningLogger, cancellationToken).ConfigureAwait(false);
                }
                return Results.NoContent();
            }
            catch (InvalidOperationException ex)
            {
                return Results.Conflict(new { error = "lease-conflict", message = ex.Message });
            }
        })
        .WithDocs("Work_Ack", "Acknowledge work result", "Acknowledges work completion and releases the trigger lease.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status409Conflict)
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkAckRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkAck);

        app.MapPost("/tenants/{tenantId}/work/{executionId}:events", async (
            string tenantId,
            string executionId,
            string? environment,
            WorkEventsRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobStore jobStore,
            [FromServices] IExecutionLogStore executionLogStore,
            [FromServices] IWorkItemStore workItemStore,
            [FromServices] ILoggerFactory loggerFactory,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(request.EnvironmentTag ?? environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (string.IsNullOrWhiteSpace(request.RunnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var runnerId = request.RunnerId.Trim();
            var runnerFailure = EnsureRunnerIdentity(callerContextAccessor, runnerId);
            if (runnerFailure is not null)
            {
                return runnerFailure;
            }

            if (request.Lease is null)
            {
                return Results.BadRequest(new { error = "lease-required", message = "Lease is required." });
            }

            if (request.Events is null || request.Events.Length == 0)
            {
                return Results.NoContent();
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var lease = FromToken(scope, request.Lease);
            lease = EnsureExecutionId(lease);

            if (!string.Equals(executionId, lease.ExecutionId, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "execution-mismatch", message = "ExecutionId does not match the lease." });
            }

            var renewRequest = new TriggerLeaseRenewRequest(lease, runnerId, DateTimeOffset.UtcNow);
            var renewed = await jobStore.TryRenewLeaseAsync(renewRequest, cancellationToken).ConfigureAwait(false);
            if (renewed is null)
            {
                return Results.Conflict(new { error = "lease-conflict", message = "Lease is not active for this runner." });
            }

            var trackingLogger = loggerFactory.CreateLogger("Croniq.Api.WorkTracking");
            var renewal = new WorkLeaseRenewal(
                renewed.LeaseId,
                runnerId,
                renewed.LeaseExpiresAtUtc,
                DateTimeOffset.UtcNow,
                renewed.ExecutionId);
            await TryTrackRenewalAsync(renewal, workItemStore, trackingLogger, cancellationToken).ConfigureAwait(false);

            var logger = loggerFactory.CreateLogger("Croniq.Api.WorkEvents");
            var baseScope = BuildWorkEventScope(lease, runnerId, resolvedEnvironment, scope.TenantId);
            var leaseExecutionId = lease.ExecutionId;
            var correlationId = ResolveCorrelationId(httpContext);
            List<ExecutionLogEntry>? entries = null;
            foreach (var entry in request.Events)
            {
                if (entry is null || string.IsNullOrWhiteSpace(entry.Message))
                {
                    continue;
                }

                var scopeValues = MergeEventScope(baseScope, entry);
                scopeValues["croniq.execution_log.skip"] = true;
                using var scopeHandle = logger.BeginScope(scopeValues);
                var level = ParseLogLevel(entry.Level);
                logger.Log(level, "{WorkerEvent}", entry.Message);

                if (!string.IsNullOrWhiteSpace(leaseExecutionId))
                {
                    entries ??= new List<ExecutionLogEntry>(request.Events.Length);
                    var properties = new Dictionary<string, object?>(scopeValues, StringComparer.OrdinalIgnoreCase);
                    properties.Remove("croniq.execution_log.skip");
                    properties["category"] = "Croniq.Api.WorkEvents";
                    properties["eventId"] = 0;

                    if (!string.IsNullOrWhiteSpace(correlationId))
                    {
                        properties["croniq.correlation_id"] = correlationId;
                    }

                    entries.Add(new ExecutionLogEntry(
                        leaseExecutionId,
                        entry.TimestampUtc ?? DateTimeOffset.UtcNow,
                        level,
                        entry.Message,
                        entry.Message,
                        Exception: null,
                        properties,
                        Activity.Current?.TraceId.ToString(),
                        Activity.Current?.SpanId.ToString(),
                        correlationId,
                        DateTimeOffset.UtcNow.UtcTicks));
                }
            }

            if (entries is { Count: > 0 })
            {
                try
                {
                    await executionLogStore.AppendAsync(leaseExecutionId!, entries, cancellationToken).ConfigureAwait(false);
                }
                catch
                {
                    // best-effort: work events should not fail if logging storage is unavailable
                }
            }

            return Results.NoContent();
        })
        .WithDocs("Work_Events", "Push work events", "Pushes execution-scoped log/events for a running work item.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status409Conflict)
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkEventsRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkEvents);
    }

    private static WorkLeaseToken ToToken(TriggerLease lease)
    {
        var normalized = EnsureExecutionId(lease);
        return new WorkLeaseToken(
            normalized.ExecutionId ?? string.Empty,
            normalized.LeaseId,
            normalized.TriggerId,
            normalized.JobKey,
            normalized.FireAtUtc,
            normalized.LeaseExpiresAtUtc,
            normalized.Payload,
            normalized.ExecutionMode,
            normalized.InvocationSource);
    }

    private static TriggerLease FromToken(PartitionScope scope, WorkLeaseToken token)
    {
        if (token is null)
        {
            throw new ArgumentNullException(nameof(token));
        }

        return new TriggerLease(
            token.LeaseId,
            token.TriggerId,
            token.JobKey,
            scope,
            token.FireAtUtc,
            token.LeaseExpiresAtUtc,
            token.Payload,
            token.ExecutionId,
            token.ExecutionMode,
            token.InvocationSource);
    }

    private static TriggerLease EnsureExecutionId(TriggerLease lease)
    {
        if (!string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return lease;
        }

        return lease with { ExecutionId = Guid.NewGuid().ToString("N") };
    }

    private static async Task TryStoreExecutionStartsAsync(
        IReadOnlyCollection<WorkLeaseToken> leases,
        PartitionScope scope,
        string runnerId,
        IExecutionLogStore executionLogStore,
        HttpContext httpContext,
        CancellationToken cancellationToken)
    {
        if (leases.Count == 0)
        {
            return;
        }

        var nowUtc = DateTimeOffset.UtcNow;
        var activity = Activity.Current;
        var correlationId = ResolveCorrelationId(httpContext);

        foreach (var lease in leases)
        {
            if (string.IsNullOrWhiteSpace(lease.ExecutionId))
            {
                continue;
            }

            var record = new ExecutionRecord(
                lease.ExecutionId,
                ExecutionKind.Job,
                null,
                lease.JobKey,
                scope.TenantId,
                scope.EnvironmentTag,
                lease.TriggerId,
                lease.FireAtUtc,
                nowUtc,
                runnerId,
                activity?.TraceId.ToString(),
                activity?.SpanId.ToString(),
                correlationId,
                lease.ExecutionMode,
                lease.InvocationSource);

            try
            {
                await executionLogStore.OnExecutionStartedAsync(record, cancellationToken).ConfigureAwait(false);
            }
            catch
            {
                // best-effort: work leases should not fail if logging storage is unavailable
            }
        }
    }

    private static async Task TryStoreExecutionCompletionAsync(
        string? executionId,
        bool succeeded,
        string? errorMessage,
        IExecutionLogStore executionLogStore,
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
            ErrorMessage: succeeded ? null : errorMessage);

        try
        {
            await executionLogStore.OnExecutionCompletedAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            // best-effort: work ack should not fail if logging storage is unavailable
        }
    }

    private static async Task TryTrackAssignmentsAsync(
        IReadOnlyCollection<WorkLeaseToken> leases,
        PartitionScope scope,
        string runnerId,
        IWorkItemStore workItemStore,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (leases.Count == 0)
        {
            return;
        }

        var assignedAtUtc = DateTimeOffset.UtcNow;
        foreach (var lease in leases)
        {
            if (string.IsNullOrWhiteSpace(lease.ExecutionId))
            {
                continue;
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
                AssignedAtUtc: assignedAtUtc,
                ExecutionMode: lease.ExecutionMode,
                InvocationSource: lease.InvocationSource);

            try
            {
                await workItemStore.UpsertAssignmentAsync(assignment, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                logger.LogWarning(ex, "Failed to track work assignment {ExecutionId}", lease.ExecutionId);
            }
        }
    }

    private static async Task TryTrackRenewalAsync(
        WorkLeaseRenewal renewal,
        IWorkItemStore workItemStore,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        try
        {
            await workItemStore.TryRenewAsync(renewal, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to track work lease renewal {LeaseId}", renewal.LeaseId);
        }
    }

    private static async Task TryTrackCompletionAsync(
        WorkCompletion completion,
        IWorkItemStore workItemStore,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        try
        {
            await workItemStore.TryCompleteAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to track work completion {LeaseId}", completion.LeaseId);
        }
    }

    private static async Task TryStoreTestRejectionAsync(
        TriggerLease lease,
        string runnerId,
        IExecutionLogStore executionLogStore,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(lease.ExecutionId))
        {
            return;
        }

        logger.LogWarning(
            "Runner rejected test execution {ExecutionId} (runner {RunnerId}, job {JobKey}, trigger {TriggerId}).",
            lease.ExecutionId,
            runnerId,
            lease.JobKey,
            lease.TriggerId);

        var properties = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase)
        {
            ["croniq.execution_id"] = lease.ExecutionId,
            ["croniq.runner_id"] = runnerId,
            ["croniq.job.key"] = lease.JobKey,
            ["croniq.trigger.id"] = lease.TriggerId,
            ["croniq.execution_mode"] = lease.ExecutionMode,
            ["croniq.invocation_source"] = lease.InvocationSource,
            ["croniq.warning.type"] = WorkRejectionReasons.TestNotAllowed
        };

        var entry = new ExecutionLogEntry(
            lease.ExecutionId,
            DateTimeOffset.UtcNow,
            LogLevel.Warning,
            "Runner rejected test execution",
            "Runner rejected test execution",
            Exception: null,
            properties,
            Activity.Current?.TraceId.ToString(),
            Activity.Current?.SpanId.ToString(),
            CorrelationId: null,
            DateTimeOffset.UtcNow.UtcTicks);

        try
        {
            await executionLogStore.AppendAsync(lease.ExecutionId, new[] { entry }, cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            // best-effort warning log
        }
    }

    private static Dictionary<string, object?> BuildWorkEventScope(TriggerLease lease, string runnerId, string environmentTag, string tenantId)
    {
        return new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase)
        {
            ["croniq.execution_id"] = lease.ExecutionId,
            ["croniq.job.key"] = lease.JobKey,
            ["croniq.trigger.id"] = lease.TriggerId,
            ["croniq.tenant_id"] = IdentifierHashing.HashTenantId(tenantId) ?? string.Empty,
            ["croniq.environment"] = environmentTag,
            ["croniq.runner_id"] = runnerId
        };
    }

    private static Dictionary<string, object?> MergeEventScope(
        Dictionary<string, object?> baseScope,
        WorkEventEntry entry)
    {
        var scope = new Dictionary<string, object?>(baseScope, StringComparer.OrdinalIgnoreCase);

        if (!string.IsNullOrWhiteSpace(entry.EventType))
        {
            scope["croniq.event.type"] = entry.EventType;
        }

        if (entry.TimestampUtc.HasValue)
        {
            scope["event.timestamp_utc"] = entry.TimestampUtc.Value;
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
}
