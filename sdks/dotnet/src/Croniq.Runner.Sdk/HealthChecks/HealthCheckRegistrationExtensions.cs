using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Diagnostics.HealthChecks;

namespace Croniq.Runner.Sdk.HealthChecks;

/// <summary>Extension methods for registering the Croniq runner health check.</summary>
public static class HealthCheckRegistrationExtensions
{
    /// <summary>Register the runner health check. Requires the runner to be registered via <c>AddCroniqRunner(...)</c> beforehand.</summary>
    public static IHealthChecksBuilder AddCroniqRunnerHealthCheck(
        this IHealthChecksBuilder builder,
        string name = "croniq-runner",
        HealthStatus failureStatus = HealthStatus.Unhealthy,
        IEnumerable<string>? tags = null)
    {
        return builder.AddCheck<CroniqRunnerHealthCheck>(name, failureStatus, tags ?? []);
    }
}
