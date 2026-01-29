using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
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
            [FromServices] Microsoft.Extensions.Options.IOptions<RunnerStoreOptions> runnerStoreOptions,
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
            if (IsDisconnectMetadata(request.MetadataJson))
            {
                var options = runnerStoreOptions?.Value ?? new RunnerStoreOptions();
                options.Normalize();
                var ttl = options.OnlineTtl;
                if (ttl <= TimeSpan.Zero)
                {
                    ttl = TimeSpan.FromSeconds(60);
                }

                seenAtUtc = nowUtc - ttl - TimeSpan.FromSeconds(1);
            }
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
            bool? includeOffline,
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
            var query = new RunnerQuery(scope, DateTimeOffset.UtcNow, includeOffline.GetValueOrDefault());
            var runners = await runnerStore.ListAsync(query, cancellationToken).ConfigureAwait(false);

            var payload = runners
                .Select(r => new RunnerStatusModel(r.RunnerId, r.LastSeenAtUtc, r.ExpiresAtUtc, r.IsOnline, r.MetadataJson))
                .ToArray();

            return Results.Ok(new RunnerListResponse(payload));
        })
        .WithDocs("Runners_List", "List runners", "Lists runners for the tenant/environment (use includeOffline=true to return offline runners within retention).")
        .Produces<RunnerListResponse>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(requireEnvironment: true, CroniqScopes.RunnersRead);

        app.MapPost("/tenants/{tenantId}/runners/{runnerId}:drain", async (
            string tenantId,
            string runnerId,
            string? environment,
            RunnerDrainRequest request,
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

            if (string.IsNullOrWhiteSpace(runnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var normalizedRunnerId = runnerId.Trim();
            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var nowUtc = DateTimeOffset.UtcNow;
            var existing = await runnerStore.TryGetAsync(
                    new RunnerLookup(scope, normalizedRunnerId, nowUtc),
                    cancellationToken)
                .ConfigureAwait(false);
            if (existing is null)
            {
                return Results.NotFound();
            }

            var draining = request.Draining ?? true;
            var metadata = RunnerInstanceGuard.MergeMetadataJson(existing.MetadataJson, new Dictionary<string, object?>
            {
                ["draining"] = draining
            });

            await runnerStore.UpsertHeartbeatAsync(
                    new RunnerHeartbeat(scope, normalizedRunnerId, existing.LastSeenAtUtc, metadata),
                    cancellationToken)
                .ConfigureAwait(false);

            return Results.NoContent();
        })
        .WithDocs("Runners_Drain", "Drain runner", "Marks a runner as draining by updating its heartbeat metadata.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScopeFromBodyOrQuery<RunnerDrainRequest>(
            r => r.EnvironmentTag,
            requireEnvironment: true,
            CroniqScopes.RunnersWrite);

        app.MapDelete("/tenants/{tenantId}/runners/{runnerId}", async (
            string tenantId,
            string runnerId,
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

            if (string.IsNullOrWhiteSpace(runnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var nowUtc = DateTimeOffset.UtcNow;
            var removed = await runnerStore.DeleteAsync(
                    new RunnerLookup(scope, runnerId.Trim(), nowUtc),
                    cancellationToken)
                .ConfigureAwait(false);

            return removed ? Results.NoContent() : Results.NotFound();
        })
        .WithDocs("Runners_Deregister", "Deregister runner", "Removes the runner presence record for the tenant/environment.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScope(requireEnvironment: true, CroniqScopes.RunnersWrite);
    }

    private static bool IsDisconnectMetadata(string? metadataJson)
    {
        if (string.IsNullOrWhiteSpace(metadataJson))
        {
            return false;
        }

        try
        {
            using var doc = JsonDocument.Parse(metadataJson);
            if (doc.RootElement.ValueKind != JsonValueKind.Object)
            {
                return false;
            }

            foreach (var property in doc.RootElement.EnumerateObject())
            {
                if (string.Equals(property.Name, "disconnectedAtUtc", StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }

                if (!string.Equals(property.Name, "transportState", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                if (property.Value.ValueKind == JsonValueKind.String
                    && string.Equals(property.Value.GetString(), "disconnected", StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }
        }
        catch (JsonException)
        {
            return false;
        }

        return false;
    }
}
