using System;

namespace Croniq.Persistence.Abstractions;

public sealed record RunnerStatus(
    string RunnerId,
    DateTimeOffset LastSeenAtUtc,
    DateTimeOffset ExpiresAtUtc,
    bool IsOnline,
    string? MetadataJson = null);
