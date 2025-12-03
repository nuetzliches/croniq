using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Partition markers to isolate multi-tenant data in stores.
/// </summary>
public readonly record struct PartitionScope
{
    public PartitionScope(string tenantId, string environmentTag)
    {
        TenantId = Normalize(tenantId, nameof(tenantId));
        EnvironmentTag = Normalize(environmentTag, nameof(environmentTag));
    }

    public string TenantId { get; }

    public string EnvironmentTag { get; }

    private static string Normalize(string value, string paramName)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            throw new ArgumentException("Value cannot be null or whitespace.", paramName);
        }

        return value.Trim();
    }
}
