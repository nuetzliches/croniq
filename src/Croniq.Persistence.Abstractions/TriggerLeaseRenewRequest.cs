using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Request to renew a trigger lease.
/// </summary>
public sealed record TriggerLeaseRenewRequest(
    TriggerLease Lease,
    string InstanceId,
    DateTimeOffset NowUtc);
