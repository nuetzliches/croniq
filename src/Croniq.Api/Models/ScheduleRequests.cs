using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record TriggerJobRequest(
    [property: Required] string JobKey,
    IDictionary<string, string>? Metadata = null,
    int? DelaySeconds = null,
    string? ExecutionMode = null);

public sealed record TriggerJobResponse(
    string Status,
    string JobKey);

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
    IReadOnlyDictionary<string, string>? Metadata,
    string? TimeZoneId,
    string? CalendarId = null);

public sealed record ScheduleDeadLetterResponse(
    long Id,
    string TriggerId,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    DateTimeOffset FireAtUtc,
    string Reason,
    string Payload,
    IReadOnlyDictionary<string, string>? Metadata,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset? ExpiresAtUtc);

public sealed record ScheduleUpsertResult(
    string TriggerId,
    string JobKey,
    string ScheduleExpression,
    string? CalendarId = null);

public sealed record ScheduleReplayResult(
    string Status,
    long Id,
    string JobKey,
    string TriggerId);
