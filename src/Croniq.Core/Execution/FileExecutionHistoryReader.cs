using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Collections.Concurrent;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Execution;

/// <summary>
/// Reads execution summaries from filesystem-backed NDJSON logs.
/// </summary>
public sealed class FileExecutionHistoryReader : IExecutionHistoryReader
{
    private readonly FileExecutionLogStoreOptions _options;

    private const int DefaultIndexLookbackDays = 14;
    private static readonly ConcurrentDictionary<string, CachedSummaryEntry> SummaryCache = new(StringComparer.OrdinalIgnoreCase);
    private const int MaxCachedSummaries = 5000;
    private const int CompletionTailBytes = 128 * 1024;
    private const int IndexTailBytes = 512 * 1024;

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

        var fromIndex = await TryListExecutionsFromIndexAsync(scope, normalized, cancellationToken).ConfigureAwait(false);
        if (fromIndex.Count >= normalized.Limit)
        {
            return fromIndex;
        }

        var results = new List<ExecutionSummary>(fromIndex);
        foreach (var file in EnumerateFiles(root))
        {
            cancellationToken.ThrowIfCancellationRequested();
            ExecutionSummary? summary = null;
            try
            {
                summary = await TryGetOrParseSummaryAsync(file, cancellationToken).ConfigureAwait(false);
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

            if (results.Any(existing => string.Equals(existing.ExecutionId, summary.ExecutionId, StringComparison.OrdinalIgnoreCase)))
            {
                continue;
            }

            results.Add(summary);
        }

        return results
            .OrderByDescending(summary => summary.StartedAtUtc)
            .ThenByDescending(summary => summary.ExecutionId, StringComparer.OrdinalIgnoreCase)
            .Take(normalized.Limit)
            .ToArray();
    }

    private async Task<IReadOnlyList<ExecutionSummary>> TryListExecutionsFromIndexAsync(PartitionScope scope, ExecutionHistoryQuery query, CancellationToken cancellationToken)
    {
        var nowUtc = DateTimeOffset.UtcNow;
        var endDay = (query.StartedBeforeUtc ?? nowUtc).UtcDateTime.Date;
        var startDay = (query.StartedAfterUtc ?? nowUtc.AddDays(-DefaultIndexLookbackDays)).UtcDateTime.Date;

        if (startDay > endDay)
        {
            return Array.Empty<ExecutionSummary>();
        }

        var results = new List<ExecutionSummary>(query.Limit);
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        for (var day = endDay; day >= startDay; day = day.AddDays(-1))
        {
            cancellationToken.ThrowIfCancellationRequested();

            var dayOffset = new DateTimeOffset(day, TimeSpan.Zero);
            var indexPath = FileExecutionLogPathHelper.GetDailyIndexPath(_options, scope.TenantId, scope.EnvironmentTag, dayOffset);
            if (!File.Exists(indexPath))
            {
                continue;
            }

            IReadOnlyList<IndexedEntry> entries;
            try
            {
                entries = await ReadIndexTailAsync(indexPath, cancellationToken).ConfigureAwait(false);
            }
            catch
            {
                continue;
            }

            foreach (var entry in entries)
            {
                cancellationToken.ThrowIfCancellationRequested();

                if (!MatchesQuery(entry.Summary, query))
                {
                    continue;
                }

                if (!seen.Add(entry.Summary.ExecutionId))
                {
                    continue;
                }

                results.Add(entry.Summary);
                if (results.Count >= query.Limit)
                {
                    return results
                        .OrderByDescending(summary => summary.StartedAtUtc)
                        .ThenByDescending(summary => summary.ExecutionId, StringComparer.OrdinalIgnoreCase)
                        .Take(query.Limit)
                        .ToArray();
                }
            }
        }

        return results
            .OrderByDescending(summary => summary.StartedAtUtc)
            .ThenByDescending(summary => summary.ExecutionId, StringComparer.OrdinalIgnoreCase)
            .Take(query.Limit)
            .ToArray();
    }

