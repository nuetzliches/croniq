using System.Collections.Generic;
using Croniq.Persistence.Abstractions;

namespace Croniq.Options;

public sealed class CroniqCalendarSeedDefinition
{
    public string? CalendarId { get; set; }

    public string Name { get; set; } = string.Empty;

    public string? Description { get; set; }

    public string TimeZoneId { get; set; } = string.Empty;

    public CalendarMode Mode { get; set; }

    public bool Enabled { get; set; } = true;

    public List<CalendarRuleDefinition> Rules { get; set; } = new();
}
