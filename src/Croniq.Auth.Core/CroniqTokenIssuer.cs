using System.Security.Claims;
using System.Text;
using Croniq.Auth.Abstractions;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using Microsoft.IdentityModel.JsonWebTokens;
using Microsoft.IdentityModel.Tokens;
using JwtRegisteredClaimNames = System.IdentityModel.Tokens.Jwt.JwtRegisteredClaimNames;

namespace Croniq.Auth.Core;

/// <summary>Issues Croniq-signed bearer tokens for automation flows.</summary>
public sealed class CroniqTokenIssuer : ICroniqTokenIssuer
{
    private readonly IOptionsMonitor<CroniqTokenOptions> _options;
    private readonly ILogger<CroniqTokenIssuer> _logger;
    private readonly TimeProvider _timeProvider;
    private readonly JsonWebTokenHandler _tokenHandler = new();
    private SecurityKey? _cachedKey;
    private string? _cachedRawKey;

    public CroniqTokenIssuer(
        IOptionsMonitor<CroniqTokenOptions> options,
        ILogger<CroniqTokenIssuer> logger,
        TimeProvider? timeProvider = null)
    {
        _options = options ?? throw new ArgumentNullException(nameof(options));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public Task<CroniqTokenIssueResult> IssueAsync(CroniqTokenIssueRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var options = _options.CurrentValue ?? new CroniqTokenOptions();
        if (!options.Enabled)
        {
            throw new InvalidOperationException("Croniq token issuance is disabled (Croniq:Auth:Tokens:Enabled = false).");
        }

        if (string.IsNullOrWhiteSpace(request.TenantId)) throw new ArgumentException("TenantId is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.ClientId)) throw new ArgumentException("ClientId is required", nameof(request));

        var now = _timeProvider.GetUtcNow().UtcDateTime;
        var lifetime = ResolveLifetime(request.Lifetime, options);
        var expires = now.Add(lifetime);

        var claims = BuildClaims(request, options);
        var descriptor = new SecurityTokenDescriptor
        {
            Issuer = options.Issuer,
            Audience = request.Audience ?? options.DefaultAudience,
            NotBefore = now,
            Expires = expires,
            Claims = claims,
            SigningCredentials = new SigningCredentials(GetSigningKey(options.SigningKey), SecurityAlgorithms.HmacSha256)
        };

        var token = _tokenHandler.CreateToken(descriptor);
        var result = new CroniqTokenIssueResult(token, "Bearer", (int)Math.Round(lifetime.TotalSeconds));
        return Task.FromResult(result);
    }

    private IDictionary<string, object> BuildClaims(CroniqTokenIssueRequest request, CroniqTokenOptions options)
    {
        var claims = new Dictionary<string, object>(StringComparer.Ordinal)
        {
            [JwtRegisteredClaimNames.Sub] = request.ClientId,
            [options.ClientClaim] = request.ClientId,
            [options.TenantClaim] = request.TenantId,
            [JwtRegisteredClaimNames.Jti] = Guid.NewGuid().ToString("N")
        };

        var resolvedEnvironment = request.EnvironmentTag;
        if (string.IsNullOrWhiteSpace(resolvedEnvironment))
        {
            resolvedEnvironment = options.DefaultEnvironment;
        }

        if (!string.IsNullOrWhiteSpace(resolvedEnvironment))
        {
            claims[options.EnvironmentClaim] = resolvedEnvironment!;
        }

        var normalizedScopes = request.Scopes is { Count: > 0 }
            ? string.Join(' ', request.Scopes)
            : null;

        if (!string.IsNullOrWhiteSpace(normalizedScopes))
        {
            claims[options.ScopeClaim] = normalizedScopes;
        }

        if (request.AdditionalClaims is not null)
        {
            foreach (var (key, value) in request.AdditionalClaims)
            {
                if (string.IsNullOrWhiteSpace(key) || value is null)
                {
                    continue;
                }

                // Let callers attach additional, non-core claims.
                // Callers are expected to avoid overriding core claim names.
                claims[key] = value;
            }
        }

        return claims;
    }

    private SecurityKey GetSigningKey(string signingKey)
    {
        if (string.IsNullOrWhiteSpace(signingKey))
        {
            throw new InvalidOperationException("Croniq:Auth:Tokens:SigningKey must be configured (base64-encoded secret).");
        }

        if (_cachedKey is not null && string.Equals(_cachedRawKey, signingKey, StringComparison.Ordinal))
        {
            return _cachedKey;
        }

        SecurityKey key;
        try
        {
            var bytes = Convert.FromBase64String(signingKey);
            key = new SymmetricSecurityKey(bytes);
        }
        catch (FormatException ex)
        {
            _logger.LogError(ex, "Croniq token signing key is not valid Base64");
            throw;
        }

        _cachedKey = key;
        _cachedRawKey = signingKey;
        return key;
    }

    private static TimeSpan ResolveLifetime(TimeSpan? requested, CroniqTokenOptions options)
    {
        if (requested.HasValue && requested.Value > TimeSpan.Zero)
        {
            return requested.Value;
        }

        var minutes = options.DefaultLifetimeMinutes <= 0 ? 15 : options.DefaultLifetimeMinutes;
        return TimeSpan.FromMinutes(minutes);
    }
}
