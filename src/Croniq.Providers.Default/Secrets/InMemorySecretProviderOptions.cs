using System.Collections.Generic;

namespace Croniq.Providers.Default.Secrets;

/// <summary>
/// Options to configure the in-memory secret provider.
/// </summary>
public sealed class InMemorySecretProviderOptions
{
    /// <summary>
    /// Seed secrets for development and testing.
    /// </summary>
    public IDictionary<string, string> Secrets { get; } = new Dictionary<string, string>();
}
