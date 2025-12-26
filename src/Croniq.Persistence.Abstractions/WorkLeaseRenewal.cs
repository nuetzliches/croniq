using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WorkLeaseRenewal(
    string LeaseId,
    string RunnerId,
    DateTimeOffset LeaseExpiresAtUtc,
    DateTimeOffset RenewedAtUtc,
    string? ExecutionId = null);
