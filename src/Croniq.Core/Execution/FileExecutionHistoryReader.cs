using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Execution;

/// <summary>
/// Reads execution summaries from filesystem-backed NDJSON logs.
/// </summary>
public sealed class FileExecutionHistoryReader : IExecutionHistoryReader
{
    private readonly FileExecutionLogStoreOptions _options;

    public FileExecutionHistoryReader(FileExecutionLogStoreOptions options)
    {
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public async Task<IReadOnlyList<ExecutionSummary>> ListExecutionsAsync(PartitionScope scope, ExecutionHistoryQuery? query, CancellationToken cancellationToken)
    {
        var normalized = (query ?? new ExecutionHistoryQuery()).Normalize();
        var root = FileExecutionLogPathHelper.GetScopeRoot(_options, scope.TenantId, scope.EnvironmentTag);
        if (!Directory.Exists(root))
        {
            return Array.Empty<ExecutionSummary>();
        }

        var results = new List<ExecutionSummary>();
        foreach (var file in EnumerateFiles(root))
        {
            cancellationToken.ThrowIfCancellationRequested();
            ExecutionSummary? summary = null;
            try
            {
                summary = await TryParseSummaryAsync(file.FullName, cancellationToken).ConfigureAwait(false);
            }
            catch
            {
                // Ignore malformed files; they might be truncated while being written.
            }

            if (summary is null)
            {
                continue;
            }

            if (!string.Equals(summary.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(summary.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            if (!MatchesQuery(summary, normalized))
            {
                continue;
            }

            results.Add(summary);
            if (results.Count >= normalized.Limit)
            {
                break;
            }
        }

        return results;
    }

    public async Task<ExecutionSummary?> GetExecutionAsync(string executionId, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(executionId))
        {
            return null;
        }

        var basePath = FileExecutionLogPathHelper.ResolveBasePath(_options);
        if (!Directory.Exists(basePath))
        {
            return null;
        }

        var filePath = Directory.EnumerateFiles(basePath, $"{executionId}.ndjson", SearchOption.AllDirectories).FirstOrDefault();
        if (filePath is null)
        {
            return null;
        }

        try
        {
            return await TryParseSummaryAsync(filePath, cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            return null;
        }
    }

    private static IEnumerable<FileInfo> EnumerateFiles(string root)
    {
        return Directory
            .EnumerateFiles(root, "*.ndjson", SearchOption.AllDirectories)
            .Select(path => new FileInfo(path))
            .OrderByDescending(info => info.LastWriteTimeUtc);
    }

    private static bool MatchesQuery(ExecutionSummary summary, ExecutionHistoryQuery query)
    {
        if (query.JobKey is { Length: > 0 } && !string.Equals(summary.JobKey, query.JobKey, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (query.Status.HasValue)
        {
            if (!summary.Status.HasValue || summary.Status.Value != query.Status.Value)
            {
                return false;
            }
        }

        if (query.StartedAfterUtc.HasValue && summary.StartedAtUtc < query.StartedAfterUtc.Value)
        {
            return false;
        }

        if (query.StartedBeforeUtc.HasValue && summary.StartedAtUtc > query.StartedBeforeUtc.Value)
        {
            return false;
        }

        return true;
    }

    private static async Task<ExecutionSummary?> TryParseSummaryAsync(string filePath, CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(filePath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        using var reader = new StreamReader(stream);

        var startLine = await reader.ReadLineAsync().ConfigureAwait(false);
        if (startLine is null)
        {
            return null;
        }

        if (!TryParseStart(startLine, out var start))
        {
            return null;
        }

        CompletionSnapshot? completion = null;
        while (!reader.EndOfStream && !cancellationToken.IsCancellationRequested)
        {
            var line = await reader.ReadLineAsync().ConfigureAwait(false);
            if (line is null)
            {
                continue;
            }

            if (TryParseCompletion(line, out var snapshot))
            {
                completion = snapshot;
            }
        }

        if (start.ExecutionId is null
            || start.JobKey is null
            || start.TenantId is null
            || start.EnvironmentTag is null
            || start.FireAtUtc is null
            || start.StartedAtUtc is null)
        {
            return null;
        }

        return new ExecutionSummary(
            start.ExecutionId,
            start.Kind,
            start.WorkflowId,
            start.JobKey,
            start.TenantId,
            start.EnvironmentTag,
            start.TriggerId,
            start.FireAtUtc.Value,
            start.StartedAtUtc.Value,
            completion?.CompletedAtUtc,
            completion?.Status,
            completion?.DurationMs,
            start.InstanceId,
            start.TraceId,
            start.CorrelationId,
            completion?.ErrorType,
            completion?.ErrorMessage);
    }

    private static bool TryParseStart(string line, out StartSnapshot snapshot)
    {
        snapshot = default;
        try
        {
            using var document = JsonDocument.Parse(line);
            var root = document.RootElement;
            if (!IsType(root, "start"))
            {
                return false;
            }

            snapshot = new StartSnapshot
            {
                ExecutionId = GetString(root, "executionId"),
                Kind = ParseEnum(root, "kind", ExecutionKind.Job),
                WorkflowId = GetString(root, "workflowId"),
                JobKey = GetString(root, "jobKey"),
                TenantId = GetString(root, "tenantId"),
                EnvironmentTag = GetString(root, "environmentTag"),
                TriggerId = GetString(root, "triggerId"),
                FireAtUtc = GetDateTime(root, "fireAtUtc"),
                StartedAtUtc = GetDateTime(root, "startedAtUtc"),
                InstanceId = GetString(root, "instanceId"),
                TraceId = GetString(root, "traceId"),
                CorrelationId = GetString(root, "correlationId")
            };

            return true;
        }
        catch
        {
            return false;
        }
    }

    private static bool TryParseCompletion(string line, out CompletionSnapshot snapshot)
    {
        snapshot = default;
        try
        {
            using var document = JsonDocument.Parse(line);
            var root = document.RootElement;
            if (!IsType(root, "completion"))
            {
                return false;
            }

            snapshot = new CompletionSnapshot
            {
                CompletedAtUtc = GetDateTime(root, "completedAtUtc"),
                Status = ParseEnum(root, "status", (ExecutionStatus?)null),
                DurationMs = GetNullableDouble(root, "durationMs"),
                ErrorType = GetString(root, "errorType"),
                ErrorMessage = GetString(root, "errorMessage")
            };

            return true;
        }
        catch
        {
            return false;
        }
    }

    private static bool IsType(JsonElement element, string expected)
    {
        if (!element.TryGetProperty("type", out var typeProperty))
        {
            return false;
        }

        var value = typeProperty.GetString();
        return string.Equals(value, expected, StringComparison.OrdinalIgnoreCase);
    }

    private static string? GetString(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var property))
        {
            return null;
        }

        if (property.ValueKind == JsonValueKind.Null)
        {
            return null;
        }

        return property.GetString();
    }

    private static DateTimeOffset? GetDateTime(JsonElement element, string propertyName)
    {
        var text = GetString(element, propertyName);
        if (string.IsNullOrWhiteSpace(text))
        {
            return null;
        }

        if (DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var timestamp))
        {
            return timestamp;
        }

        return null;
    }

    private static double? GetNullableDouble(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var property))
        {
            return null;
        }

        if (property.ValueKind == JsonValueKind.Number && property.TryGetDouble(out var value))
        {
            return value;
        }

        if (property.ValueKind == JsonValueKind.String && double.TryParse(property.GetString(), NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed))
        {
            return parsed;
        }

        return null;
    }

    private static ExecutionStatus? ParseEnum(JsonElement element, string propertyName, ExecutionStatus? defaultValue)
    {
        if (!element.TryGetProperty(propertyName, out var property) || property.ValueKind == JsonValueKind.Null)
        {
            return defaultValue;
        }

        if (property.ValueKind == JsonValueKind.Number)
        {
            if (property.TryGetInt32(out var numeric))
            {
                return (ExecutionStatus)numeric;
            }

            if (property.TryGetInt64(out var numeric64))
            {
                return (ExecutionStatus)numeric64;
            }

            return defaultValue;
        }

        if (property.ValueKind == JsonValueKind.String)
        {
            var text = property.GetString();
            if (string.IsNullOrWhiteSpace(text))
            {
                return defaultValue;
            }

            if (Enum.TryParse<ExecutionStatus>(text, ignoreCase: true, out var status))
            {
                return status;
            }
        }

        return defaultValue;
    }

    private static ExecutionKind ParseEnum(JsonElement element, string propertyName, ExecutionKind defaultValue)
    {
        if (!element.TryGetProperty(propertyName, out var property) || property.ValueKind == JsonValueKind.Null)
        {
            return defaultValue;
        }

        if (property.ValueKind == JsonValueKind.Number)
        {
            if (property.TryGetInt32(out var numeric))
            {
                return (ExecutionKind)numeric;
            }

            if (property.TryGetInt64(out var numeric64))
            {
                return (ExecutionKind)numeric64;
            }

            return defaultValue;
        }

        if (property.ValueKind == JsonValueKind.String)
        {
            var text = property.GetString();
            if (string.IsNullOrWhiteSpace(text))
            {
                return defaultValue;
            }

            if (Enum.TryParse<ExecutionKind>(text, ignoreCase: true, out var kind))
            {
                return kind;
            }
        }

        return defaultValue;
    }

    private readonly record struct StartSnapshot
    {
        public string? ExecutionId { get; init; }
        public ExecutionKind Kind { get; init; }
        public string? WorkflowId { get; init; }
        public string? JobKey { get; init; }
        public string? TenantId { get; init; }
        public string? EnvironmentTag { get; init; }
        public string? TriggerId { get; init; }
        public DateTimeOffset? FireAtUtc { get; init; }
        public DateTimeOffset? StartedAtUtc { get; init; }
        public string? InstanceId { get; init; }
        public string? TraceId { get; init; }
        public string? CorrelationId { get; init; }
    }

    private readonly record struct CompletionSnapshot
    {
        public DateTimeOffset? CompletedAtUtc { get; init; }
        public ExecutionStatus? Status { get; init; }
        public double? DurationMs { get; init; }
        public string? ErrorType { get; init; }
        public string? ErrorMessage { get; init; }
    }
}
