using System;

namespace Croniq.Core.Jobs;

public readonly record struct JobKey
{
    public JobKey(string tenantId, string environmentTag, string namespaceSegment, string jobName, string? variant = null)
    {
        TenantId = Normalize(tenantId, nameof(tenantId));
        EnvironmentTag = Normalize(environmentTag, nameof(environmentTag));
        NamespaceSegment = Normalize(namespaceSegment, nameof(namespaceSegment));
        JobName = Normalize(jobName, nameof(jobName));
        Variant = variant?.Trim();
        Value = Variant is null
            ? $"{TenantId}:{EnvironmentTag}:{NamespaceSegment}:{JobName}"
            : $"{TenantId}:{EnvironmentTag}:{NamespaceSegment}:{JobName}:{Variant}";
    }

    public string TenantId { get; }

    public string EnvironmentTag { get; }

    public string NamespaceSegment { get; }

    public string JobName { get; }

    public string? Variant { get; }

    public string Value { get; }

    public override string ToString() => Value;

    public static JobKey Create(string tenantId, string environmentTag, string namespaceSegment, string jobName, string? variant = null)
    {
        return new JobKey(tenantId, environmentTag, namespaceSegment, jobName, variant);
    }

    public static bool TryParse(string value, out JobKey jobKey)
    {
        jobKey = default;
        if (string.IsNullOrWhiteSpace(value))
        {
            return false;
        }

        var parts = value.Split(':');
        if (parts.Length is < 4 or > 5)
        {
            return false;
        }

        var variant = parts.Length == 5 ? parts[4] : null;
        try
        {
            jobKey = new JobKey(parts[0], parts[1], parts[2], parts[3], variant);
            return true;
        }
        catch
        {
            return false;
        }
    }

    private static string Normalize(string value, string paramName)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            throw new ArgumentException("Value cannot be null or whitespace.", paramName);
        }

        return value.Trim();
    }
}
