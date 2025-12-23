using System;

namespace Croniq.Core.Jobs;

public readonly record struct JobKey
{
    public JobKey(string namespaceSegment, string jobName, string? variant = null)
    {
        NamespaceSegment = Normalize(namespaceSegment, nameof(namespaceSegment));
        JobName = Normalize(jobName, nameof(jobName));
        Variant = string.IsNullOrWhiteSpace(variant) ? null : variant.Trim();
        Value = Variant is null
            ? $"{NamespaceSegment}:{JobName}"
            : $"{NamespaceSegment}:{JobName}:{Variant}";
    }

    public string NamespaceSegment { get; }

    public string JobName { get; }

    public string? Variant { get; }

    public string Value { get; }

    public override string ToString() => Value;

    public static JobKey Create(string namespaceSegment, string jobName, string? variant = null)
    {
        return new JobKey(namespaceSegment, jobName, variant);
    }

    public static bool TryParse(string value, out JobKey jobKey)
    {
        jobKey = default;
        if (string.IsNullOrWhiteSpace(value))
        {
            return false;
        }

        var parts = value.Split(':');
        if (parts.Length is < 2 or > 3)
        {
            return false;
        }

        // Format: namespace:name[:variant]
        var namespaceSegment = parts[0];
        var jobName = parts[1];
        var variant = parts.Length == 3 ? parts[2] : null;
        try
        {
            jobKey = new JobKey(namespaceSegment, jobName, variant);
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
