using System.Collections.Generic;
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

public sealed record JobResponse(
    string JobKey,
    string Namespace,
    string Name,
    string? Variant,
    string? Description,
    IReadOnlyDictionary<string, string>? Metadata);

public sealed record UpsertJobRequest(
    [property: Required] string JobKey,
    [property: Required] string Namespace,
    [property: Required] string Name,
    string? Variant,
    string? Description,
    IDictionary<string, string>? Metadata = null);

public sealed record ScheduleResponse(
    string TriggerId,
    string JobKey,
    string CronExpression,
    string TenantId,
    string EnvironmentTag,
    DateTimeOffset? StartAtUtc,
    DateTimeOffset? EndAtUtc,
    bool Enabled,
    IReadOnlyDictionary<string, string>? Metadata);
