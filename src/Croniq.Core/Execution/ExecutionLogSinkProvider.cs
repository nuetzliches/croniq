using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Diagnostics.Metrics;
using System.Linq;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Execution;

/// <summary>
/// ILoggerProvider that captures execution-scoped log events and forwards them to the configured IExecutionLogStore.
/// Only logs within a scope containing "croniq.execution_id" are persisted.
/// </summary>
public sealed class ExecutionLogSinkProvider : ILoggerProvider, ISupportExternalScope
{
    private readonly IExecutionLogStore _store;
    private readonly Lazy<IExecutionLogExporter> _exporter;
    private readonly ExecutionLogSinkOptions _options;
    private readonly Channel<ExecutionLogEntry> _channel;
    private readonly CancellationTokenSource _cts = new();
    private readonly Task _background;
    private readonly ConcurrentDictionary<string, SequenceState> _sequences = new(StringComparer.OrdinalIgnoreCase);
    private long _lastSequenceCleanupTicks;
    private IExternalScopeProvider? _scopeProvider;
    private static readonly string ExporterCategory = typeof(LoggerExecutionLogExporter).FullName ?? "Croniq.Core.Execution.LoggerExecutionLogExporter";
    private static readonly Meter Meter = new("Croniq.Core.Execution.ExecutionLogSink");
    private static readonly Counter<long> DroppedEntries = Meter.CreateCounter<long>("croniq.execution_log.dropped");

