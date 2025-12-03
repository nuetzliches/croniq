using System;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Policies;

public interface IMisfirePolicy
{
    MisfireDecision Evaluate(TriggerLease lease, MisfirePolicyOptions options, DateTimeOffset now);
}

public sealed record MisfireDecision(bool IsMisfire, string? Reason);
