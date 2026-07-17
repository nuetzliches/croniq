using System.Text.Json;

using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Logging;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk;

/// <summary>
/// Context handed to a job handler for one execution. The handler receives
/// the work assignment payload (job key, attempt, raw metadata), a
/// <see cref="Logger"/> pre-scoped with execution identifiers, a streaming
/// <see cref="LogWriter"/> for sending logs back to the Croniq server UI,
/// and a <see cref="CancellationToken"/> linked to both host shutdown and
/// server-side cancellation.
/// </summary>
public sealed class CroniqExecutionContext
{
    private readonly Lazy<ILogWriter> _logWriter;
    private readonly Func<WorkEvent, CancellationToken, Task> _pushEventsAsync;

    internal CroniqExecutionContext(
        string executionId,
        string jobKey,
        DateTimeOffset? scheduledFor,
        int attempt,
        JsonElement metadata,
        TimeSpan timeout,
        string runnerId,
        IReadOnlyList<string> runnerTags,
        CancellationToken cancellationToken,
        ILogger logger,
        Lazy<ILogWriter> logWriter,
        Func<WorkEvent, CancellationToken, Task> pushEventsAsync)
    {
        ExecutionId = executionId;
        JobKey = jobKey;
        ScheduledFor = scheduledFor;
        Attempt = attempt;
        Metadata = metadata;
        Timeout = timeout;
        RunnerId = runnerId;
        RunnerTags = runnerTags;
        CancellationToken = cancellationToken;
        Logger = logger;
        _logWriter = logWriter;
        _pushEventsAsync = pushEventsAsync;
    }

    /// <summary>Server-assigned execution identifier.</summary>
    public string ExecutionId { get; }

    /// <summary>Job key, e.g. <c>billing:invoice-generate</c>.</summary>
    public string JobKey { get; }

    /// <summary>
    /// The trigger's original logical fire time — stable across retries and
    /// dead-letter replays. Use this (not <see cref="DateTimeOffset.UtcNow"/>)
    /// for time-relative job logic like "the month being reported".
    /// <c>null</c> when the server predates the field; the SDK never falls
    /// back to the queue fire time.
    /// </summary>
    public DateTimeOffset? ScheduledFor { get; }

    /// <summary>1-based attempt counter (incremented on each retry).</summary>
    public int Attempt { get; }

    /// <summary>
    /// Raw metadata payload from the server. Job-specific schema; use
    /// <see cref="JsonElement.TryGetProperty(string, out JsonElement)"/> or
    /// <c>JsonSerializer.Deserialize</c> to extract typed values.
    /// </summary>
    public JsonElement Metadata { get; }

    /// <summary>Server-declared timeout for this execution.</summary>
    public TimeSpan Timeout { get; }

    /// <summary>The runner's stable identifier.</summary>
    public string RunnerId { get; }

    /// <summary>Free-form tags this runner self-declared at registration.</summary>
    public IReadOnlyList<string> RunnerTags { get; }

    /// <summary>
    /// Combined token: linked to host shutdown <em>and</em> server-side
    /// cancellation. Handlers should propagate this to downstream awaits.
    /// </summary>
    public CancellationToken CancellationToken { get; }

    /// <summary>
    /// Logger pre-scoped with <c>execution_id</c>, <c>job_key</c>,
    /// <c>runner_id</c>, and <c>attempt</c>. Use for handler-side logging
    /// that goes through your standard observability backend.
    /// </summary>
    public ILogger Logger { get; }

    /// <summary>
    /// Streaming log-writer that POSTs events to the Croniq server's
    /// execution log (visible in the UI). Lazily initialised: the first
    /// access spawns the background flusher. The runner drains the writer
    /// (bounded by <see cref="LogWriterOptions.ShutdownTimeout"/>) before
    /// sending the ack.
    /// </summary>
    public ILogWriter LogWriter => _logWriter.Value;

    /// <summary>
    /// Push a single structured event to the execution log. Awaits the HTTP
    /// POST inline. For high-volume scenarios, prefer
    /// <see cref="LogWriter"/>.
    /// </summary>
    public Task LogAsync(
        LogLevel level,
        string message,
        IReadOnlyDictionary<string, string>? fields = null,
        CancellationToken cancellationToken = default)
    {
        var ev = new WorkEvent
        {
            Level = MapLevel(level),
            Message = message,
            Fields = fields,
        };
        return _pushEventsAsync(ev, cancellationToken);
    }

    /// <summary>
    /// Push one or more pre-constructed events. Awaits the HTTP POST inline.
    /// </summary>
    public Task PushLogEventsAsync(IReadOnlyList<WorkEvent> events, CancellationToken cancellationToken = default)
    {
        if (events.Count == 0)
        {
            return Task.CompletedTask;
        }
        if (events.Count == 1)
        {
            return _pushEventsAsync(events[0], cancellationToken);
        }
        return PushManyAsync(events, cancellationToken);
    }

    private async Task PushManyAsync(IReadOnlyList<WorkEvent> events, CancellationToken cancellationToken)
    {
        foreach (var ev in events)
        {
            await _pushEventsAsync(ev, cancellationToken).ConfigureAwait(false);
        }
    }

    internal Lazy<ILogWriter> LazyLogWriter => _logWriter;

    private static string MapLevel(LogLevel level) => level switch
    {
        LogLevel.Trace => "trace",
        LogLevel.Debug => "debug",
        LogLevel.Information => "info",
        LogLevel.Warning => "warn",
        LogLevel.Error or LogLevel.Critical => "error",
        _ => "info",
    };
}
