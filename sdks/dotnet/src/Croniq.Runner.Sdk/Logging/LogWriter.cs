using System.Net;
using System.Threading.Channels;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Logging;

/// <summary>
/// Default <see cref="ILogWriter"/> implementation. Backed by a bounded
/// <see cref="Channel{T}"/> and a single background flusher task. Mirrors
/// the Rust SDK's <c>log_writer.rs</c> semantics: batch-by-count, batch-
/// by-timer, drain-on-dispose with a hard cap.
/// </summary>
internal sealed class LogWriter : ILogWriter
{
    private readonly ICroniqClient _client;
    private readonly string _executionId;
    private readonly LogEnrichment _enrichment;
    private readonly LogWriterOptions _options;
    private readonly ILogger _logger;
    private readonly TimeProvider _timeProvider;

    private readonly Channel<Command> _channel;
    private readonly CancellationTokenSource _shutdownCts = new();
    private readonly Task _flusherTask;

    private readonly Action<int>? _onFlusherParked;

    private int _disposed;

    /// <param name="onFlusherParked">
    /// Test-only hook, invoked immediately before the flusher parks on
    /// <see cref="Task.WhenAny(Task[])"/> — the moment at which every command
    /// written so far has been drained into the batch buffer <em>and</em> the
    /// <see cref="PeriodicTimer"/>'s wait is registered against
    /// <paramref name="timeProvider"/>. The argument is the number of events
    /// currently buffered.
    /// <para>
    /// A test driving a <c>FakeTimeProvider</c> has to establish exactly that
    /// state before it calls <c>Advance</c>, and it cannot be observed from
    /// outside (issue #570). The timer is constructed inside the loop, so an
    /// <c>Advance</c> that lands first schedules the next tick past a clock
    /// nothing will move again; one that lands before the event is drained
    /// ticks an empty buffer, which the tick branch correctly skips. Either
    /// way no POST is ever produced and no amount of waiting afterwards
    /// recovers it — fake time only moves when the test moves it. A
    /// real-time sleep is a bet on runner scheduling; this is not.
    /// </para>
    /// <para>
    /// Passed through the constructor rather than exposed as an event so it is
    /// wired before the loop starts: the loop can park before a subscriber
    /// attached, and in a test where nothing else wakes it that first park is
    /// the only one. Null in production — the cost is a null-check per loop
    /// iteration.
    /// </para>
    /// </param>
    public LogWriter(
        ICroniqClient client,
        string executionId,
        LogEnrichment enrichment,
        LogWriterOptions options,
        ILogger logger,
        TimeProvider? timeProvider = null,
        Action<int>? onFlusherParked = null)
    {
        _client = client;
        _executionId = executionId;
        _enrichment = enrichment;
        _options = options;
        _logger = logger;
        // Default to TimeProvider.System so production code paths are
        // byte-equivalent to the pre-TimeProvider behaviour. Tests pass
        // a FakeTimeProvider to drive the time-threshold flush loop
        // deterministically (see #134 sub-item 3 — the case-10
        // conformance scenario's `Task.WhenAny` read-bias is impossible
        // to verify without controllable time).
        _timeProvider = timeProvider ?? TimeProvider.System;
        _onFlusherParked = onFlusherParked;

        _channel = Channel.CreateBounded<Command>(new BoundedChannelOptions(options.ChannelCapacity)
        {
            FullMode = BoundedChannelFullMode.Wait,
            SingleReader = true,
            SingleWriter = false,
        });
        _flusherTask = Task.Run(() => FlusherLoopAsync(_shutdownCts.Token));
    }

    public ValueTask WriteAsync(
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
        return WriteAsync(ev, cancellationToken);
    }

    public ValueTask WriteAsync(WorkEvent workEvent, CancellationToken cancellationToken = default)
    {
        if (Volatile.Read(ref _disposed) != 0)
        {
            return ValueTask.CompletedTask;
        }
        return _channel.Writer.WriteAsync(new Command.WriteEvent(workEvent), cancellationToken);
    }

    public async ValueTask FlushAsync(CancellationToken cancellationToken = default)
    {
        if (Volatile.Read(ref _disposed) != 0)
        {
            return;
        }
        var tcs = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        await _channel.Writer.WriteAsync(new Command.Flush(tcs), cancellationToken).ConfigureAwait(false);
        using var registration = cancellationToken.Register(() => tcs.TrySetCanceled(cancellationToken));
        await tcs.Task.ConfigureAwait(false);
    }

