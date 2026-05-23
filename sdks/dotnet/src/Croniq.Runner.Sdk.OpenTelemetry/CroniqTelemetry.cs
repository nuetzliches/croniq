namespace Croniq.Runner.Sdk.OpenTelemetry;

/// <summary>
/// Public constants for the Croniq runner's OpenTelemetry instrumentation.
/// Mirrors the names used internally by the SDK so consumers can attach
/// their own listeners or build dashboards without inspecting source.
/// </summary>
public static class CroniqTelemetry
{
    /// <summary>
    /// The <c>ActivitySource</c> name used by the runner. Pass to
    /// <c>TracerProviderBuilder.AddSource(...)</c>.
    /// </summary>
    public const string ActivitySourceName = "Croniq.Runner";

    /// <summary>
    /// The <c>Meter</c> name used by the runner. Pass to
    /// <c>MeterProviderBuilder.AddMeter(...)</c>.
    /// </summary>
    public const string MeterName = "Croniq.Runner";

    /// <summary>Semantic attribute keys (<c>croniq.*</c> namespace).</summary>
    public static class Attributes
    {
        public const string JobKey = "croniq.job.key";
        public const string ExecutionId = "croniq.execution.id";
        public const string ExecutionAttempt = "croniq.execution.attempt";
        public const string RunnerId = "croniq.runner.id";
        public const string RunnerTags = "croniq.runner.tags";
        public const string ExecutionTimeout = "croniq.execution.timeout";
        public const string Outcome = "croniq.execution.outcome";
    }

    /// <summary>Metric instrument names emitted by the runner.</summary>
    public static class Metrics
    {
        public const string ExecutionsCompleted = "croniq.runner.executions.completed";
        public const string ExecutionsFailed = "croniq.runner.executions.failed";
        public const string ExecutionDuration = "croniq.runner.executions.duration";
        public const string PollDuration = "croniq.runner.poll.duration";
        public const string ExecutionsInflight = "croniq.runner.executions.inflight";
    }
}
