using System.Collections.Concurrent;
using System.Linq;
using System.Security.Claims;
using System.Security.Cryptography;
using Croniq.Auth.Abstractions;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using Microsoft.IdentityModel.JsonWebTokens;
using Microsoft.IdentityModel.Protocols;
using Microsoft.IdentityModel.Protocols.OpenIdConnect;
using Microsoft.IdentityModel.Tokens;

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
    private readonly IOptionsMonitor<CroniqOidcOptions> _oidcOptions;
    private readonly ILogger<CallerContextFactory> _logger;
    private readonly JsonWebTokenHandler _tokenHandler = new();
    private readonly object _oidcConfigurationLock = new();
    private ConfigurationManager<OpenIdConnectConfiguration>? _configurationManager;
    private string? _configuredAuthority;

    public CallerContextFactory(
        IApiKeyStore apiKeyStore,
        IOptionsMonitor<CroniqOidcOptions> oidcOptions,
        ILogger<CallerContextFactory> logger)
    {
        _apiKeyStore = apiKeyStore ?? throw new ArgumentNullException(nameof(apiKeyStore));
        _oidcOptions = oidcOptions ?? throw new ArgumentNullException(nameof(oidcOptions));
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

    public async Task<ICallerContext?> FromBearerTokenAsync(string bearerToken, CancellationToken cancellationToken = default)
    {
        var token = ExtractBearerToken(bearerToken);
        if (string.IsNullOrWhiteSpace(token))
        {
            return null;
        }

        var options = _oidcOptions.CurrentValue ?? new CroniqOidcOptions();
        if (!options.Enabled || string.IsNullOrWhiteSpace(options.Authority))
        {
            _logger.LogDebug("OIDC bearer auth disabled or authority missing");
            return null;
        }

        try
        {
            var validationParameters = await BuildValidationParametersAsync(options, cancellationToken).ConfigureAwait(false);
            var result = await _tokenHandler.ValidateTokenAsync(token, validationParameters).ConfigureAwait(false);
            if (!result.IsValid || result.ClaimsIdentity is null)
            {
                _logger.LogWarning("Bearer token validation failed: {Reason}", result.Exception?.Message ?? "invalid");
                return null;
            }

            var principal = new ClaimsPrincipal(result.ClaimsIdentity);
            var tenantId = ResolveTenant(principal, options);
            if (string.IsNullOrWhiteSpace(tenantId))
            {
                _logger.LogWarning("Bearer token missing tenant claim '{Claim}'", options.TenantClaim);
                return null;
            }

            var environment = ResolveEnvironment(principal, options);
            var callerId = ResolveCallerId(principal, options);
            var scopes = ResolveScopes(principal, options);

            if (options.RequiredScopes?.Length > 0 && !HasAllScopes(scopes, options.RequiredScopes))
            {
                _logger.LogWarning("Bearer token missing required scopes: {Scopes}", string.Join(", ", options.RequiredScopes));
                return null;
            }

            return new CallerContext(
                tenantId,
                environment,
                CallerType.User,
                callerId,
                scopes);
        }
        catch (SecurityTokenException ex)
        {
            _logger.LogWarning(ex, "Bearer token rejected");
            return null;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Unexpected failure while validating bearer token");
            return null;
        }
    }

    private static string? ExtractBearerToken(string headerValue)
    {
        if (string.IsNullOrWhiteSpace(headerValue))
        {
            return null;
        }

        const string prefix = "Bearer ";
        return headerValue.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
            ? headerValue.Substring(prefix.Length).Trim()
            : headerValue.Trim();
    }

    private async Task<TokenValidationParameters> BuildValidationParametersAsync(CroniqOidcOptions options, CancellationToken cancellationToken)
    {
        var configuration = await GetOidcConfigurationAsync(options, cancellationToken).ConfigureAwait(false);
        var clockSkew = options.ClockSkewSeconds < 0 ? 0 : options.ClockSkewSeconds;

        return new TokenValidationParameters
        {
            ValidateIssuer = true,
            ValidIssuer = options.Authority,
            IssuerSigningKeys = configuration.SigningKeys,
            ValidateAudience = !string.IsNullOrWhiteSpace(options.Audience),
            ValidAudience = options.Audience,
            RequireExpirationTime = true,
            ValidateLifetime = true,
            ClockSkew = TimeSpan.FromSeconds(clockSkew),
            RequireSignedTokens = true,
            NameClaimType = options.CallerIdClaim,
            ValidateIssuerSigningKey = true
        };
    }

    private Task<OpenIdConnectConfiguration> GetOidcConfigurationAsync(CroniqOidcOptions options, CancellationToken cancellationToken)
    {
        var authority = options.Authority?.TrimEnd('/');
        if (string.IsNullOrWhiteSpace(authority))
        {
            throw new InvalidOperationException("Croniq:Auth:Oidc:Authority is required when OIDC authentication is enabled.");
        }

        lock (_oidcConfigurationLock)
        {
            if (_configurationManager is null || !string.Equals(_configuredAuthority, authority, StringComparison.OrdinalIgnoreCase))
            {
                var metadataAddress = string.IsNullOrWhiteSpace(options.MetadataAddress)
                    ? $"{authority}/.well-known/openid-configuration"
                    : options.MetadataAddress;

                var documentRetriever = new HttpDocumentRetriever
                {
                    RequireHttps = options.RequireHttpsMetadata
                };

                _configurationManager = new ConfigurationManager<OpenIdConnectConfiguration>(
                    metadataAddress!,
                    new OpenIdConnectConfigurationRetriever(),
                    documentRetriever)
                {
                    AutomaticRefreshInterval = options.MetadataRefreshInterval,
                    RefreshInterval = options.MetadataRefreshInterval
                };
                _configuredAuthority = authority;
            }
        }

        return _configurationManager!.GetConfigurationAsync(cancellationToken);
    }

    private static string? ResolveTenant(ClaimsPrincipal principal, CroniqOidcOptions options)
    {
        return FindFirst(principal, options.TenantClaim, options.TenantFallbackClaims);
    }

    private static string? ResolveEnvironment(ClaimsPrincipal principal, CroniqOidcOptions options)
    {
        return FindFirst(principal, options.EnvironmentClaim, options.EnvironmentFallbackClaims) ?? options.DefaultEnvironment;
    }

    private static string ResolveCallerId(ClaimsPrincipal principal, CroniqOidcOptions options)
    {
        return FindFirst(principal, options.CallerIdClaim, options.CallerIdFallbackClaims)
            ?? principal.Identity?.Name
            ?? "oidc-user";
    }

    private static IReadOnlyCollection<string> ResolveScopes(ClaimsPrincipal principal, CroniqOidcOptions options)
    {
        var claims = options.ScopeClaims?.Length > 0 ? options.ScopeClaims : new[] { "scope", "scp" };
        var comparer = options.NormalizeScopesToLowercase ? StringComparer.Ordinal : StringComparer.OrdinalIgnoreCase;
        var result = new HashSet<string>(comparer);

        foreach (var claimName in claims)
        {
            foreach (var claim in principal.FindAll(claimName))
            {
                if (string.IsNullOrWhiteSpace(claim.Value))
                {
                    continue;
                }

                var parts = claim.Value.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
                foreach (var part in parts)
                {
                    var formatted = options.NormalizeScopesToLowercase ? part.ToLowerInvariant() : part;
                    result.Add(formatted);
                }
            }
        }

        return result.Count == 0 ? Array.Empty<string>() : result.ToArray();
    }

    private static bool HasAllScopes(IReadOnlyCollection<string> granted, IReadOnlyCollection<string> required)
    {
        if (required.Count == 0)
        {
            return true;
        }

        var comparer = StringComparer.OrdinalIgnoreCase;
        return required.All(scope => granted.Any(grantedScope => comparer.Equals(grantedScope, scope)));
    }

    private static string? FindFirst(ClaimsPrincipal principal, string? primary, IReadOnlyCollection<string>? fallbacks)
    {
        if (!string.IsNullOrWhiteSpace(primary))
        {
            var match = principal.FindFirst(primary);
            if (!string.IsNullOrWhiteSpace(match?.Value))
            {
                return match.Value;
            }
        }

        if (fallbacks is null)
        {
            return null;
        }

        foreach (var claimName in fallbacks)
        {
            var match = principal.FindFirst(claimName);
            if (!string.IsNullOrWhiteSpace(match?.Value))
            {
                return match.Value;
            }
        }

        return null;
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

        services.AddOptions<CroniqOidcOptions>();
        services.AddScoped<ICallerContextAccessor, CallerContextAccessor>();
        services.AddSingleton<IApiKeyStore>(_ => new InMemoryApiKeyStore(options.ApiKeys));
        services.AddScoped<ICallerContextFactory, CallerContextFactory>();
        return services;
    }
}
