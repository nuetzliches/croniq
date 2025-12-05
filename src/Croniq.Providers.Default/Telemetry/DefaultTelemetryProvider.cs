using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Diagnostics.Metrics;
using Croniq.Providers.Telemetry;

namespace Croniq.Providers.Default.Telemetry;

/// <summary>
/// Default telemetry provider that caches <see cref="ActivitySource"/> and <see cref="Meter"/> instances.
/// </summary>
public sealed class DefaultTelemetryProvider : ITelemetryProvider, IDisposable
{
    private readonly ConcurrentDictionary<(string, string?), ActivitySource> _sources = new();
    private readonly ConcurrentDictionary<(string, string?), Meter> _meters = new();
    private bool _disposed;

    public ActivitySource GetActivitySource(string name, string? version = null)
    {
        if (_disposed) throw new ObjectDisposedException(nameof(DefaultTelemetryProvider));
        if (string.IsNullOrWhiteSpace(name)) throw new ArgumentException("Name is required.", nameof(name));

        return _sources.GetOrAdd((name, version), key => new ActivitySource(key.Item1, key.Item2));
    }

    public Meter GetMeter(string name, string? version = null)
    {
        if (_disposed) throw new ObjectDisposedException(nameof(DefaultTelemetryProvider));
        if (string.IsNullOrWhiteSpace(name)) throw new ArgumentException("Name is required.", nameof(name));

        return _meters.GetOrAdd((name, version), key => new Meter(key.Item1, key.Item2));
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        foreach (var meter in _meters.Values)
        {
            meter.Dispose();
        }
    }
}
