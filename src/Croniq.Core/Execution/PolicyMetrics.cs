using System.Diagnostics;
using System.Diagnostics.Metrics;
using Croniq.Core.Jobs;
using Croniq.Core.Observability;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Execution;

internal static class PolicyMetrics
{
    private static readonly Meter Meter = new("Croniq.Core.Policy", "1.0.0");
    private static readonly Counter<long> RetryAttempts = Meter.CreateCounter<long>("cronipolicy_retry_attempts", description: "Number of retry attempts executed by the resilience pipeline.");
    private static readonly Counter<long> CircuitOpened = Meter.CreateCounter<long>("cronipolicy_circuit_open", description: "Number of times the execution circuit breaker entered the open state.");
    private static readonly Counter<long> DeadLetterTotal = Meter.CreateCounter<long>("cronipolicy_deadletter_total", description: "Number of executions routed to the dead-letter store.");

    public static void RecordRetry(JobKey jobKey, PartitionScope? scope = null)
    {
        RetryAttempts.Add(1, BuildTags(jobKey, scope));
    }

    public static void RecordCircuitOpened(JobKey jobKey, PartitionScope? scope = null)
    {
        CircuitOpened.Add(1, BuildTags(jobKey, scope));
    }

    public static void RecordDeadLetter(JobKey jobKey, string reason, PartitionScope? scope = null)
    {
        var tags = BuildTags(jobKey, scope);
        tags.Add("reason", string.IsNullOrWhiteSpace(reason) ? "unknown" : reason);
        DeadLetterTotal.Add(1, tags);
    }

    private static TagList BuildTags(JobKey jobKey, PartitionScope? scope)
    {
        var tags = new TagList
        {
            { "job", jobKey.Value },
            { "namespace", jobKey.NamespaceSegment },
            { "name", jobKey.JobName }
        };

        if (scope.HasValue)
        {
            tags.Add("tenant", IdentifierHashing.HashTenantId(scope.Value.TenantId) ?? string.Empty);
            tags.Add("env", scope.Value.EnvironmentTag);
        }

        if (!string.IsNullOrWhiteSpace(jobKey.Variant))
        {
            tags.Add("variant", jobKey.Variant!);
        }

        return tags;
    }
}
