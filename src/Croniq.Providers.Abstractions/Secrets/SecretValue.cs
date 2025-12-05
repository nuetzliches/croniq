using System;

namespace Croniq.Providers.Secrets;

/// <summary>
/// Returned secret payload with optional metadata.
/// </summary>
public sealed record SecretValue(
    string Value,
    DateTimeOffset? ExpiresAtUtc = null,
    string? Version = null);