    private static async Task<IReadOnlyList<IndexedEntry>> ReadIndexTailAsync(string indexPath, CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(indexPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite, 4096, useAsync: true);
        if (!stream.CanSeek || stream.Length <= 0)
        {
            return Array.Empty<IndexedEntry>();
        }

        var length = stream.Length;
        var tailBytes = (int)Math.Min(IndexTailBytes, length);
        stream.Seek(length - tailBytes, SeekOrigin.Begin);

        var buffer = new byte[tailBytes];
        var read = 0;
        while (read < tailBytes)
        {
            var chunk = await stream.ReadAsync(buffer.AsMemory(read, tailBytes - read), cancellationToken).ConfigureAwait(false);
            if (chunk == 0)
            {
                break;
            }
            read += chunk;
        }

        if (read <= 0)
        {
            return Array.Empty<IndexedEntry>();
        }

        var text = Encoding.UTF8.GetString(buffer, 0, read);
        var lines = text.Split(new[] { "\r\n", "\n" }, StringSplitOptions.RemoveEmptyEntries);

        var results = new List<IndexedEntry>();
        for (var idx = lines.Length - 1; idx >= 0; idx--)
        {
            var line = lines[idx];
            if (line.IndexOf("summary", StringComparison.OrdinalIgnoreCase) < 0)
            {
                continue;
            }

            if (TryParseIndexedEntry(line, out var entry) && entry is not null)
            {
                results.Add(entry.Value);
            }
        }

        return results;
    }

    private static bool TryParseIndexedEntry(string line, out IndexedEntry? entry)
    {
        entry = null;
        try
        {
            using var document = JsonDocument.Parse(line);
            var root = document.RootElement;
            if (!IsType(root, "summary"))
            {
                return false;
            }

            var executionId = GetString(root, "executionId");
            var jobKey = GetString(root, "jobKey");
            var tenantId = GetString(root, "tenantId");
            var environmentTag = GetString(root, "environmentTag");
            var fireAtUtc = GetDateTime(root, "fireAtUtc");
            var startedAtUtc = GetDateTime(root, "startedAtUtc");
            if (executionId is null || jobKey is null || tenantId is null || environmentTag is null || fireAtUtc is null || startedAtUtc is null)
            {
                return false;
            }

            var kind = ParseEnum(root, "kind", ExecutionKind.Job);
            var workflowId = GetString(root, "workflowId");
            var triggerId = GetString(root, "triggerId");
            var completedAtUtc = GetDateTime(root, "completedAtUtc");
            var status = ParseEnum(root, "status", (ExecutionStatus?)null);
            var durationMs = GetNullableDouble(root, "durationMs");
            var instanceId = GetString(root, "instanceId");
            var traceId = GetString(root, "traceId");
            var correlationId = GetString(root, "correlationId");
            var errorType = GetString(root, "errorType");
            var errorMessage = GetString(root, "errorMessage");

            var logPath = GetString(root, "logPath");

            var summary = new ExecutionSummary(
                executionId,
                kind,
                workflowId,
                jobKey,
                tenantId,
                environmentTag,
                triggerId,
                fireAtUtc.Value,
                startedAtUtc.Value,
                completedAtUtc,
                status,
                durationMs,
                instanceId,
                traceId,
                correlationId,
                errorType,
                errorMessage);

            entry = new IndexedEntry(summary, logPath);
            return true;
        }
        catch
        {
            return false;
        }
    }

    public async Task<ExecutionSummary?> GetExecutionAsync(string executionId, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(executionId))
        {
            return null;
        }

        var fromIndex = await TryGetExecutionFromIndexAsync(executionId, cancellationToken).ConfigureAwait(false);
        if (fromIndex is not null)
        {
            return fromIndex;
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
            return await TryGetOrParseSummaryAsync(new FileInfo(filePath), cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            return null;
        }
    }

