using System;
using System.Collections.Generic;

namespace Croniq.Options;

public sealed class CroniqTriggerSeedDefinition
{
    public string? TriggerId { get; set; }

    public string JobKey { get; set; } = string.Empty;

    public string CronExpression { get; set; } = string.Empty;

    public DateTimeOffset? StartAtUtc { get; set; }

    public DateTimeOffset? EndAtUtc { get; set; }

    public bool Enabled { get; set; } = true;

    public Dictionary<string, string>? Metadata { get; set; }

    public string? Description { get; set; }

    public string? ManagedBy { get; set; }

    public string? TimeZoneId { get; set; }

    public string? CalendarId { get; set; }
}
