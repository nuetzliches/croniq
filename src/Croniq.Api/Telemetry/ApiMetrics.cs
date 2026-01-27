using System.Collections.Concurrent;
using System.Diagnostics;
using System.Diagnostics.Metrics;
using Croniq.Core.Jobs;
using Croniq.Core.Observability;
using Croniq.Persistence.Abstractions;

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
    private static readonly Counter<long> RunnerTransportSelections = Meter.CreateCounter<long>(
        "croniq.api.runner.transport.selections_total",
        description: "Number of runner transport selections observed by the API.");
    private static readonly Counter<long> RunnerTransportTransitions = Meter.CreateCounter<long>(
        "croniq.api.runner.transport.transitions_total",
        description: "Number of runner transport transitions observed by the API.");
    private static readonly Counter<long> RunnerTestDecisions = Meter.CreateCounter<long>(
        "croniq.api.runner.test.decisions_total",
        description: "Number of test execution decisions (accepted/rejected) observed by the API.");
    private static readonly ConcurrentDictionary<string, string> RunnerTransportState = new(StringComparer.OrdinalIgnoreCase);

    public static void RecordScheduleUpsert(string tenantId, string environmentTag, string jobKey)
    {
        var tags = new TagList
        {
            { "tenant", IdentifierHashing.HashTenantId(tenantId) ?? string.Empty },
            { "env", environmentTag },
            { "job", jobKey }
        };

        ScheduleUpserts.Add(1, tags);
    }

    public static void RecordManualTrigger(JobKey jobKey, PartitionScope scope)
    {
        ManualTriggers.Add(1, BuildJobTags(jobKey, scope));
    }

    public static string? RecordRunnerTransportSelection(
        string tenantId,
        string environmentTag,
        string runnerId,
        string transport)
    {
        if (string.IsNullOrWhiteSpace(tenantId)
            || string.IsNullOrWhiteSpace(environmentTag)
            || string.IsNullOrWhiteSpace(runnerId)
            || string.IsNullOrWhiteSpace(transport))
        {
            return null;
        }

        var tags = new TagList
        {
            { "tenant", IdentifierHashing.HashTenantId(tenantId) ?? string.Empty },
            { "env", environmentTag },
            { "transport", transport }
        };

        RunnerTransportSelections.Add(1, tags);

        var key = BuildRunnerKey(tenantId, environmentTag, runnerId);
        RunnerTransportState.TryGetValue(key, out var previous);
        RunnerTransportState[key] = transport;
        return previous;
    }

    public static void RecordRunnerTransportTransition(
        string tenantId,
        string environmentTag,
        string fromTransport,
        string toTransport)
    {
        if (string.IsNullOrWhiteSpace(tenantId)
            || string.IsNullOrWhiteSpace(environmentTag)
            || string.IsNullOrWhiteSpace(fromTransport)
            || string.IsNullOrWhiteSpace(toTransport))
        {
            return;
        }

        var tags = new TagList
        {
            { "tenant", IdentifierHashing.HashTenantId(tenantId) ?? string.Empty },
            { "env", environmentTag },
            { "from", fromTransport },
            { "to", toTransport }
        };

        RunnerTransportTransitions.Add(1, tags);
    }

    public static void RecordRunnerTestDecision(
        string tenantId,
        string environmentTag,
        string transport,
        string decision,
        string? executionMode,
        string? invocationSource)
    {
        if (string.IsNullOrWhiteSpace(tenantId)
            || string.IsNullOrWhiteSpace(environmentTag)
            || string.IsNullOrWhiteSpace(transport)
            || string.IsNullOrWhiteSpace(decision))
        {
            return;
        }

        var tags = new TagList
        {
            { "tenant", IdentifierHashing.HashTenantId(tenantId) ?? string.Empty },
            { "env", environmentTag },
            { "transport", transport },
            { "decision", decision }
        };

        if (!string.IsNullOrWhiteSpace(executionMode))
        {
            tags.Add("execution_mode", executionMode!);
        }

        if (!string.IsNullOrWhiteSpace(invocationSource))
        {
            tags.Add("invocation_source", invocationSource!);
        }

        RunnerTestDecisions.Add(1, tags);
    }

    private static string BuildRunnerKey(string tenantId, string environmentTag, string runnerId)
        => $"{tenantId}::{environmentTag}::{runnerId}";

    private static TagList BuildJobTags(JobKey jobKey, PartitionScope scope)
    {
        var tags = new TagList
        {
            { "tenant", IdentifierHashing.HashTenantId(scope.TenantId) ?? string.Empty },
            { "env", scope.EnvironmentTag },
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
