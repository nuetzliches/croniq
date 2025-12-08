using System.Diagnostics;
using System.Diagnostics.Metrics;
using Croniq.Core.Jobs;

namespace Croniq.Core.Execution;

internal static class SchedulerMetrics
{
    private static readonly Meter Meter = new("Croniq.Core.Scheduler", "1.0.0");
    private static readonly Counter<long> JobExecutions = Meter.CreateCounter<long>(
        "cronijob_executions_total",
        description: "Total number of job executions grouped by result.");
    private static readonly Histogram<double> JobExecutionDuration = Meter.CreateHistogram<double>(
        "cronijob_execution_duration_ms",
        unit: "ms",
        description: "Job execution duration in milliseconds.");
    private static readonly Counter<long> TriggerMisfires = Meter.CreateCounter<long>(
        "cronitrigger_misfires_total",
        description: "Number of trigger misfires evaluated by the scheduler.");
    private static readonly Counter<long> TriggerQuotaReschedules = Meter.CreateCounter<long>(
        "cronitrigger_quota_reschedules_total",
        description: "Number of triggers deferred due to quota enforcement.");
    private static readonly UpDownCounter<long> QueueDepth = Meter.CreateUpDownCounter<long>(
        "cronijob_queue_depth",
        description: "Active jobs currently being processed by the scheduler.");

    public static void RecordJobExecution(JobKey jobKey, bool succeeded, double durationMs)
    {
        var tags = BuildJobTags(jobKey);
        tags.Add("result", succeeded ? "success" : "failure");
        JobExecutions.Add(1, tags);
        JobExecutionDuration.Record(Math.Max(durationMs, 0d), tags);
    }

    public static void RecordMisfire(JobKey jobKey, string reason)
    {
        var tags = BuildJobTags(jobKey);
        tags.Add("reason", string.IsNullOrWhiteSpace(reason) ? "unknown" : reason);
        TriggerMisfires.Add(1, tags);
    }

    public static void RecordQuotaReschedule(JobKey jobKey)
    {
        TriggerQuotaReschedules.Add(1, BuildJobTags(jobKey));
    }

    public static void AdjustQueueDepth(JobKey jobKey, long delta)
    {
        QueueDepth.Add(delta, BuildScopeTags(jobKey));
    }

    private static TagList BuildJobTags(JobKey jobKey)
    {
        var tags = BuildScopeTags(jobKey);
        tags.Add("job", jobKey.Value);
        tags.Add("namespace", jobKey.NamespaceSegment);
        tags.Add("name", jobKey.JobName);
        if (!string.IsNullOrWhiteSpace(jobKey.Variant))
        {
            tags.Add("variant", jobKey.Variant!);
        }

        return tags;
    }

    private static TagList BuildScopeTags(JobKey jobKey)
    {
        var tags = new TagList
        {
            { "tenant", jobKey.TenantId },
            { "env", jobKey.EnvironmentTag }
        };

        return tags;
    }
}
