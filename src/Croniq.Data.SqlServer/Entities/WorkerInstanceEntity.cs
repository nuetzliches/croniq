using System;

namespace Croniq.Data.SqlServer.Entities;

public sealed class WorkerInstanceEntity
{
    public long Id { get; set; }

    public string TenantId { get; set; } = string.Empty;

    public string EnvironmentTag { get; set; } = string.Empty;

    public string InstanceId { get; set; } = string.Empty;

    public DateTime LastSeenAtUtc { get; set; }

    public DateTime ExpiresAtUtc { get; set; }

    public string? MetadataJson { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
