using System;

namespace Croniq.Data.Postgres.Entities;

public sealed class RunnerEntity
{
    public long Id { get; set; }

    public string TenantId { get; set; } = string.Empty;

    public string EnvironmentTag { get; set; } = string.Empty;

    public string RunnerId { get; set; } = string.Empty;

    public DateTime LastSeenAtUtc { get; set; }

    public DateTime ExpiresAtUtc { get; set; }

    public string? MetadataJson { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
