using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WorkerHeartbeat(
    PartitionScope Scope,
    string InstanceId,
    DateTimeOffset SeenAtUtc,
    string? MetadataJson = null);