    private async Task<ExecutionSummary?> TryGetExecutionFromIndexAsync(string executionId, CancellationToken cancellationToken)
    {
        var basePath = FileExecutionLogPathHelper.ResolveBasePath(_options);
        if (!Directory.Exists(basePath))
        {
            return null;
        }

        var cutoff = DateTimeOffset.UtcNow.UtcDateTime.Date.AddDays(-DefaultIndexLookbackDays);

        IEnumerable<FileInfo> indexFiles;
        try
        {
            indexFiles = Directory
                .EnumerateFiles(basePath, "executions-index-*.ndjson", SearchOption.AllDirectories)
                .Where(path => string.Equals(new DirectoryInfo(Path.GetDirectoryName(path) ?? string.Empty).Name, "_index", StringComparison.OrdinalIgnoreCase))
                .Select(path => new FileInfo(path))
                .Where(info => TryParseIndexDate(info.Name, out var date) && date >= cutoff)
                .OrderByDescending(info => info.LastWriteTimeUtc);
        }
        catch
        {
            return null;
        }

        foreach (var indexFile in indexFiles)
        {
            cancellationToken.ThrowIfCancellationRequested();

            IReadOnlyList<IndexedEntry> entries;
            try
            {
                entries = await ReadIndexTailAsync(indexFile.FullName, cancellationToken).ConfigureAwait(false);
            }
            catch
            {
                continue;
            }

            foreach (var entry in entries)
            {
                if (!string.Equals(entry.Summary.ExecutionId, executionId, StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                TryWarmCache(entry);
                return entry.Summary;
            }
        }

        return null;
    }

    private void TryWarmCache(IndexedEntry entry)
    {
        if (string.IsNullOrWhiteSpace(entry.LogPath))
        {
            return;
        }

        try
        {
            var scopeRoot = FileExecutionLogPathHelper.GetScopeRoot(_options, entry.Summary.TenantId, entry.Summary.EnvironmentTag);
            var fullPath = Path.GetFullPath(Path.Combine(scopeRoot, entry.LogPath));
            if (!File.Exists(fullPath))
            {
                return;
            }

            var info = new FileInfo(fullPath);
            SummaryCache[fullPath] = new CachedSummaryEntry(info.LastWriteTimeUtc, info.Length, entry.Summary);
        }
        catch
        {
            // Best effort.
        }
    }

    private static bool TryParseIndexDate(string fileName, out DateTime date)
    {
        date = default;

        const string prefix = "executions-index-";
        const string suffix = ".ndjson";

        if (!fileName.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) || !fileName.EndsWith(suffix, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        var core = fileName[prefix.Length..^suffix.Length];
        return DateTime.TryParseExact(core, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out date);
    }

    private static async Task<ExecutionSummary?> TryGetOrParseSummaryAsync(FileInfo file, CancellationToken cancellationToken)
    {
        if (SummaryCache.Count > MaxCachedSummaries)
        {
            SummaryCache.Clear();
        }

        var cacheKey = file.FullName;
        var lastWriteUtc = file.LastWriteTimeUtc;
        var length = file.Length;

        if (SummaryCache.TryGetValue(cacheKey, out var cached)
            && cached.LastWriteTimeUtc == lastWriteUtc
            && cached.Length == length)
        {
            return cached.Summary;
        }

        var parsed = await TryParseSummaryAsync(file.FullName, cancellationToken).ConfigureAwait(false);
        SummaryCache[cacheKey] = new CachedSummaryEntry(lastWriteUtc, length, parsed);
        return parsed;
    }

    private static IEnumerable<FileInfo> EnumerateFiles(string root)
    {
        return Directory
            .EnumerateFiles(root, "*.ndjson", SearchOption.AllDirectories)
            .Where(path => !path.Contains($"{Path.DirectorySeparatorChar}_index{Path.DirectorySeparatorChar}", StringComparison.OrdinalIgnoreCase))
            .Where(path => !Path.GetFileName(path).StartsWith("executions-index-", StringComparison.OrdinalIgnoreCase))
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
        await using var stream = new FileStream(filePath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite, 4096, useAsync: true);
        using var reader = new StreamReader(stream, Encoding.UTF8, detectEncodingFromByteOrderMarks: true, bufferSize: 4096, leaveOpen: true);

        var startLine = await reader.ReadLineAsync().ConfigureAwait(false);
        if (startLine is null)
        {
            return null;
        }

        if (!TryParseStart(startLine, out var start))
        {
            return null;
        }

        CompletionSnapshot? completion = await TryParseCompletionFromTailAsync(stream, cancellationToken).ConfigureAwait(false);

        if (completion is null)
        {
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

    private static async Task<CompletionSnapshot?> TryParseCompletionFromTailAsync(FileStream stream, CancellationToken cancellationToken)
    {
        if (!stream.CanSeek)
        {
            return null;
        }

        var length = stream.Length;
        if (length <= 0)
        {
            return null;
        }

        var tailBytes = (int)Math.Min(CompletionTailBytes, length);
        var startOffset = length - tailBytes;
        stream.Seek(startOffset, SeekOrigin.Begin);

        var buffer = new byte[tailBytes];
        var read = 0;
        while (read < tailBytes)
        {
            var chunk = await stream.ReadAsync(buffer.AsMemory(read, tailBytes - read), cancellationToken).ConfigureAwait(false);
            if (chunk == 0)
            {
                break;
            }
            read += chunk;
        }

        if (read <= 0)
        {
            return null;
        }

        var text = Encoding.UTF8.GetString(buffer, 0, read);
        var lines = text.Split(new[] { "\r\n", "\n" }, StringSplitOptions.RemoveEmptyEntries);
        for (var idx = lines.Length - 1; idx >= 0; idx--)
        {
            var line = lines[idx];
            if (line.IndexOf("completion", StringComparison.OrdinalIgnoreCase) < 0)
            {
                continue;
            }

            if (TryParseCompletion(line, out var snapshot))
            {
                return snapshot;
            }
        }

        return null;
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

    private readonly record struct IndexedEntry(ExecutionSummary Summary, string? LogPath);

    private readonly record struct CachedSummaryEntry(DateTime LastWriteTimeUtc, long Length, ExecutionSummary? Summary);
}
