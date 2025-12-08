using System.Diagnostics;
using System.Diagnostics.Metrics;
using Croniq.Core.Jobs;

namespace Croniq.Api.Telemetry;

internal static class ApiMetrics
{
    private static readonly Meter Meter = new("Croniq.Api", "1.0.0");
    private static readonly Counter<long> ScheduleUpserts = Meter.CreateCounter<long>(
        "cronigateway_schedule_upserts_total",
        description: "Number of schedules created or updated via the Croniq API.");
    private static readonly Counter<long> ManualTriggers = Meter.CreateCounter<long>(
        "cronigateway_manual_triggers_total",
        description: "Number of manual job trigger requests executed through the Croniq API.");

    public static void RecordScheduleUpsert(string tenantId, string environmentTag, string jobKey)
    {
        var tags = new TagList
        {
            { "tenant", tenantId },
            { "env", environmentTag },
            { "job", jobKey }
        };

        ScheduleUpserts.Add(1, tags);
    }

    public static void RecordManualTrigger(JobKey jobKey)
    {
        ManualTriggers.Add(1, BuildJobTags(jobKey));
    }

    private static TagList BuildJobTags(JobKey jobKey)
    {
        var tags = new TagList
        {
            { "tenant", jobKey.TenantId },
            { "env", jobKey.EnvironmentTag },
            { "job", jobKey.Value },
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
