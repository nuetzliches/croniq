using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Options for the filesystem-based execution log store.
/// </summary>
public sealed class FileExecutionLogStoreOptions
{
    /// <summary>
    /// Base directory where execution logs are written. Default: "logs".
    /// </summary>
    public string BasePath { get; set; } = "logs";

    /// <summary>
    /// Optional shard length (characters from ExecutionId) to reduce directory fan-out. Default: 2.
    /// </summary>
    public int ShardPrefixLength { get; set; } = 2;

    /// <summary>
    /// How long to keep in-memory execution tracking after completion. Default: 10 minutes.
    /// </summary>
    public TimeSpan ExecutionCacheRetention { get; set; } = TimeSpan.FromMinutes(10);

    /// <summary>
    /// How often to sweep completed execution tracking entries. Default: 5 minutes.
    /// </summary>
    public TimeSpan ExecutionCacheCleanupInterval { get; set; } = TimeSpan.FromMinutes(5);
}
