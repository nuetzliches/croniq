using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Grpc.Core;
using Microsoft.AspNetCore.Http;

namespace Croniq.Api;

internal static class RunnerInstanceGuard
{
    internal const string RunnerIdInUseError = "runner-id-in-use";

    private static readonly JsonSerializerOptions MetadataJsonOptions = new()
    {
        PropertyNameCaseInsensitive = true
    };

    internal static string? ResolveRunnerInstanceId(string? runnerInstanceId, string? metadataJson)
    {
        if (!string.IsNullOrWhiteSpace(runnerInstanceId))
        {
            return runnerInstanceId.Trim();
        }

        return TryExtractRunnerInstanceId(metadataJson);
    }

    internal static Dictionary<string, object?> BuildMetadataUpdates(
        string? runnerInstanceId,
        string? transportState,
        bool? allowTestExecutions,
        int? maxInflight,
        IReadOnlyCollection<string>? capabilities)
    {
        var updates = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);

        if (!string.IsNullOrWhiteSpace(runnerInstanceId))
        {
            updates["runnerInstanceId"] = runnerInstanceId.Trim();
        }

        if (!string.IsNullOrWhiteSpace(transportState))
        {
            updates["transportState"] = transportState.Trim();
        }

        if (allowTestExecutions.HasValue)
        {
            updates["allowTestExecutions"] = allowTestExecutions.Value;
        }

        if (maxInflight.HasValue && maxInflight.Value > 0)
        {
            updates["maxInflight"] = maxInflight.Value;
        }

        if (capabilities is { Count: > 0 })
        {
            updates["capabilities"] = capabilities;
        }

        return updates;
    }

    internal static async Task<(IResult? Result, string? MetadataJson)> EnsureRunnerInstanceAvailableAsync(
        IRunnerStore runnerStore,
        PartitionScope scope,
        string runnerId,
        string? runnerInstanceId,
        string? metadataJson,
        IReadOnlyDictionary<string, object?>? metadataUpdates,
        DateTimeOffset nowUtc,
        DateTimeOffset seenAtUtc,
        CancellationToken cancellationToken)
    {
        if (runnerStore is null) throw new ArgumentNullException(nameof(runnerStore));

        var normalizedRunnerId = runnerId?.Trim();
        if (string.IsNullOrWhiteSpace(normalizedRunnerId))
        {
            return (Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." }), metadataJson);
        }

        var resolvedInstanceId = ResolveRunnerInstanceId(runnerInstanceId, metadataJson);
        var existing = await runnerStore.TryGetAsync(new RunnerLookup(scope, normalizedRunnerId, nowUtc), cancellationToken)
            .ConfigureAwait(false);
        if (existing is not null)
        {
            var existingInstanceId = TryExtractRunnerInstanceId(existing.MetadataJson);
            if (!string.IsNullOrWhiteSpace(existingInstanceId)
                && !string.IsNullOrWhiteSpace(resolvedInstanceId)
                && !string.Equals(existingInstanceId, resolvedInstanceId, StringComparison.OrdinalIgnoreCase))
            {
                return (Results.Problem(
                    statusCode: StatusCodes.Status409Conflict,
                    title: RunnerIdInUseError,
                    detail: "RunnerId is already in use by another active runner instance."), metadataJson);
            }
        }

        var mergedMetadata = MergeMetadataJson(metadataJson ?? existing?.MetadataJson, metadataUpdates);
        var heartbeat = new RunnerHeartbeat(scope, normalizedRunnerId, seenAtUtc, mergedMetadata);
        await runnerStore.UpsertHeartbeatAsync(heartbeat, cancellationToken).ConfigureAwait(false);
        return (null, mergedMetadata);
    }

    internal static async Task<string?> EnsureRunnerInstanceAvailableOrThrowAsync(
        IRunnerStore runnerStore,
        PartitionScope scope,
        string runnerId,
        string? runnerInstanceId,
        string? metadataJson,
        IReadOnlyDictionary<string, object?>? metadataUpdates,
        DateTimeOffset nowUtc,
        DateTimeOffset seenAtUtc,
        CancellationToken cancellationToken)
    {
        if (runnerStore is null) throw new ArgumentNullException(nameof(runnerStore));

        var normalizedRunnerId = runnerId?.Trim();
        if (string.IsNullOrWhiteSpace(normalizedRunnerId))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "runner_id is required."));
        }

        var resolvedInstanceId = ResolveRunnerInstanceId(runnerInstanceId, metadataJson);
        var existing = await runnerStore.TryGetAsync(new RunnerLookup(scope, normalizedRunnerId, nowUtc), cancellationToken)
            .ConfigureAwait(false);
        if (existing is not null)
        {
            var existingInstanceId = TryExtractRunnerInstanceId(existing.MetadataJson);
            if (!string.IsNullOrWhiteSpace(existingInstanceId)
                && !string.IsNullOrWhiteSpace(resolvedInstanceId)
                && !string.Equals(existingInstanceId, resolvedInstanceId, StringComparison.OrdinalIgnoreCase))
            {
                throw new RpcException(new Status(StatusCode.AlreadyExists, RunnerIdInUseError));
            }
        }

        var mergedMetadata = MergeMetadataJson(metadataJson ?? existing?.MetadataJson, metadataUpdates);
        var heartbeat = new RunnerHeartbeat(scope, normalizedRunnerId, seenAtUtc, mergedMetadata);
        await runnerStore.UpsertHeartbeatAsync(heartbeat, cancellationToken).ConfigureAwait(false);
        return mergedMetadata;
    }

    internal static string? TryExtractRunnerInstanceId(string? metadataJson)
    {
        if (string.IsNullOrWhiteSpace(metadataJson))
        {
            return null;
        }

        try
        {
            using var document = JsonDocument.Parse(metadataJson);
            if (document.RootElement.ValueKind != JsonValueKind.Object)
            {
                return null;
            }

            foreach (var property in document.RootElement.EnumerateObject())
            {
                if (!string.Equals(property.Name, "runnerInstanceId", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                return property.Value.ValueKind == JsonValueKind.String
                    ? property.Value.GetString()
                    : null;
            }
        }
        catch (JsonException)
        {
            return null;
        }

        return null;
    }

    internal static string? MergeMetadataJson(string? metadataJson, IReadOnlyDictionary<string, object?>? updates)
    {
        if (updates is null || updates.Count == 0)
        {
            return metadataJson;
        }

        var merged = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);

        if (!string.IsNullOrWhiteSpace(metadataJson))
        {
            try
            {
                var payload = JsonSerializer.Deserialize<Dictionary<string, object?>>(metadataJson, MetadataJsonOptions);
                if (payload is not null)
                {
                    foreach (var pair in payload)
                    {
                        merged[pair.Key] = pair.Value;
                    }
                }
            }
            catch (JsonException)
            {
                // ignore invalid metadata json
            }
        }

        foreach (var pair in updates)
        {
            merged[pair.Key] = pair.Value;
        }

        return JsonSerializer.Serialize(merged);
    }
}
