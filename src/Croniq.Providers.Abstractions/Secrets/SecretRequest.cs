namespace Croniq.Providers.Secrets;

/// <summary>
/// Describes a secret lookup request.
/// </summary>
public sealed record SecretRequest(
    string Name,
    string? Version = null,
    string? Scope = null);
