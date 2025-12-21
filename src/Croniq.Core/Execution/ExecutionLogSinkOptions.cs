using System;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Execution;

/// <summary>
/// Options controlling the execution log sink.
/// </summary>
public sealed class ExecutionLogSinkOptions
{
    /// <summary>
    /// Minimum log level to persist.
    /// </summary>
    public LogLevel MinimumLevel { get; set; } = LogLevel.Information;

    /// <summary>
    /// Maximum number of queued entries before dropping new ones.
    /// </summary>
    public int MaxQueueLength { get; set; } = 10_000;

    /// <summary>
    /// Batch size used when flushing to the store.
    /// </summary>
    public int BatchSize { get; set; } = 50;

    /// <summary>
    /// Periodic flush interval.
    /// </summary>
    public TimeSpan FlushInterval { get; set; } = TimeSpan.FromMilliseconds(500);

    /// <summary>
    /// How long to keep per-execution sequence counters. Default: 10 minutes.
    /// </summary>
    public TimeSpan SequenceRetention { get; set; } = TimeSpan.FromMinutes(10);

    /// <summary>
    /// How often to sweep stale sequence counters. Default: 2 minutes.
    /// </summary>
    public TimeSpan SequenceCleanupInterval { get; set; } = TimeSpan.FromMinutes(2);
}
