using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Providers.Secrets;
using Microsoft.Extensions.Options;

namespace Croniq.Providers.Default.Secrets;

/// <summary>
/// Simple in-memory secret provider for development and tests.
/// </summary>
public sealed class InMemorySecretProvider : ISecretProvider
{
    private readonly InMemorySecretProviderOptions _options;

    public InMemorySecretProvider(IOptions<InMemorySecretProviderOptions> options)
    {
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
    }

    public Task<SecretValue?> GetSecretAsync(SecretRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        cancellationToken.ThrowIfCancellationRequested();

        if (TryResolve(request, out var value))
        {
            return Task.FromResult<SecretValue?>(new SecretValue(value!, null, request.Version));
        }

        return Task.FromResult<SecretValue?>(null);
    }

    private bool TryResolve(SecretRequest request, out string? value)
    {
        value = null;
        if (string.IsNullOrWhiteSpace(request.Name))
        {
            return false;
        }

        // Prefer scoped name "scope/name" if provided.
        if (!string.IsNullOrWhiteSpace(request.Scope))
        {
            var scopedKey = $"{request.Scope.TrimEnd('/')}/{request.Name}";
            if (_options.Secrets.TryGetValue(scopedKey, out var scoped) && !string.IsNullOrWhiteSpace(scoped))
            {
                value = scoped;
                return true;
            }
        }

        // Fallback to plain name.
        if (_options.Secrets.TryGetValue(request.Name, out var plain) && !string.IsNullOrWhiteSpace(plain))
        {
            value = plain;
            return true;
        }

        // Last resort: environment variable (upper snake).
        var envKey = (request.Scope is null ? request.Name : $"{request.Scope}_{request.Name}")
            .Replace(':', '_')
            .Replace('/', '_')
            .Replace('-', '_')
            .ToUpperInvariant();

        var envValue = Environment.GetEnvironmentVariable(envKey);
        if (!string.IsNullOrWhiteSpace(envValue))
        {
            value = envValue!;
            return true;
        }

        return false;
    }
}
