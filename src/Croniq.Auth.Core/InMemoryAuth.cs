using System.Collections.Concurrent;
using System.Linq;
using System.Security.Cryptography;
using Croniq.Auth.Abstractions;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Auth.Core;

public sealed class CallerContextAccessor : ICallerContextAccessor
{
    public ICallerContext? Current { get; set; }
}

public sealed record CallerContext(
    string TenantId,
    string? EnvironmentTag,
    CallerType CallerType,
    string CallerId,
    IReadOnlyCollection<string> Scopes,
    bool IsActive = true) : ICallerContext;

public sealed class CallerContextFactory : ICallerContextFactory
{
    private readonly IApiKeyStore _apiKeyStore;
    private readonly ILogger<CallerContextFactory> _logger;

    public CallerContextFactory(IApiKeyStore apiKeyStore, ILogger<CallerContextFactory> logger)
    {
        _apiKeyStore = apiKeyStore ?? throw new ArgumentNullException(nameof(apiKeyStore));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task<ICallerContext?> FromApiKeyAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(presentedKey)) return null;

        var validation = await _apiKeyStore.ValidateAsync(presentedKey, cancellationToken).ConfigureAwait(false);
        if (!validation.IsValid || string.IsNullOrWhiteSpace(validation.TenantId) || string.IsNullOrWhiteSpace(validation.CallerId))
        {
            _logger.LogWarning("API key validation failed: {Reason}", validation.Failure ?? "unknown");
            return null;
        }

        return new CallerContext(
            validation.TenantId!,
            validation.EnvironmentTag,
            CallerType.ApiKey,
            validation.CallerId!,
            validation.Scopes);
    }

    public Task<ICallerContext?> FromBearerTokenAsync(string bearerToken, CancellationToken cancellationToken = default)
    {
        // OIDC/JWT support will be added later; return null for now.
        _logger.LogDebug("Bearer token authentication not implemented yet");
        return Task.FromResult<ICallerContext?>(null);
    }
}

public sealed record ApiKeySeed(
    string KeyId,
    string Secret,
    string TenantId,
    string? EnvironmentTag,
    IReadOnlyCollection<string> Scopes,
    string? ClientId = null);

public sealed class InMemoryApiKeyStoreOptions
{
    public IList<ApiKeySeed> ApiKeys { get; } = new List<ApiKeySeed>();
}

public sealed class InMemoryApiKeyStore : IApiKeyStore
{
    private readonly ConcurrentDictionary<string, ApiKeyRecord> _store;

    public InMemoryApiKeyStore(IEnumerable<ApiKeySeed>? seeds = null)
    {
        _store = new ConcurrentDictionary<string, ApiKeyRecord>(StringComparer.Ordinal);

        if (seeds is not null)
        {
            foreach (var seed in seeds)
            {
                var record = new ApiKeyRecord(
                    seed.KeyId,
                    seed.Secret,
                    seed.TenantId,
                    seed.EnvironmentTag,
                    seed.Scopes.ToArray(),
                    seed.ClientId ?? seed.KeyId,
                    ExpiresAtUtc: null,
                    IsActive: true);
                _store[seed.KeyId] = record;
            }
        }
    }

    public Task<ApiKeyIssueResult> IssueAsync(ApiKeyIssueRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var keyId = $"ak_{Guid.NewGuid():N}";
        var secret = GenerateSecret();
        var record = new ApiKeyRecord(
            keyId,
            secret,
            request.TenantId,
            request.EnvironmentTag,
            request.Scopes?.ToArray() ?? Array.Empty<string>(),
            request.ClientId,
            ExpiresAtUtc: request.Ttl.HasValue ? DateTimeOffset.UtcNow.Add(request.Ttl.Value) : null,
            IsActive: true);
        _store[keyId] = record;

        return Task.FromResult(new ApiKeyIssueResult(
            request.ClientId,
            request.TenantId,
            keyId,
            $"{keyId}.{secret}",
            record.ExpiresAtUtc));
    }