    public ExecutionLogSinkProvider(
        IExecutionLogStore store,
        IServiceProvider services,
        IOptions<ExecutionLogSinkOptions> options)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        if (services is null) throw new ArgumentNullException(nameof(services));
        _exporter = new Lazy<IExecutionLogExporter>(
            () => services.GetService<IExecutionLogExporter>() ?? new NoOpExecutionLogExporter(),
            LazyThreadSafetyMode.ExecutionAndPublication);
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        var bounded = new BoundedChannelOptions(_options.MaxQueueLength)
        {
            FullMode = BoundedChannelFullMode.DropWrite,
            SingleReader = true,
            SingleWriter = false
        };
        _channel = Channel.CreateBounded<ExecutionLogEntry>(bounded);
        _background = Task.Run(BackgroundLoopAsync);
    }

    public ILogger CreateLogger(string categoryName) => new ExecutionLogger(this, categoryName);

    public void Dispose()
    {
        _cts.Cancel();
        _channel.Writer.TryComplete();
        try
        {
            _background.Wait(TimeSpan.FromSeconds(1));
        }
        catch
        {
            // ignore background completion issues
        }
    }

    public void SetScopeProvider(IExternalScopeProvider scopeProvider)
    {
        _scopeProvider = scopeProvider;
    }

    internal bool TryEnqueue(LogLevel level, string category, EventId eventId, object? state, Exception? exception, Func<object?, Exception?, string> formatter)
    {
        if (string.Equals(category, ExporterCategory, StringComparison.OrdinalIgnoreCase))
        {
            return false; // avoid recursion when exporter logs
        }

        if (level < _options.MinimumLevel)
        {
            return false;
        }

        var now = DateTimeOffset.UtcNow;
        var (executionId, properties) = ExtractScope();
        if (properties is not null && properties.TryGetValue("croniq.execution_log.skip", out var skipValue))
        {
            if (skipValue is bool skip && skip)
            {
                return false;
            }

            if (skipValue is string skipText && bool.TryParse(skipText, out var parsed) && parsed)
            {
                return false;
            }
        }
        if (string.IsNullOrWhiteSpace(executionId))
        {
            return false;
        }

        var messageTemplate = ExtractMessageTemplate(state);
        var rendered = formatter(state, exception);

        properties ??= new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);
        properties["category"] = category;
        properties["eventId"] = eventId.Id;

        var sequence = _sequences.AddOrUpdate(
            executionId,
            _ => new SequenceState(1, now),
            (_, current) => new SequenceState(current.Value + 1, now)).Value;

        TryCleanupSequences(now);

        var entry = new ExecutionLogEntry(
            executionId,
            now,
            level,
            messageTemplate ?? rendered,
            rendered,
            exception?.ToString(),
            properties,
            Activity.Current?.TraceId.ToString(),
            Activity.Current?.SpanId.ToString(),
            TryGetCorrelation(properties),
            sequence);

        if (!_channel.Writer.TryWrite(entry))
        {
            DroppedEntries.Add(1);
            return false;
        }

        return true;
    }

    private async Task BackgroundLoopAsync()
    {
        var batch = new List<ExecutionLogEntry>(_options.BatchSize);
        while (!_cts.IsCancellationRequested)
        {
            try
            {
                while (await _channel.Reader.WaitToReadAsync(_cts.Token).ConfigureAwait(false))
                {
                    while (_channel.Reader.TryRead(out var entry))
                    {
                        batch.Add(entry);
                        if (batch.Count >= _options.BatchSize)
                        {
                            await FlushAsync(batch).ConfigureAwait(false);
                            batch.Clear();
                        }
                    }

                    if (batch.Count > 0)
                    {
                        await FlushAsync(batch).ConfigureAwait(false);
                        batch.Clear();
                    }

                    TryCleanupSequences(DateTimeOffset.UtcNow);
                }
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch
            {
                // swallow to keep background loop alive
            }

            try
            {
                await Task.Delay(_options.FlushInterval, _cts.Token).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }

        if (batch.Count > 0)
        {
            await FlushAsync(batch).ConfigureAwait(false);
        }
    }

    private async Task FlushAsync(IReadOnlyCollection<ExecutionLogEntry> entries)
    {
        var exporter = _exporter.Value;
        foreach (var group in entries.GroupBy(x => x.ExecutionId))
        {
            try
            {
                await _store.AppendAsync(group.Key, group.ToList(), _cts.Token).ConfigureAwait(false);
            }
            catch
            {
                // swallowing store errors keeps other executions flowing
            }
        }

        try
        {
            await exporter.ExportAsync(entries, _cts.Token).ConfigureAwait(false);
        }
        catch
        {
            // do not fail the sink if exporter fails
        }
    }

    private (string? executionId, Dictionary<string, object?>? properties) ExtractScope()
    {
        string? executionId = null;
        Dictionary<string, object?>? properties = null;

        _scopeProvider?.ForEachScope((scope, _) =>
        {
            if (scope is not IEnumerable<KeyValuePair<string, object?>> kvps)
            {
                return;
            }

            properties ??= new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);
            foreach (var kvp in kvps)
            {
                if (string.Equals(kvp.Key, "croniq.execution_id", StringComparison.OrdinalIgnoreCase))
                {
                    executionId ??= kvp.Value?.ToString();
                }

                properties[kvp.Key] = kvp.Value;
            }
        }, state: (object?)null);

        return (executionId, properties);
    }

    private static string? ExtractMessageTemplate(object? state)
    {
        if (state is IEnumerable<KeyValuePair<string, object?>> kvps)
        {
            foreach (var kvp in kvps)
            {
                if (string.Equals(kvp.Key, "{OriginalFormat}", StringComparison.OrdinalIgnoreCase) && kvp.Value is string template)
                {
                    return template;
                }
            }
        }

        return null;
    }

    private static string? TryGetCorrelation(IDictionary<string, object?>? properties)
    {
        if (properties is null) return null;
        if (properties.TryGetValue("croniq.correlation_id", out var correlation) && correlation is string text && !string.IsNullOrWhiteSpace(text))
        {
            return text;
        }

        return null;
    }

    private void TryCleanupSequences(DateTimeOffset nowUtc)
    {
        var interval = _options.SequenceCleanupInterval;
        if (interval <= TimeSpan.Zero)
        {
            return;
        }

        var nowTicks = nowUtc.UtcTicks;
        var lastTicks = Interlocked.Read(ref _lastSequenceCleanupTicks);
        if (nowTicks - lastTicks < interval.Ticks)
        {
            return;
        }

        if (Interlocked.CompareExchange(ref _lastSequenceCleanupTicks, nowTicks, lastTicks) != lastTicks)
        {
            return;
        }

        var retention = _options.SequenceRetention;
        if (retention <= TimeSpan.Zero)
        {
            return;
        }

        foreach (var entry in _sequences)
        {
            if (nowUtc - entry.Value.LastSeenUtc < retention)
            {
                continue;
            }

            _sequences.TryRemove(entry.Key, out _);
        }
    }

    private sealed record SequenceState(long Value, DateTimeOffset LastSeenUtc);

    private sealed class ExecutionLogger : ILogger
    {
        private readonly ExecutionLogSinkProvider _provider;
        private readonly string _categoryName;

        public ExecutionLogger(ExecutionLogSinkProvider provider, string categoryName)
        {
            _provider = provider;
            _categoryName = categoryName;
        }

        public IDisposable? BeginScope<TState>(TState state) where TState : notnull
        {
            return _provider._scopeProvider?.Push(state);
        }

        public bool IsEnabled(LogLevel logLevel) => logLevel != LogLevel.None;

        public void Log<TState>(LogLevel logLevel, EventId eventId, TState state, Exception? exception, Func<TState, Exception?, string> formatter)
        {
            if (formatter is null) throw new ArgumentNullException(nameof(formatter));
            _provider.TryEnqueue(logLevel, _categoryName, eventId, state, exception, (s, ex) => formatter((TState)s!, ex));
        }
    }
}
