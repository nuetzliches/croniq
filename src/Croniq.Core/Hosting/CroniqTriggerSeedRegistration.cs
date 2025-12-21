using System;
using System.Collections.Generic;
using Croniq.Sdk;

namespace Croniq.Core.Hosting;

public sealed class CroniqTriggerSeedRegistration
{
    public CroniqTriggerSeedRegistration(CroniqJobAttribute jobAttribute, string cronExpression)
    {
        JobAttribute = jobAttribute ?? throw new ArgumentNullException(nameof(jobAttribute));
        CronExpression = string.IsNullOrWhiteSpace(cronExpression)
            ? throw new ArgumentException("Cron expression is required.", nameof(cronExpression))
            : cronExpression;
    }

    public CroniqJobAttribute JobAttribute { get; }

    public string? TriggerId { get; set; }

    public string CronExpression { get; set; }

    public DateTimeOffset? StartAtUtc { get; set; }

    public DateTimeOffset? EndAtUtc { get; set; }

    public bool Enabled { get; set; } = true;

    public Dictionary<string, string>? Metadata { get; set; }

    public string? ManagedBy { get; set; }

    public string? TimeZoneId { get; set; }
}
