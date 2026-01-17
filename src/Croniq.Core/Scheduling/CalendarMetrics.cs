using System.Collections.Generic;
using System.Diagnostics.Metrics;
using Croniq.Core.Observability;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Scheduling;

internal static class CalendarMetrics
{
    private static readonly Meter Meter = new("Croniq.Core.Calendar", "1.0.0");
    private static readonly Counter<long> Evaluations = Meter.CreateCounter<long>(
        "cronicalendar_evaluations_total",
        description: "Calendar evaluations grouped by result.");
    private static readonly Counter<long> SkippedOccurrences = Meter.CreateCounter<long>(
        "cronicalendar_skipped_occurrences_total",
        description: "Number of schedule candidates skipped by calendar filtering.");
    private static readonly Histogram<double> EvaluationDuration = Meter.CreateHistogram<double>(
        "cronicalendar_evaluation_duration_ms",
        unit: "ms",
        description: "Calendar evaluation duration in milliseconds.");

    public static void RecordEvaluation(CalendarMode mode, string result, double durationMs, PartitionScope? scope = null)
    {
        var tags = BuildTags(mode, scope, result);
        Evaluations.Add(1, tags);
        EvaluationDuration.Record(Math.Max(durationMs, 0d), tags);
    }

    public static void RecordSkipped(CalendarMode mode, int skipped, PartitionScope? scope = null)
    {
        if (skipped <= 0)
        {
            return;
        }

        SkippedOccurrences.Add(skipped, BuildTags(mode, scope));
    }

    private static KeyValuePair<string, object?>[] BuildTags(CalendarMode mode, PartitionScope? scope, string? result = null)
    {
        var tags = BuildScopeTags(scope);
        tags.Add(new KeyValuePair<string, object?>("calendar_mode", mode == CalendarMode.Include ? "include" : "exclude"));
        if (!string.IsNullOrWhiteSpace(result))
        {
            tags.Add(new KeyValuePair<string, object?>("result", result));
        }

        return tags.ToArray();
    }

    private static List<KeyValuePair<string, object?>> BuildScopeTags(PartitionScope? scope)
    {
        var tags = new List<KeyValuePair<string, object?>>(3);
        if (!scope.HasValue)
        {
            return tags;
        }

        tags.Add(new KeyValuePair<string, object?>("tenant", IdentifierHashing.HashTenantId(scope.Value.TenantId) ?? string.Empty));
        tags.Add(new KeyValuePair<string, object?>("env", scope.Value.EnvironmentTag));
        return tags;
    }
}
