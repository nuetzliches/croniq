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
    private static void MapRunnerEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/tenants/{tenantId}/runners/heartbeat", async (
            string tenantId,
            string? environment,
            RunnerHeartbeatRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IRunnerStore runnerStore,
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
            var nowUtc = DateTimeOffset.UtcNow;
            var seenAtUtc = request.SeenAtUtc ?? nowUtc;
            var runnerInstanceId = RunnerInstanceGuard.ResolveRunnerInstanceId(request.RunnerInstanceId, request.MetadataJson);
            var metadataUpdates = RunnerInstanceGuard.BuildMetadataUpdates(
                runnerInstanceId,
                transportState: null,
                allowTestExecutions: null,
                maxInflight: null,
                capabilities: null);
            var (runnerConflict, _) = await RunnerInstanceGuard.EnsureRunnerInstanceAvailableAsync(
                runnerStore,
                scope,
                runnerId,
                runnerInstanceId,
                request.MetadataJson,
                metadataUpdates,
                nowUtc,
                seenAtUtc,
                cancellationToken).ConfigureAwait(false);
            if (runnerConflict is not null)
            {
                return runnerConflict;
            }

            return Results.NoContent();
        })
        .WithDocs("Runners_Heartbeat", "Runner heartbeat", "Records a runner heartbeat to track availability.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScopeFromBodyOrQuery<RunnerHeartbeatRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.RunnersHeartbeat);

        app.MapGet("/tenants/{tenantId}/runners", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IRunnerStore runnerStore,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var query = new RunnerQuery(scope, DateTimeOffset.UtcNow);
            var runners = await runnerStore.ListAsync(query, cancellationToken).ConfigureAwait(false);

            var payload = runners
                .Select(r => new RunnerStatusModel(r.RunnerId, r.LastSeenAtUtc, r.ExpiresAtUtc, r.IsOnline, r.MetadataJson))
                .ToArray();

            return Results.Ok(new RunnerListResponse(payload));
        })
        .WithDocs("Runners_List", "List runners", "Lists active runners for the tenant/environment.")
        .Produces<RunnerListResponse>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(requireEnvironment: true, CroniqScopes.RunnersRead);
    }
}
