using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

namespace Croniq.Runner.Sdk.OpenTelemetry;

/// <summary>
/// Extensions that wire the Croniq runner's <c>ActivitySource</c> and
/// <c>Meter</c> into the consumer's OpenTelemetry pipeline.
/// </summary>
public static class OpenTelemetryRunnerExtensions
{
    /// <summary>
    /// Register the Croniq runner <c>ActivitySource</c> with the tracer.
    /// Usage:
    /// <code>
    /// builder.Services.AddOpenTelemetry()
    ///     .WithTracing(t => t.AddCroniqRunnerInstrumentation().AddOtlpExporter());
    /// </code>
    /// </summary>
    public static TracerProviderBuilder AddCroniqRunnerInstrumentation(this TracerProviderBuilder builder)
    {
        ArgumentNullException.ThrowIfNull(builder);
        return builder.AddSource(CroniqTelemetry.ActivitySourceName);
    }

    /// <summary>
    /// Register the Croniq runner <c>Meter</c> with the meter provider.
    /// Usage:
    /// <code>
    /// builder.Services.AddOpenTelemetry()
    ///     .WithMetrics(m => m.AddCroniqRunnerInstrumentation().AddOtlpExporter());
    /// </code>
    /// </summary>
    public static MeterProviderBuilder AddCroniqRunnerInstrumentation(this MeterProviderBuilder builder)
    {
        ArgumentNullException.ThrowIfNull(builder);
        return builder.AddMeter(CroniqTelemetry.MeterName);
    }
}
