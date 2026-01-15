using System.Security.Cryptography;
using System.Text;
using System.Linq;
using Croniq.Auth.Abstractions;
using Croniq.Core.Observability;
using Croniq.Options;
using Microsoft.AspNetCore.Identity;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Auth.SqlServer;

public sealed class PasswordAuthService : IPasswordAuthService
{
    private readonly IPasswordUserStore _users;
    private readonly IRefreshTokenStore _refreshTokens;
    private readonly ICroniqTokenIssuer _tokenIssuer;
    private readonly IOptionsMonitor<PasswordAuthOptions> _options;
    private readonly IOptionsMonitor<CroniqOptions> _coreOptions;
    private readonly ILogger<PasswordAuthService> _logger;
    private readonly TimeProvider _timeProvider;
    private readonly PasswordHasher<PasswordAuthUser> _passwordHasher = new();

    public PasswordAuthService(
        IPasswordUserStore users,
        IRefreshTokenStore refreshTokens,
        ICroniqTokenIssuer tokenIssuer,
        IOptionsMonitor<PasswordAuthOptions> options,
        IOptionsMonitor<CroniqOptions> coreOptions,
        ILogger<PasswordAuthService> logger,
        TimeProvider? timeProvider = null)
    {
        _users = users ?? throw new ArgumentNullException(nameof(users));
        _refreshTokens = refreshTokens ?? throw new ArgumentNullException(nameof(refreshTokens));
        _tokenIssuer = tokenIssuer ?? throw new ArgumentNullException(nameof(tokenIssuer));
        _options = options ?? throw new ArgumentNullException(nameof(options));
        _coreOptions = coreOptions ?? throw new ArgumentNullException(nameof(coreOptions));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task<PasswordLoginResult?> LoginAsync(
        string tenantId,
        string username,
        string password,
        string? environmentTag,
        IReadOnlyCollection<string>? requestedScopes,
        string? audience,
        CancellationToken cancellationToken = default)
    {
        if (!IsEnabled())
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(username)) throw new ArgumentNullException(nameof(username));
        if (string.IsNullOrWhiteSpace(password)) throw new ArgumentNullException(nameof(password));

        if (string.IsNullOrWhiteSpace(environmentTag))
        {
            environmentTag = _coreOptions.CurrentValue?.EnvironmentTag;
        }

        var user = await _users.FindByUsernameAsync(tenantId, username, cancellationToken).ConfigureAwait(false);
        if (user is null)
        {
            return new PasswordLoginResult(false, null, null, null, null);
        }

        if (!user.IsActive)
        {
            return new PasswordLoginResult(false, null, null, null, null);
        }

        var now = _timeProvider.GetUtcNow();
        if (user.LockoutEndUtc.HasValue && user.LockoutEndUtc.Value > now)
        {
            return new PasswordLoginResult(false, null, null, null, user.LockoutEndUtc);
        }

        var verification = _passwordHasher.VerifyHashedPassword(
            new PasswordAuthUser(user.UserId, user.Username),
            user.PasswordHash,
            password);

        if (verification == PasswordVerificationResult.SuccessRehashNeeded)
        {
            try
            {
                var upgradedHash = _passwordHasher.HashPassword(new PasswordAuthUser(user.UserId, user.Username), password);
                await _users.UpsertAsync(new PasswordUserUpsertRequest(
                    user.TenantId,
                    user.Username,
                    upgradedHash,
                    user.Scopes,
                    user.IsActive,
                    PasswordChangeRequired: user.PasswordChangeRequired),
                    cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to upgrade password hash for {TenantId}/{UserId}", IdentifierHashing.HashTenantId(user.TenantId) ?? string.Empty, user.UserId);
            }
        }

        if (verification != PasswordVerificationResult.Success && verification != PasswordVerificationResult.SuccessRehashNeeded)
        {
            var options = _options.CurrentValue ?? new PasswordAuthOptions();
            var nextFailed = user.FailedLoginCount + 1;

            DateTimeOffset? lockoutEndUtc = null;
            if (options.MaxFailedAccessAttempts > 0 && nextFailed >= options.MaxFailedAccessAttempts)
            {
                var lockoutMinutes = options.LockoutMinutes <= 0 ? 15 : options.LockoutMinutes;
                lockoutEndUtc = now.AddMinutes(lockoutMinutes);
            }

            await _users.RecordLoginFailureAsync(user.TenantId, user.UserId, lockoutEndUtc, cancellationToken).ConfigureAwait(false);
            return new PasswordLoginResult(false, null, null, null, lockoutEndUtc);
        }

        await _users.RecordLoginSuccessAsync(user.TenantId, user.UserId, cancellationToken).ConfigureAwait(false);

        var scopes = ResolveGrantedScopes(user.Scopes, requestedScopes);
        if (scopes is null)
        {
            return new PasswordLoginResult(false, null, null, null, null);
        }

        var accessTokenLifetime = ResolveAccessTokenLifetime(_options.CurrentValue);

        // Include PasswordChangeRequired directly in the access token so API endpoints can enforce it
        // without a DB lookup.
        // TODO (2FA): When adding MFA/2FA, consider also embedding the auth method / MFA state in the token
        // (e.g. via AMR or a dedicated claim) and update enforcement to allow MFA completion flows.
        var token = await _tokenIssuer.IssueAsync(new CroniqTokenIssueRequest(
            tenantId,
            ClientId: user.UserId,
            environmentTag,
            scopes,
            audience,
            accessTokenLifetime,
            AdditionalClaims: new Dictionary<string, object?>
            {
                [CroniqClaimNames.PasswordChangeRequired] = user.PasswordChangeRequired,
            }),
            cancellationToken).ConfigureAwait(false);

        var (refreshToken, refreshTokenHash) = CreateRefreshToken();
        var refreshExpiresAt = now.AddDays(ResolveRefreshTokenLifetimeDays(_options.CurrentValue));

        await _refreshTokens.CreateAsync(new RefreshTokenCreateRequest(
            tenantId,
            user.UserId,
            refreshTokenHash,
            refreshExpiresAt),
            cancellationToken).ConfigureAwait(false);

        return new PasswordLoginResult(true, token.AccessToken, refreshToken, token.ExpiresInSeconds, null, user.PasswordChangeRequired);
    }

    public async Task<PasswordRefreshResult?> RefreshAsync(
        string tenantId,
        string refreshToken,
        string? environmentTag,
        IReadOnlyCollection<string>? requestedScopes,
        string? audience,
        CancellationToken cancellationToken = default)
    {
        if (!IsEnabled())
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(refreshToken)) throw new ArgumentNullException(nameof(refreshToken));

        if (string.IsNullOrWhiteSpace(environmentTag))
        {
            environmentTag = _coreOptions.CurrentValue?.EnvironmentTag;
        }

        var refreshHash = HashRefreshToken(refreshToken);
        var existing = await _refreshTokens.FindActiveByHashAsync(tenantId, refreshHash, cancellationToken).ConfigureAwait(false);
        if (existing is null)
        {
            return new PasswordRefreshResult(false, null, null, null);
        }

        var user = await _users.FindByIdAsync(tenantId, existing.UserId, cancellationToken).ConfigureAwait(false);
        if (user is null || !user.IsActive)
        {
            return new PasswordRefreshResult(false, null, null, null);
        }

        var scopes = ResolveGrantedScopes(user.Scopes, requestedScopes);
        if (scopes is null)
        {
            return new PasswordRefreshResult(false, null, null, null);
        }

        var accessTokenLifetime = ResolveAccessTokenLifetime(_options.CurrentValue);
        var token = await _tokenIssuer.IssueAsync(new CroniqTokenIssueRequest(
            tenantId,
            ClientId: user.UserId,
            environmentTag,
            scopes,
            audience,
            accessTokenLifetime,
            AdditionalClaims: new Dictionary<string, object?>
            {
                [CroniqClaimNames.PasswordChangeRequired] = user.PasswordChangeRequired,
            }),
            cancellationToken).ConfigureAwait(false);

        var (newRefreshToken, newHash) = CreateRefreshToken();
        var now = _timeProvider.GetUtcNow();
        var refreshExpiresAt = now.AddDays(ResolveRefreshTokenLifetimeDays(_options.CurrentValue));

        var created = await _refreshTokens.CreateAsync(new RefreshTokenCreateRequest(
            tenantId,
            user.UserId,
            newHash,
            refreshExpiresAt),
            cancellationToken).ConfigureAwait(false);

        await _refreshTokens.RevokeAsync(tenantId, existing.TokenId, created.TokenId, cancellationToken).ConfigureAwait(false);

        return new PasswordRefreshResult(true, token.AccessToken, newRefreshToken, token.ExpiresInSeconds, user.PasswordChangeRequired);
    }

