using System.Diagnostics;
using System.Diagnostics.Metrics;

namespace Croniq.Providers.Telemetry;

/// <summary>
/// Abstraction for emitting traces and metrics.
/// </summary>
public interface ITelemetryProvider
{
    ActivitySource GetActivitySource(string name, string? version = null);

    Meter GetMeter(string name, string? version = null);
}
