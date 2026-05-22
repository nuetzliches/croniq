using System.Diagnostics;
using System.Diagnostics.Metrics;
using System.Reflection;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Core instrumentation: a process-wide <see cref="ActivitySource"/> and
/// <see cref="Meter"/> used by the runner regardless of whether the
/// optional OpenTelemetry package is installed. Consumers register them
/// via <c>AddCroniqRunnerInstrumentation()</c> from the
/// <c>Croniq.Runner.Sdk.OpenTelemetry</c> package — without a listener,
/// the calls have near-zero cost.
/// </summary>
internal static class CroniqInstrumentation
{
    /// <summary>ActivitySource name used by the runner.</summary>
    public const string ActivitySourceName = "Croniq.Runner";

    /// <summary>Meter name used by the runner.</summary>
    public const string MeterName = "Croniq.Runner";

    private static readonly string Version =
        typeof(CroniqInstrumentation).Assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion
        ?? "0.0.0";

    public static readonly ActivitySource ActivitySource = new(ActivitySourceName, Version);
    public static readonly Meter Meter = new(MeterName, Version);

    public static readonly Counter<long> ExecutionsCompleted = Meter.CreateCounter<long>(
        "croniq.runner.executions.completed",
        unit: "{executions}",
        description: "Number of executions completed (success + failure).");

    public static readonly Counter<long> ExecutionsFailed = Meter.CreateCounter<long>(
        "croniq.runner.executions.failed",
        unit: "{executions}",
        description: "Number of executions that ended in failure.");

    public static readonly Histogram<double> ExecutionDuration = Meter.CreateHistogram<double>(
        "croniq.runner.executions.duration",
        unit: "ms",
        description: "Wall-clock execution duration in milliseconds.");

    public static readonly Histogram<double> PollDuration = Meter.CreateHistogram<double>(
        "croniq.runner.poll.duration",
        unit: "ms",
        description: "Latency of a single /v1/work/poll call.");

    public static readonly UpDownCounter<int> ExecutionsInflight = Meter.CreateUpDownCounter<int>(
        "croniq.runner.executions.inflight",
        unit: "{executions}",
        description: "Currently in-flight executions on this runner.");
}

/// <summary>Semantic attribute names used on Croniq runner spans and metrics.</summary>
internal static class CroniqAttributes
{
    public const string JobKey = "croniq.job.key";
    public const string ExecutionId = "croniq.execution.id";
    public const string ExecutionAttempt = "croniq.execution.attempt";
    public const string RunnerId = "croniq.runner.id";
    public const string RunnerTags = "croniq.runner.tags";
    public const string ExecutionTimeout = "croniq.execution.timeout";
    public const string Outcome = "croniq.execution.outcome";
}