    public async Task<bool?> LogoutAsync(string tenantId, string refreshToken, CancellationToken cancellationToken = default)
    {
        if (!IsEnabled())
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(refreshToken)) throw new ArgumentNullException(nameof(refreshToken));

        var refreshHash = HashRefreshToken(refreshToken);
        var existing = await _refreshTokens.FindActiveByHashAsync(tenantId, refreshHash, cancellationToken).ConfigureAwait(false);
        if (existing is null)
        {
            return false;
        }

        await _refreshTokens.RevokeAsync(tenantId, existing.TokenId, replacedByTokenId: null, cancellationToken).ConfigureAwait(false);
        return true;
    }

    public async Task<bool?> ChangePasswordAsync(
        string tenantId,
        string userId,
        string currentPassword,
        string newPassword,
        CancellationToken cancellationToken = default)
    {
        if (!IsEnabled())
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(userId)) throw new ArgumentNullException(nameof(userId));
        if (string.IsNullOrWhiteSpace(currentPassword)) throw new ArgumentNullException(nameof(currentPassword));
        if (string.IsNullOrWhiteSpace(newPassword)) throw new ArgumentNullException(nameof(newPassword));

        var user = await _users.FindByIdAsync(tenantId, userId, cancellationToken).ConfigureAwait(false);
        if (user is null || !user.IsActive)
        {
            return false;
        }

        var verification = _passwordHasher.VerifyHashedPassword(
            new PasswordAuthUser(user.UserId, user.Username),
            user.PasswordHash,
            currentPassword);

        if (verification != PasswordVerificationResult.Success && verification != PasswordVerificationResult.SuccessRehashNeeded)
        {
            return false;
        }

        var newHash = _passwordHasher.HashPassword(new PasswordAuthUser(user.UserId, user.Username), newPassword);

        await _users.UpsertAsync(new PasswordUserUpsertRequest(
                user.TenantId,
                user.Username,
                newHash,
                user.Scopes,
                user.IsActive,
                PasswordChangeRequired: false),
            cancellationToken).ConfigureAwait(false);

        await _users.RecordLoginSuccessAsync(user.TenantId, user.UserId, cancellationToken).ConfigureAwait(false);

        // Password change is a security boundary: invalidate all refresh tokens so sessions must be re-established.
        await _refreshTokens.RevokeAllForUserAsync(user.TenantId, user.UserId, cancellationToken).ConfigureAwait(false);
        return true;
    }

    public string HashPassword(string userId, string username, string password)
    {
        if (string.IsNullOrWhiteSpace(userId)) throw new ArgumentNullException(nameof(userId));
        if (string.IsNullOrWhiteSpace(username)) throw new ArgumentNullException(nameof(username));
        if (string.IsNullOrWhiteSpace(password)) throw new ArgumentNullException(nameof(password));

        return _passwordHasher.HashPassword(new PasswordAuthUser(userId, username), password);
    }

    private bool IsEnabled()
    {
        var options = _options.CurrentValue ?? new PasswordAuthOptions();
        return options.Enabled;
    }

    private static TimeSpan ResolveAccessTokenLifetime(PasswordAuthOptions? options)
    {
        var minutes = options?.AccessTokenLifetimeMinutes ?? 15;
        if (minutes <= 0)
        {
            minutes = 15;
        }

        return TimeSpan.FromMinutes(minutes);
    }

    private static int ResolveRefreshTokenLifetimeDays(PasswordAuthOptions? options)
    {
        var days = options?.RefreshTokenLifetimeDays ?? 7;
        return days <= 0 ? 7 : days;
    }

    private static IReadOnlyCollection<string>? ResolveGrantedScopes(
        IReadOnlyCollection<string> allowed,
        IReadOnlyCollection<string>? requested)
    {
        if (allowed is null || allowed.Count == 0)
        {
            return Array.Empty<string>();
        }

        if (requested is null || requested.Count == 0)
        {
            return allowed;
        }

        var allowedSet = new HashSet<string>(allowed.Where(s => !string.IsNullOrWhiteSpace(s)).Select(s => s.Trim()), StringComparer.OrdinalIgnoreCase);
        var requestedClean = requested.Where(s => !string.IsNullOrWhiteSpace(s)).Select(s => s.Trim()).ToArray();

        foreach (var scope in requestedClean)
        {
            if (!allowedSet.Contains(scope))
            {
                return null;
            }
        }

        return requestedClean;
    }

    private static (string Token, string Hash) CreateRefreshToken()
    {
        var bytes = new byte[32];
        RandomNumberGenerator.Fill(bytes);
        var token = Convert.ToBase64String(bytes);
        var hash = HashRefreshToken(token);
        return (token, hash);
    }

    private static string HashRefreshToken(string token)
    {
        var raw = Encoding.UTF8.GetBytes(token);
        var hash = SHA256.HashData(raw);
        return Convert.ToBase64String(hash);
    }

    private sealed record PasswordAuthUser(string UserId, string Username);
}
