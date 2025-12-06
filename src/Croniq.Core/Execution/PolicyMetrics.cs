using System.Diagnostics;
using System.Diagnostics.Metrics;
using Croniq.Core.Jobs;

namespace Croniq.Core.Execution;

internal static class PolicyMetrics
{
    private static readonly Meter Meter = new("Croniq.Core.Policy", "1.0.0");
    private static readonly Counter<long> RetryAttempts = Meter.CreateCounter<long>("cronipolicy.retry_attempts", description: "Number of retry attempts executed by the resilience pipeline.");
    private static readonly Counter<long> CircuitOpened = Meter.CreateCounter<long>("cronipolicy.circuit_open", description: "Number of times the execution circuit breaker entered the open state.");
    private static readonly Counter<long> DeadLetterTotal = Meter.CreateCounter<long>("cronipolicy.deadletter_total", description: "Number of executions routed to the dead-letter store.");

    public static void RecordRetry(JobKey jobKey)
    {
        RetryAttempts.Add(1, BuildTags(jobKey));
    }

    public static void RecordCircuitOpened(JobKey jobKey)
    {
        CircuitOpened.Add(1, BuildTags(jobKey));
    }

    public static void RecordDeadLetter(JobKey jobKey, string reason)
    {
        var tags = BuildTags(jobKey);
        tags.Add("reason", string.IsNullOrWhiteSpace(reason) ? "unknown" : reason);
        DeadLetterTotal.Add(1, tags);
    }

    private static TagList BuildTags(JobKey jobKey)
    {
        var tags = new TagList
        {
            { "job", jobKey.Value },
            { "tenant", jobKey.TenantId },
            { "env", jobKey.EnvironmentTag },
            { "namespace", jobKey.NamespaceSegment },
            { "name", jobKey.JobName }
        };

        if (!string.IsNullOrWhiteSpace(jobKey.Variant))
        {
            tags.Add("variant", jobKey.Variant!);
        }

        return tags;
    }
}
