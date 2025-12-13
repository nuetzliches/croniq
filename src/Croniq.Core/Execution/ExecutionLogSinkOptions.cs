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
}
