using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WorkerStatus(
    string InstanceId,
    DateTimeOffset LastSeenAtUtc,
    DateTimeOffset ExpiresAtUtc,
    bool IsOnline,
    string? MetadataJson = null);