    public Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(keyId)) throw new ArgumentNullException(nameof(keyId));

        if (_store.TryGetValue(keyId, out var record) && record.TenantId == tenantId)
        {
            _store[keyId] = record with { IsActive = false };
            return Task.FromResult(true);
        }

        return Task.FromResult(false);
    }

    public async Task<bool> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        if (!_store.TryGetValue(keyId, out var record) || record.TenantId != tenantId)
        {
            return false;
        }

        await RevokeAsync(tenantId, keyId, cancellationToken).ConfigureAwait(false);
        var issueRequest = new ApiKeyIssueRequest(
            tenantId,
            record.ClientId,
            record.EnvironmentTag,
            record.Scopes,
            record.ExpiresAtUtc.HasValue ? record.ExpiresAtUtc.Value - DateTimeOffset.UtcNow : null);
        await IssueAsync(issueRequest, cancellationToken).ConfigureAwait(false);
        return true;
    }

    public Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(presentedKey))
        {
            return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "missing"));
        }

        var (keyId, secret) = SplitKey(presentedKey);

        if (keyId is not null && _store.TryGetValue(keyId, out var record) && record.IsActive)
        {
            if (record.ExpiresAtUtc.HasValue && record.ExpiresAtUtc.Value < DateTimeOffset.UtcNow)
            {
                return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "expired"));
            }

            if (string.Equals(record.Secret, secret, StringComparison.Ordinal))
            {
                return Task.FromResult(new ApiKeyValidationResult(
                    true,
                    record.TenantId,
                    record.EnvironmentTag,
                    keyId,
                    record.Scopes,
                    null));
            }

            return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "invalid-secret"));
        }

        // Legacy/flat key support (no key id)
        foreach (var candidate in _store.Values.Where(r => r.IsActive))
        {
            if (candidate.ExpiresAtUtc.HasValue && candidate.ExpiresAtUtc.Value < DateTimeOffset.UtcNow) continue;
            if (string.Equals(candidate.Secret, presentedKey, StringComparison.Ordinal))
            {
                return Task.FromResult(new ApiKeyValidationResult(
                    true,
                    candidate.TenantId,
                    candidate.EnvironmentTag,
                    candidate.ClientId,
                    candidate.Scopes,
                    null));
            }
        }

        return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "not-found"));
    }

    public Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        var client = _store.Values.FirstOrDefault(v => v.ClientId == clientId && v.TenantId == tenantId);
        if (client is null)
        {
            return Task.FromResult<ApiClientDescriptor?>(null);
        }

        return Task.FromResult<ApiClientDescriptor?>(new ApiClientDescriptor(
            clientId,
            tenantId,
            clientId,
            client.EnvironmentTag,
            client.Scopes,
            client.IsActive,
            client.ExpiresAtUtc));
    }

    private static (string? KeyId, string Secret) SplitKey(string presented)
    {
        var idx = presented.IndexOf('.');
        if (idx > 0)
        {
            return (presented.Substring(0, idx), presented.Substring(idx + 1));
        }

        return (null, presented);
    }

    private static string GenerateSecret()
    {
        Span<byte> buffer = stackalloc byte[32];
        RandomNumberGenerator.Fill(buffer);
        return Convert.ToBase64String(buffer);
    }

    private sealed record ApiKeyRecord(
        string KeyId,
        string Secret,
        string TenantId,
        string? EnvironmentTag,
        IReadOnlyCollection<string> Scopes,
        string ClientId,
        DateTimeOffset? ExpiresAtUtc,
        bool IsActive);
}

public static class AuthCoreServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqAuthCore(this IServiceCollection services, Action<InMemoryApiKeyStoreOptions>? configure = null)
    {
        var options = new InMemoryApiKeyStoreOptions();
        configure?.Invoke(options);

        services.AddScoped<ICallerContextAccessor, CallerContextAccessor>();
        services.AddSingleton<IApiKeyStore>(_ => new InMemoryApiKeyStore(options.ApiKeys));
        services.AddScoped<ICallerContextFactory, CallerContextFactory>();
        return services;
    }
}
