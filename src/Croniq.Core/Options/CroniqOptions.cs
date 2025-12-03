using System;

namespace Croniq.Core.Options;

public sealed class CroniqOptions
{
    public string TenantId { get; set; } = "default";

    public string EnvironmentTag { get; set; } = "dev";

    public string InstanceId { get; set; } = Guid.NewGuid().ToString("N");
}
