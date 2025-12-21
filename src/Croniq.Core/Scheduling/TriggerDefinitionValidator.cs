using System;
using System.Security.Cryptography;
using System.Text;
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
        var timeZoneId = NormalizeTimeZoneId(definition.TimeZoneId, out var timeZoneError);
        if (timeZoneError is not null)
        {
            error = timeZoneError;
            return false;
        }

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
            ? BuildTriggerId(jobKey.Value, cronExpression, timeZoneId)
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
            timeZoneId,
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

    private static string BuildTriggerId(string jobKey, string cronExpression, string? timeZoneId)
    {
        const int maxTriggerIdLength = 512;
        var encodedCron = EncodeSegment(cronExpression);
        var candidate = string.IsNullOrWhiteSpace(timeZoneId)
            ? $"{jobKey}:{encodedCron}"
            : $"{jobKey}:{encodedCron}:{EncodeSegment(timeZoneId!)}";

        if (candidate.Length <= maxTriggerIdLength)
        {
            return candidate;
        }

        var hash = ComputeStableHash($"{cronExpression}|{timeZoneId}");
        return $"{jobKey}:hash-{hash}";
    }

    private static string EncodeSegment(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var encoded = Convert.ToBase64String(bytes);
        return encoded.TrimEnd('=').Replace('+', '-').Replace('/', '_');
    }

    private static string ComputeStableHash(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    private static string? NormalizeTimeZoneId(string? timeZoneId, out string? error)
    {
        error = null;
        if (string.IsNullOrWhiteSpace(timeZoneId))
        {
            return null;
        }

        var trimmed = timeZoneId.Trim();
        if (!TimeZoneUtil.TryFindTimeZoneById(trimmed, out var resolved))
        {
            error = $"TimeZoneId '{trimmed}' is invalid.";
            return null;
        }

        return resolved!.Id;
    }
}

public sealed record TriggerDefinitionValidationResult(
    JobKey JobKey,
    string TriggerId,
    string ScheduleExpression,
    DateTimeOffset? StartAtUtc,
    DateTimeOffset? EndAtUtc,
    string? TimeZoneId,
    string Summary);