    public TextWriter AsLineWriter(LogLevel level = LogLevel.Information) => new LineTextWriter(this, level);

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0)
        {
            return;
        }

        _channel.Writer.TryComplete();

        try
        {
            await _flusherTask.WaitAsync(_options.ShutdownTimeout).ConfigureAwait(false);
        }
        catch (TimeoutException)
        {
            // execution_id travels as scope state, not inside the message —
            // it is server-supplied (#441).
            using (_logger.BeginScope(new Dictionary<string, object> { ["execution_id"] = _executionId }))
            {
                _logger.LogWarning("log_writer drain timed out after {Timeout}", _options.ShutdownTimeout);
            }
            await _shutdownCts.CancelAsync().ConfigureAwait(false);
        }
        finally
        {
            _shutdownCts.Dispose();
        }
    }

    private async Task FlusherLoopAsync(CancellationToken shutdownCt)
    {
        var buffer = new List<WorkEvent>(_options.MaxBatchPerPost);
        var pendingFlushes = new List<TaskCompletionSource>(2);

        var timer = new PeriodicTimer(_options.BatchTimeThreshold, _timeProvider);
        // PeriodicTimer.WaitForNextTickAsync supports only **one pending
        // consumer at a time** — calling it again before the previous
        // task has completed throws InvalidOperationException. Keep one
        // tickTask in flight across loop iterations and only re-arm it
        // after the timer-branch consumes it. Without this, every
        // channel-read win would attempt a fresh WaitForNextTickAsync
        // while the prior tickTask is still pending; in real time the
        // 200 ms window often masked the race, but with TimeProvider /
        // FakeTimeProvider it surfaces immediately.
        Task<bool>? tickTask = null;
        try
        {
            while (true)
            {
                tickTask ??= timer.WaitForNextTickAsync(shutdownCt).AsTask();
                var readTask = _channel.Reader.WaitToReadAsync(shutdownCt).AsTask();

                // Both waits are registered and the buffer holds everything
                // read so far — the state a fake-clock test needs before it
                // advances time. Raised before the await rather than after it
                // so a subscriber cannot observe a half-armed loop; the tick
                // completes `tickTask` whether or not anyone is awaiting it
                // yet, so there is no window to lose.
                _onFlusherParked?.Invoke(buffer.Count);

                var winner = await Task.WhenAny(readTask, tickTask).ConfigureAwait(false);

                if (winner == readTask)
                {
                    bool more;
                    try
                    {
                        more = await readTask.ConfigureAwait(false);
                    }
                    catch (OperationCanceledException)
                    {
                        more = false;
                    }

                    while (_channel.Reader.TryRead(out var cmd))
                    {
                        switch (cmd)
                        {
                            case Command.WriteEvent we:
                                buffer.Add(we.Event);
                                if (buffer.Count >= _options.BatchSizeThreshold)
                                {
                                    await FlushBufferAsync(buffer).ConfigureAwait(false);
                                    CompletePendingFlushes(pendingFlushes);
                                }
                                break;
                            case Command.Flush flush:
                                pendingFlushes.Add(flush.Completion);
                                break;
                        }
                    }

                    if (!more)
                    {
                        break;
                    }

                    if (pendingFlushes.Count > 0)
                    {
                        await FlushBufferAsync(buffer).ConfigureAwait(false);
                        CompletePendingFlushes(pendingFlushes);
                    }

                    // tickTask stays in flight — we re-use it on the
                    // next iteration so the PeriodicTimer never sees
                    // two concurrent WaitForNextTickAsync calls.
                }
                else if (winner == tickTask)
                {
                    bool hadTick;
                    try
                    {
                        hadTick = await tickTask.ConfigureAwait(false);
                    }
                    catch (OperationCanceledException)
                    {
                        break;
                    }
                    tickTask = null; // re-arm on the next iteration
                    if (!hadTick)
                    {
                        break;
                    }

                    if (buffer.Count > 0)
                    {
                        await FlushBufferAsync(buffer).ConfigureAwait(false);
                    }
                    CompletePendingFlushes(pendingFlushes);
                }
            }
        }
        finally
        {
            timer.Dispose();

            // Drain remainder after shutdown / completion
            while (_channel.Reader.TryRead(out var cmd))
            {
                switch (cmd)
                {
                    case Command.WriteEvent we:
                        buffer.Add(we.Event);
                        break;
                    case Command.Flush flush:
                        pendingFlushes.Add(flush.Completion);
                        break;
                }
            }
            await FlushBufferAsync(buffer).ConfigureAwait(false);
            CompletePendingFlushes(pendingFlushes);
        }
    }

    private async Task FlushBufferAsync(List<WorkEvent> buffer)
    {
        while (buffer.Count > 0)
        {
            var take = Math.Min(buffer.Count, _options.MaxBatchPerPost);
            var chunk = new List<WorkEvent>(take);
            for (var i = 0; i < take; i++)
            {
                chunk.Add(_enrichment.Enrich(buffer[i]));
            }
            buffer.RemoveRange(0, take);

            try
            {
                await _client.PushEventsAsync(_executionId, chunk, CancellationToken.None).ConfigureAwait(false);
            }
            catch (HttpRequestException ex) when (ex.StatusCode == HttpStatusCode.Forbidden)
            {
                // Ownership refusal (#436/#437) — permanent, so every later
                // batch is lost too. Loud enough that an operator notices the
                // silent log stream instead of hunting for missing output.
                using (_logger.BeginScope(new Dictionary<string, object> { ["execution_id"] = _executionId }))
                {
                    _logger.LogError(
                        ex,
                        "log_writer: batch POST refused with 403 Forbidden — this runner's " +
                        "credential does not own its runner_id, so no log event will reach the " +
                        "server ({Count} events dropped). Give the runner its own runner_id, or " +
                        "release the existing binding with DELETE /v1/runners/{{id}}.",
                        chunk.Count);
                }
            }
            catch (Exception ex)
            {
                using (_logger.BeginScope(new Dictionary<string, object> { ["execution_id"] = _executionId }))
                {
                    _logger.LogWarning(ex, "log_writer: batch POST failed — {Count} events dropped", chunk.Count);
                }
            }
        }
    }

    private static void CompletePendingFlushes(List<TaskCompletionSource> pending)
    {
        if (pending.Count == 0)
        {
            return;
        }
        foreach (var tcs in pending)
        {
            tcs.TrySetResult();
        }
        pending.Clear();
    }

    private static string MapLevel(LogLevel level) => level switch
    {
        LogLevel.Trace => "trace",
        LogLevel.Debug => "debug",
        LogLevel.Information => "info",
        LogLevel.Warning => "warn",
        LogLevel.Error => "error",
        LogLevel.Critical => "error",
        _ => "info",
    };

    private abstract record Command
    {
        public sealed record WriteEvent(WorkEvent Event) : Command;
        public sealed record Flush(TaskCompletionSource Completion) : Command;
    }
}
