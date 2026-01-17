using System;
using System.Collections.Generic;
using Croniq.Persistence.Abstractions;

namespace Croniq.Api.Models;

public sealed record CalendarResponse(
    string CalendarId,
    string TenantId,
    string EnvironmentTag,
    string Name,
    string? Description,
    string TimeZoneId,
    CalendarMode Mode,
    IReadOnlyCollection<CalendarRuleDefinition> Rules,
    bool Enabled,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc);

public sealed record CalendarUpsertResult(
    string CalendarId,
    string Name);
