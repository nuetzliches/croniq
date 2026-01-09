using System;
using System.Linq;
using System.Threading;
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
    private static void MapWorkerEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/tenants/{tenantId}/workers/heartbeat", async (
            string tenantId,
            string? environment,
            WorkerHeartbeatRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWorkerStore workerStore,
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

            if (string.IsNullOrWhiteSpace(request.InstanceId))
            {
                return Results.BadRequest(new { error = "instance-required", message = "InstanceId is required." });
            }

            var instanceId = request.InstanceId.Trim();
            var identityFailure = EnsureWorkerIdentity(callerContextAccessor, instanceId);
            if (identityFailure is not null)
            {
                return identityFailure;
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var seenAtUtc = request.SeenAtUtc ?? DateTimeOffset.UtcNow;
            var heartbeat = new WorkerHeartbeat(scope, instanceId, seenAtUtc, request.MetadataJson);
            await workerStore.UpsertHeartbeatAsync(heartbeat, cancellationToken).ConfigureAwait(false);

            return Results.NoContent();
        })
        .WithDocs("Workers_Heartbeat", "Worker heartbeat", "Records a worker host heartbeat to track availability.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScopeFromBodyOrQuery<WorkerHeartbeatRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.WorkersHeartbeat);

        app.MapGet("/tenants/{tenantId}/workers", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWorkerStore workerStore,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var query = new WorkerQuery(scope, DateTimeOffset.UtcNow);
            var workers = await workerStore.ListAsync(query, cancellationToken).ConfigureAwait(false);

            var payload = workers
                .Select(w => new WorkerStatusModel(w.InstanceId, w.LastSeenAtUtc, w.ExpiresAtUtc, w.IsOnline, w.MetadataJson))
                .ToArray();

            return Results.Ok(new WorkerListResponse(payload));
        })
        .WithDocs("Workers_List", "List workers", "Lists active worker hosts for the tenant/environment.")
        .Produces<WorkerListResponse>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(requireEnvironment: true, CroniqScopes.WorkersRead);
    }
}
