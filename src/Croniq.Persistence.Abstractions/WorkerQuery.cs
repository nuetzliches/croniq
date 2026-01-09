using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WorkerQuery(
    PartitionScope Scope,
    DateTimeOffset NowUtc);
