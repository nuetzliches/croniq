using System;

namespace Croniq.Options;

public enum TenantMode
{
    Single,
    Multi
}

public sealed class CroniqOptions
{
    public const string SingleTenantId = "default";

    /// <summary>
    /// Controls how tenant scoping is resolved.
    /// Can be overridden via CRONIQ_CORE_TENANT_MODE (Single|Multi). Default is Single.
    /// </summary>
    public TenantMode TenantMode { get; set; } = TenantMode.Single;

    /// <summary>
    /// Required when <see cref="TenantMode"/> is <see cref="Croniq.Options.TenantMode.Multi"/>.
    /// </summary>
    public string? TenantId { get; set; }

    public string EnvironmentTag { get; set; } = "dev";

    public string InstanceId { get; set; } = Guid.NewGuid().ToString("N");

    public string GetEffectiveTenantId()
    {
        return TenantMode == TenantMode.Single
            ? SingleTenantId
            : (TenantId ?? string.Empty).Trim();
    }
}
