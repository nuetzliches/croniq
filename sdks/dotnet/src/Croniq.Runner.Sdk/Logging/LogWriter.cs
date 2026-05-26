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

    private int _disposed;

    public LogWriter(
        ICroniqClient client,
        string executionId,
        LogEnrichment enrichment,
        LogWriterOptions options,
        ILogger logger,
        TimeProvider? timeProvider = null)
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
            _logger.LogWarning(
                "log_writer drain timed out after {Timeout} (execution {ExecutionId})",
                _options.ShutdownTimeout, _executionId);
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
            catch (Exception ex)
            {
                _logger.LogWarning(
                    ex,
                    "log_writer: batch POST failed — {Count} events dropped (execution {ExecutionId})",
                    chunk.Count, _executionId);
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
