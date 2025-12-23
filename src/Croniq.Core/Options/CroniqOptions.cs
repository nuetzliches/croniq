using System;

namespace Croniq.Options;

public sealed class CroniqOptions
{
    public string TenantReference { get; set; } = "default";

    public string EnvironmentTag { get; set; } = "dev";

    public string InstanceId { get; set; } = Guid.NewGuid().ToString("N");
}
