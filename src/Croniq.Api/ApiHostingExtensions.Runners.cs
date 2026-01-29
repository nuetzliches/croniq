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
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static readonly JsonSerializerOptions RunnerPresenceStreamJsonOptions = new(JsonSerializerDefaults.Web);
    private static readonly TimeSpan RunnerPresenceStreamPollInterval = TimeSpan.FromSeconds(10);

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

        app.MapGet("/tenants/{tenantId}/runners/stream", async (
            string tenantId,
            string? environment,
            bool? includeOffline,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IRunnerStore runnerStore,
            [FromServices] ILogger<RunnerPresenceApiMarker> logger,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                await MissingEnvironment().ExecuteAsync(httpContext);
                return;
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var include = includeOffline.GetValueOrDefault();
            var previous = new Dictionary<string, RunnerPresenceSnapshot>(StringComparer.OrdinalIgnoreCase);
            var isFirst = true;

            httpContext.Response.StatusCode = StatusCodes.Status200OK;
            httpContext.Response.ContentType = "text/event-stream";
            httpContext.Response.Headers["Cache-Control"] = "no-cache";
            httpContext.Response.Headers.Append("X-Accel-Buffering", "no");

            try
            {
                while (!cancellationToken.IsCancellationRequested)
                {
                    var pollStartedAt = DateTimeOffset.UtcNow;
                    IReadOnlyCollection<RunnerStatus> runners;
                    try
                    {
                        runners = await runnerStore
                            .ListAsync(new RunnerQuery(scope, pollStartedAt, include), cancellationToken)
                            .ConfigureAwait(false);
                    }
                    catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
                    {
                        break;
                    }
                    catch (Exception ex)
                    {
                        logger.LogWarning(
                            ex,
                            "Runner presence stream poll failed for {TenantId}/{EnvironmentTag}",
                            scope.TenantId,
                            scope.EnvironmentTag);
                        await WriteSseCommentAsync(httpContext.Response, "upstream-error", cancellationToken).ConfigureAwait(false);
                        await Task.Delay(RunnerPresenceStreamPollInterval, cancellationToken).ConfigureAwait(false);
                        continue;
                    }

                    var current = runners
                        .Select(RunnerPresenceSnapshot.FromStatus)
                        .ToDictionary(snapshot => snapshot.RunnerId, StringComparer.OrdinalIgnoreCase);

                    var totalCount = current.Count;
                    var onlineCount = totalCount == 0 ? 0 : current.Values.Count(runner => runner.IsOnline);
                    var latestSeenAtUtc = totalCount == 0
                        ? (DateTimeOffset?)null
                        : current.Values.Max(runner => runner.LastSeenAtUtc);

                    RunnerPresenceStreamEvent payload;
                    if (isFirst)
                    {
                        var snapshot = current.Values.Select(ToRunnerStatusModel).ToArray();
                        payload = new RunnerPresenceStreamEvent(
                            "presence.snapshot",
                            pollStartedAt,
                            latestSeenAtUtc,
                            onlineCount,
                            totalCount,
                            Snapshot: snapshot);
                    }
                    else
                    {
                        var updated = current.Values
                            .Where(entry => !previous.TryGetValue(entry.RunnerId, out var prior) || !entry.Equals(prior))
                            .Select(ToRunnerStatusModel)
                            .ToArray();
                        var removed = previous.Keys
                            .Where(key => !current.ContainsKey(key))
                            .ToArray();

                        payload = new RunnerPresenceStreamEvent(
                            "presence.delta",
                            pollStartedAt,
                            latestSeenAtUtc,
                            onlineCount,
                            totalCount,
                            Updated: updated.Length > 0 ? updated : null,
                            RemovedRunnerIds: removed.Length > 0 ? removed : null);
                    }

                    var json = JsonSerializer.Serialize(payload, RunnerPresenceStreamJsonOptions);
                    await WriteSseDataAsync(httpContext.Response, json, cancellationToken).ConfigureAwait(false);

                    previous = current;
                    isFirst = false;

                    await Task.Delay(RunnerPresenceStreamPollInterval, cancellationToken).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                // ignore cancellation
            }
        })
        .WithDocs("Runners_Stream", "Stream runners", "Server-sent events stream for runner presence updates.")
        .Produces(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
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

    private sealed record RunnerPresenceSnapshot(
        string RunnerId,
        DateTimeOffset LastSeenAtUtc,
        DateTimeOffset ExpiresAtUtc,
        bool IsOnline,
        string? MetadataJson)
    {
        public static RunnerPresenceSnapshot FromStatus(RunnerStatus status) =>
            new(status.RunnerId, status.LastSeenAtUtc, status.ExpiresAtUtc, status.IsOnline, status.MetadataJson);
    }

    private static RunnerStatusModel ToRunnerStatusModel(RunnerPresenceSnapshot snapshot)
    {
        return new RunnerStatusModel(
            snapshot.RunnerId,
            snapshot.LastSeenAtUtc,
            snapshot.ExpiresAtUtc,
            snapshot.IsOnline,
            snapshot.MetadataJson);
    }
}
