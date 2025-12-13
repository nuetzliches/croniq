using System;
using System.Collections.Concurrent;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

/// <summary>
/// Simple filesystem-backed execution log store that writes NDJSON per execution.
/// </summary>
public sealed class FileExecutionLogStore : IExecutionLogStore
{
    private readonly FileExecutionLogStoreOptions _options;
    private readonly ConcurrentDictionary<string, string> _executionFiles = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, SemaphoreSlim> _locks = new(StringComparer.OrdinalIgnoreCase);
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web)
    {
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull
    };

    public FileExecutionLogStore(FileExecutionLogStoreOptions options)
    {
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public Task OnExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken)
    {
        if (record is null) throw new ArgumentNullException(nameof(record));

        var path = ResolvePath(record);
        _executionFiles[record.ExecutionId] = path;
        _locks.TryAdd(record.ExecutionId, new SemaphoreSlim(1, 1));

        return WriteLineAsync(path, new
        {
            type = "start",
            record.ExecutionId,
            record.Kind,
            record.JobKey,
            record.TenantId,
            record.EnvironmentTag,
            record.TriggerId,
            record.FireAtUtc,
            record.StartedAtUtc,
            record.InstanceId,
            record.TraceId,
            record.SpanId,
            record.CorrelationId
        }, record.ExecutionId, cancellationToken);
    }

    public Task AppendAsync(string executionId, System.Collections.Generic.IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(executionId)) throw new ArgumentException("ExecutionId must be provided", nameof(executionId));
        if (entries is null) throw new ArgumentNullException(nameof(entries));
        if (!_executionFiles.TryGetValue(executionId, out var path))
        {
            return Task.CompletedTask;
        }

        return WriteBatchAsync(path, executionId, entries, cancellationToken);
    }

    public Task OnExecutionCompletedAsync(ExecutionCompletion completion, CancellationToken cancellationToken)
    {
        if (completion is null) throw new ArgumentNullException(nameof(completion));
        if (!_executionFiles.TryGetValue(completion.ExecutionId, out var path))
        {
            return Task.CompletedTask;
        }

        return WriteLineAsync(path, new
        {
            type = "completion",
            completion.ExecutionId,
            completion.CompletedAtUtc,
            completion.Status,
            completion.DurationMs,
            completion.ErrorType,
            completion.ErrorMessage
        }, completion.ExecutionId, cancellationToken);
    }

    private async Task WriteBatchAsync(string path, string executionId, System.Collections.Generic.IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken)
    {
        var logBuilder = new StringBuilder();
        foreach (var entry in entries)
        {
            logBuilder.AppendLine(JsonSerializer.Serialize(new
            {
                type = "log",
                entry.ExecutionId,
                entry.TimestampUtc,
                entry.Level,
                entry.MessageTemplate,
                entry.RenderedMessage,
                entry.Exception,
                entry.Properties,
                entry.TraceId,
                entry.SpanId,
                entry.CorrelationId,
                entry.Sequence
            }, _jsonOptions));
        }

        await WriteRawAsync(path, executionId, logBuilder.ToString(), cancellationToken).ConfigureAwait(false);
    }

    private Task WriteLineAsync(string path, object payload, string executionId, CancellationToken cancellationToken)
    {
        var line = JsonSerializer.Serialize(payload, _jsonOptions) + Environment.NewLine;
        return WriteRawAsync(path, executionId, line, cancellationToken);
    }

    private async Task WriteRawAsync(string path, string executionId, string content, CancellationToken cancellationToken)
    {
        var semaphore = _locks.GetOrAdd(executionId, _ => new SemaphoreSlim(1, 1));
        await semaphore.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrWhiteSpace(directory))
            {
                Directory.CreateDirectory(directory);
            }

            await using var stream = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read, 4096, useAsync: true);
            var buffer = Encoding.UTF8.GetBytes(content);
            await stream.WriteAsync(buffer.AsMemory(0, buffer.Length), cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            semaphore.Release();
        }
    }

    private string ResolvePath(ExecutionRecord record)
    {
        var basePath = string.IsNullOrWhiteSpace(_options.BasePath) ? "logs" : _options.BasePath;
        var started = record.StartedAtUtc.UtcDateTime;
        var year = started.ToString("yyyy", CultureInfo.InvariantCulture);
        var month = started.ToString("MM", CultureInfo.InvariantCulture);
        var day = started.ToString("dd", CultureInfo.InvariantCulture);
        var safeKey = Sanitize(record.JobKey);
        var kindSegment = record.Kind switch
        {
            ExecutionKind.Workflow when !string.IsNullOrWhiteSpace(record.WorkflowId) => $"wf-{Sanitize(record.WorkflowId!)}",
            _ => $"job-{safeKey}"
        };
        var shard = ResolveShard(record.ExecutionId);

        return Path.Combine(
            basePath,
            Sanitize(record.TenantId),
            Sanitize(record.EnvironmentTag),
            kindSegment,
            year,
            month,
            day,
            shard,
            $"{record.ExecutionId}.ndjson");
    }

    private static string Sanitize(string value)
    {
        var invalid = Path.GetInvalidFileNameChars();
        var sb = new StringBuilder(value.Length);
        foreach (var ch in value)
        {
            if (Array.IndexOf(invalid, ch) >= 0 || ch == ':')
            {
                sb.Append('_');
            }
            else
            {
                sb.Append(ch);
            }
        }

        return sb.ToString();
    }

    private string ResolveShard(string executionId)
    {
        if (_options.ShardPrefixLength <= 0 || string.IsNullOrWhiteSpace(executionId))
        {
            return "shard";
        }

        var length = Math.Min(_options.ShardPrefixLength, executionId.Length);
        return Sanitize(executionId[..length]);
    }
}
