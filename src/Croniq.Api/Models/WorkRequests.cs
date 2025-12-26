using System;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record WorkPollRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    int? BatchSize = null,
    int? WaitForMs = null);

public sealed record WorkLeaseToken(
    [property: Required] string LeaseId,
    [property: Required] string TriggerId,
    [property: Required] string JobKey,
    DateTimeOffset FireAtUtc,
    DateTimeOffset LeaseExpiresAtUtc,
    string? Payload);

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
