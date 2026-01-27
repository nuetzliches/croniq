using System;

namespace Croniq.Persistence.Abstractions;

public sealed record RunnerLookup(
    PartitionScope Scope,
    string RunnerId,
    DateTimeOffset NowUtc);
