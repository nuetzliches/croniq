using System;

namespace Croniq.Persistence.Abstractions;

public sealed record RunnerQuery(
    PartitionScope Scope,
    DateTimeOffset NowUtc);
