using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Public job metadata persisted for registration and display.
/// </summary>
public sealed record JobDefinition(
    string JobKey,
    string Namespace,
    string Name,
    string? Variant,
    string? Description,
    IReadOnlyDictionary<string, string>? Metadata,
    bool IsActive = true);
