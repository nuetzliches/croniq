using System;

namespace Croniq.Persistence.Abstractions;

public sealed record RunnerHeartbeat(
    PartitionScope Scope,
    string RunnerId,
    DateTimeOffset SeenAtUtc,
    string? MetadataJson = null);
