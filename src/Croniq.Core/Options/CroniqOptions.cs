using System;

namespace Croniq.Options;

public enum TenantMode
{
    Single,
    Multi
}

public sealed class CroniqOptions
{
    /// <summary>
    /// Informational flag for tenant mode.
    /// Does not change tenant id resolution.
    /// Can be overridden via CRONIQ_CORE_TENANT_MODE (Single|Multi). Default is Single.
    /// </summary>
    public TenantMode TenantMode { get; set; } = TenantMode.Single;

    /// <summary>
    /// Tenant id used for scoping.
    /// Can be overridden via CRONIQ_CORE_TENANT_ID. Default is "default".
    /// </summary>
    public string TenantId { get; set; } = "default";

    public string EnvironmentTag { get; set; } = "dev";

    public string InstanceId { get; set; } = Guid.NewGuid().ToString("N");
}
