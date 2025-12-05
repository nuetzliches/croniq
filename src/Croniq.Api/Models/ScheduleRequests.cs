using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record UpsertScheduleRequest(
    [property: Required] string JobKey,
    [property: Required] string CronExpression,
    string? TriggerId,
    DateTimeOffset? StartAtUtc,
    DateTimeOffset? EndAtUtc,
    bool Enabled = true,
    string? Description = null,
    IDictionary<string, string>? Metadata = null);

public sealed record TriggerJobRequest(
    [property: Required] string JobKey,
    IDictionary<string, string>? Metadata = null);
