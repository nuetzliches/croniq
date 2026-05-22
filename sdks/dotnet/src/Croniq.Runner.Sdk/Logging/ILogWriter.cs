using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Logging;

/// <summary>
/// Streaming log-writer for one execution. Enqueues events into a bounded
/// channel; a background task batches and POSTs them to
/// <c>/v1/work/{execution_id}/events</c>. <see cref="WriteAsync(LogLevel, string, IReadOnlyDictionary{string, string}?, CancellationToken)"/>
/// suspends only on channel capacity, never on HTTP — so a long-running
/// subprocess's stdout reader will not deadlock when the server is slow.
/// </summary>
public interface ILogWriter : IAsyncDisposable
{
    /// <summary>Enqueue a structured event built from level + message + optional fields.</summary>
    ValueTask WriteAsync(
        LogLevel level,
        string message,
        IReadOnlyDictionary<string, string>? fields = null,
        CancellationToken cancellationToken = default);

    /// <summary>Enqueue a pre-constructed event.</summary>
    ValueTask WriteAsync(WorkEvent workEvent, CancellationToken cancellationToken = default);

    /// <summary>Wait until every currently-queued event is server-side.</summary>
    ValueTask FlushAsync(CancellationToken cancellationToken = default);

    /// <summary>
    /// A <see cref="TextWriter"/> adapter that splits incoming text on line
    /// breaks and enqueues one event per line at the given level. Use to
    /// pipe <see cref="System.Diagnostics.Process.StandardOutput"/> /
    /// <c>StandardError</c> directly into the log stream.
    /// </summary>
    TextWriter AsLineWriter(LogLevel level = LogLevel.Information);
}
