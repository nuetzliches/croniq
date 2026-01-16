using System;
using System.IO;

namespace Croniq.Core.Execution;

internal static class FileExecutionLogPathHelper
{
    private static readonly char[] InvalidFileNameChars = Path.GetInvalidFileNameChars();
    private const string IndexFolderName = "_index";

    public static string ResolveBasePath(FileExecutionLogStoreOptions options)
    {
        if (options is null)
        {
            throw new ArgumentNullException(nameof(options));
        }

        return string.IsNullOrWhiteSpace(options.BasePath) ? "logs" : options.BasePath;
    }

    public static string GetScopeRoot(FileExecutionLogStoreOptions options, string tenantId, string environmentTag)
    {
        return Path.Combine(
            ResolveBasePath(options),
            Sanitize(tenantId),
            Sanitize(environmentTag));
    }

    public static string GetIndexRoot(FileExecutionLogStoreOptions options, string tenantId, string environmentTag)
    {
        return Path.Combine(GetScopeRoot(options, tenantId, environmentTag), IndexFolderName);
    }

    public static string GetDailyIndexPath(FileExecutionLogStoreOptions options, string tenantId, string environmentTag, DateTimeOffset startedAtUtc)
    {
        var indexRoot = GetIndexRoot(options, tenantId, environmentTag);
        var day = startedAtUtc.UtcDateTime;
        var name = $"executions-index-{day:yyyy-MM-dd}.ndjson";
        return Path.Combine(indexRoot, name);
    }

    public static string ResolveShard(string executionId, int prefixLength)
    {
        if (prefixLength <= 0 || string.IsNullOrWhiteSpace(executionId))
        {
            return "shard";
        }

        var length = Math.Min(prefixLength, executionId.Length);
        return Sanitize(executionId[..length]);
    }

    public static string BuildKindSegment(ExecutionKind kind, string jobKey, string? workflowId)
    {
        if (kind == ExecutionKind.Workflow && !string.IsNullOrWhiteSpace(workflowId))
        {
            return $"wf-{Sanitize(workflowId!)}";
        }

        return $"job-{Sanitize(jobKey)}";
    }

    public static string Sanitize(string value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return string.Empty;
        }

        var trimmed = value.Trim();
        var buffer = new char[trimmed.Length];
        var idx = 0;
        foreach (var ch in trimmed)
        {
            if (Array.IndexOf(InvalidFileNameChars, ch) >= 0 || ch == ':')
            {
                buffer[idx++] = '_';
            }
            else
            {
                buffer[idx++] = ch;
            }
        }

        return new string(buffer, 0, idx);
    }
}
