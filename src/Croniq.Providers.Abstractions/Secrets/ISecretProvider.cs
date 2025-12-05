using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Providers.Secrets;

/// <summary>
/// Abstraction for retrieving secrets from an underlying store (Key Vault, Secrets Manager, etc.).
/// </summary>
public interface ISecretProvider
{
    /// <summary>
    /// Resolves a secret by name, optionally scoped or versioned. Returns null when the secret is missing.
    /// </summary>
    Task<SecretValue?> GetSecretAsync(SecretRequest request, CancellationToken cancellationToken = default);
}
