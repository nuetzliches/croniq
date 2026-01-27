using System;
using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record WorkPollRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    int? BatchSize = null,
    int? WaitForMs = null,
    bool? AllowTestExecutions = null,
    int? MaxInflight = null,
    string[]? Capabilities = null);

public sealed record WorkLeaseToken(
    [property: Required] string ExecutionId,
    [property: Required] string LeaseId,
    [property: Required] string TriggerId,
    [property: Required] string JobKey,
    DateTimeOffset FireAtUtc,
    DateTimeOffset LeaseExpiresAtUtc,
    string? Payload,
    string ExecutionMode = Persistence.Abstractions.ExecutionIntent.ExecutionModes.Normal,
    string InvocationSource = Persistence.Abstractions.ExecutionIntent.InvocationSources.Schedule);

public sealed record WorkPollResponse(
    WorkLeaseToken[] Leases);

public sealed record WorkRenewRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    [property: Required] WorkLeaseToken Lease);

public sealed record WorkRenewResponse(
    bool Renewed,
    WorkLeaseToken? Lease);

public sealed record WorkAckRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    [property: Required] WorkLeaseToken Lease,
    bool Succeeded,
    DateTimeOffset? NextFireTimeUtc = null,
    string? DeadLetterReason = null);

public sealed record WorkEventsRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    [property: Required] WorkLeaseToken Lease,
    WorkEventEntry[]? Events);

public sealed record WorkEventEntry(
    [property: Required] string Message,
    string? Level = null,
    DateTimeOffset? TimestampUtc = null,
    Dictionary<string, string>? Properties = null,
    string? EventType = null);
