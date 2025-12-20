using System;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Scheduling;

public static class TriggerDefinitionValidator
{
    public static bool TryValidate(
        CroniqTriggerSeedDefinition definition,
        PartitionScope? scope,
        out TriggerDefinitionValidationResult result,
        out string? error)
    {
        result = default!;
        error = null;

        var jobKeyValue = definition.JobKey?.Trim();
        if (string.IsNullOrWhiteSpace(jobKeyValue))
        {
            error = "JobKey is required.";
            return false;
        }

        if (!JobKey.TryParse(jobKeyValue, out var jobKey))
        {
            error = $"JobKey '{jobKeyValue}' is invalid.";
            return false;
        }

        if (scope.HasValue && !MatchesScope(jobKey, scope.Value))
        {
            error = $"JobKey '{jobKeyValue}' must match tenant '{scope.Value.TenantId}' and environment '{scope.Value.EnvironmentTag}'.";
            return false;
        }

        var cronExpression = definition.CronExpression?.Trim();
        if (string.IsNullOrWhiteSpace(cronExpression))
        {
            error = "CronExpression is required.";
            return false;
        }

        var startAtUtc = definition.StartAtUtc?.ToUniversalTime();
        var endAtUtc = definition.EndAtUtc?.ToUniversalTime();

        string summary;
        if (TriggerSchedule.IsOnceExpression(cronExpression))
        {
            summary = BuildOnceSummary(startAtUtc);
        }
        else
        {
            try
            {
                var cron = new CronExpression(cronExpression);
                summary = NormalizeSummary(cron.GetExpressionSummary());
            }
            catch (Exception ex)
            {
                error = $"CronExpression '{cronExpression}' is invalid ({ex.Message}).";
                return false;
            }
        }

        if (startAtUtc.HasValue && endAtUtc.HasValue && startAtUtc.Value > endAtUtc.Value)
        {
            error = "StartAtUtc must be before EndAtUtc.";
            return false;
        }

        var triggerId = string.IsNullOrWhiteSpace(definition.TriggerId)
            ? $"{jobKey.Value}:{cronExpression}"
            : definition.TriggerId.Trim();

        if (string.IsNullOrWhiteSpace(triggerId))
        {
            error = "TriggerId is required.";
            return false;
        }

        result = new TriggerDefinitionValidationResult(
            jobKey,
            triggerId,
            cronExpression,
            startAtUtc,
            endAtUtc,
            summary);

        return true;
    }

    private static bool MatchesScope(JobKey jobKey, PartitionScope scope)
    {
        return string.Equals(jobKey.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(jobKey.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static string NormalizeSummary(string summary)
    {
        if (string.IsNullOrWhiteSpace(summary))
        {
            return "cron summary unavailable";
        }

        return summary
            .Replace("\r", string.Empty, StringComparison.Ordinal)
            .Replace("\n", "; ", StringComparison.Ordinal)
            .Trim();
    }

    private static string BuildOnceSummary(DateTimeOffset? startAtUtc)
    {
        if (startAtUtc.HasValue)
        {
            return $"once at {startAtUtc.Value:O}";
        }

        return "once";
    }
}

public sealed record TriggerDefinitionValidationResult(
    JobKey JobKey,
    string TriggerId,
    string ScheduleExpression,
    DateTimeOffset? StartAtUtc,
    DateTimeOffset? EndAtUtc,
    string Summary);
