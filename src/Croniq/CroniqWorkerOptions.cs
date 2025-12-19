using System;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq;

public sealed class CroniqWorkerOptions
{
    public string? TenantId { get; set; }

    public string? EnvironmentTag { get; set; }

    public string? InstanceId { get; set; }

    public int? BatchSize { get; set; }

    public TimeSpan? IdleDelay { get; set; }

    public TimeSpan? BusyDelay { get; set; }

    public TimeSpan? ErrorDelay { get; set; }

    public int? InMemoryLeaseDurationSeconds { get; set; }

    public Action<IServiceCollection>? ConfigureServices { get; set; }
}

