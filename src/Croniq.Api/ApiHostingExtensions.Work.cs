using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

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
            [FromServices] IJobStore jobStore,
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

            var batchSize = request.BatchSize.GetValueOrDefault(1);
            if (batchSize <= 0 || batchSize > 250)
            {
                return Results.BadRequest(new { error = "invalid-batch-size", message = "BatchSize must be between 1 and 250." });
            }

            var waitForMs = request.WaitForMs.GetValueOrDefault(0);
            if (waitForMs < 0 || waitForMs > 30_000)
            {
                return Results.BadRequest(new { error = "invalid-wait", message = "WaitForMs must be between 0 and 30000." });
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var runnerId = request.RunnerId.Trim();
            var deadlineUtc = waitForMs > 0
                ? DateTimeOffset.UtcNow.AddMilliseconds(waitForMs)
                : DateTimeOffset.UtcNow;

            IReadOnlyCollection<TriggerLease> leases = Array.Empty<TriggerLease>();
            while (true)
            {
                var acquire = new TriggerAcquireRequest(scope, runnerId, DateTimeOffset.UtcNow, batchSize);
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

            var payload = leases
                .Select(ToToken)
                .ToArray();

            return Results.Ok(new WorkPollResponse(payload));
        })
        .WithDocs("Work_Poll", "Poll work", "Claims due trigger leases for execution (HTTP long-poll style).")
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkPollRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkExecute);

        app.MapPost("/tenants/{tenantId}/work/renew", async (
            string tenantId,
            string? environment,
            WorkRenewRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobStore jobStore,
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

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var lease = FromToken(scope, request.Lease);
            var renew = new TriggerLeaseRenewRequest(lease, request.RunnerId.Trim(), DateTimeOffset.UtcNow);
            var updated = await jobStore.TryRenewLeaseAsync(renew, cancellationToken).ConfigureAwait(false);

            return updated is null
                ? Results.NotFound(new WorkRenewResponse(Renewed: false, Lease: null))
                : Results.Ok(new WorkRenewResponse(Renewed: true, Lease: ToToken(updated)));
        })
        .WithDocs("Work_Renew", "Renew work lease", "Renews an existing trigger lease for a running work item.")
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkRenewRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkExecute);

        app.MapPost("/tenants/{tenantId}/work/ack", async (
            string tenantId,
            string? environment,
            WorkAckRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobStore jobStore,
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

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var lease = FromToken(scope, request.Lease);
            var deadLetterReason = request.Succeeded ? null : (string.IsNullOrWhiteSpace(request.DeadLetterReason) ? "work-failed" : request.DeadLetterReason);

            var release = new TriggerReleaseRequest(
                lease,
                request.Succeeded,
                request.NextFireTimeUtc,
                deadLetterReason);

            await jobStore.ReleaseAsync(release, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Work_Ack", "Acknowledge work result", "Acknowledges work completion and releases the trigger lease.")
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkAckRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkExecute);
    }

    private static WorkLeaseToken ToToken(TriggerLease lease)
        => new(
            lease.LeaseId,
            lease.TriggerId,
            lease.JobKey,
            lease.FireAtUtc,
            lease.LeaseExpiresAtUtc,
            lease.Payload);

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
            token.Payload);
    }
}
