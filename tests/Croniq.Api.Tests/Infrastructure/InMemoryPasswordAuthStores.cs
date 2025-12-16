using System.Collections.Concurrent;
using Croniq.Auth.Abstractions;

namespace Croniq.Api.Tests.Infrastructure;

public sealed class InMemoryPasswordUserStore : IPasswordUserStore
{
    private readonly ConcurrentDictionary<string, PasswordUserRecord> _byTenantAndName = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, PasswordUserRecord> _byTenantAndId = new(StringComparer.Ordinal);

    public Task<PasswordUserRecord?> FindByUsernameAsync(string tenantId, string username, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(username)) return Task.FromResult<PasswordUserRecord?>(null);

        var key = GetNameKey(tenantId, username);
        return Task.FromResult(_byTenantAndName.TryGetValue(key, out var record) ? record : null);
    }

    public Task<PasswordUserRecord?> FindByIdAsync(string tenantId, string userId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(userId)) return Task.FromResult<PasswordUserRecord?>(null);

        var key = GetIdKey(tenantId, userId);
        return Task.FromResult(_byTenantAndId.TryGetValue(key, out var record) ? record : null);
    }

    public Task<PasswordUserRecord> UpsertAsync(PasswordUserUpsertRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var now = DateTimeOffset.UtcNow;
        var nameKey = GetNameKey(request.TenantId, request.Username);

        PasswordUserRecord result;

        if (_byTenantAndName.TryGetValue(nameKey, out var existing))
        {
            result = existing with
            {
                Username = request.Username,
                PasswordHash = request.PasswordHash,
                Scopes = request.Scopes,
                IsActive = request.IsActive,
                UpdatedAtUtc = now
            };
        }
        else
        {
            var userId = $"usr_{Guid.NewGuid():N}";
            result = new PasswordUserRecord(
                userId,
                request.TenantId,
                request.Username,
                request.Scopes,
                request.PasswordHash,
                request.IsActive,
                FailedLoginCount: 0,
                LockoutEndUtc: null,
                CreatedAtUtc: now,
                UpdatedAtUtc: now);
        }

        _byTenantAndName[nameKey] = result;
        _byTenantAndId[GetIdKey(result.TenantId, result.UserId)] = result;

        return Task.FromResult(result);
    }

    public Task RecordLoginFailureAsync(string tenantId, string userId, DateTimeOffset? lockoutEndUtc, CancellationToken cancellationToken = default)
    {
        var key = GetIdKey(tenantId, userId);
        if (!_byTenantAndId.TryGetValue(key, out var existing))
        {
            return Task.CompletedTask;
        }

        var now = DateTimeOffset.UtcNow;
        var updated = existing with
        {
            FailedLoginCount = existing.FailedLoginCount + 1,
            LockoutEndUtc = lockoutEndUtc,
            UpdatedAtUtc = now
        };

        _byTenantAndId[key] = updated;
        _byTenantAndName[GetNameKey(updated.TenantId, updated.Username)] = updated;

        return Task.CompletedTask;
    }

    public Task RecordLoginSuccessAsync(string tenantId, string userId, CancellationToken cancellationToken = default)
    {
        var key = GetIdKey(tenantId, userId);
        if (!_byTenantAndId.TryGetValue(key, out var existing))
        {
            return Task.CompletedTask;
        }

        var now = DateTimeOffset.UtcNow;
        var updated = existing with
        {
            FailedLoginCount = 0,
            LockoutEndUtc = null,
            UpdatedAtUtc = now
        };

        _byTenantAndId[key] = updated;
        _byTenantAndName[GetNameKey(updated.TenantId, updated.Username)] = updated;

        return Task.CompletedTask;
    }

    private static string GetNameKey(string tenantId, string username) => $"{tenantId}|{username.Trim().ToUpperInvariant()}";

    private static string GetIdKey(string tenantId, string userId) => $"{tenantId}|{userId}";
}

public sealed class InMemoryRefreshTokenStore : IRefreshTokenStore
{
    private readonly ConcurrentDictionary<string, RefreshTokenRecord> _byHash = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, RefreshTokenRecord> _byId = new(StringComparer.Ordinal);

    public Task<RefreshTokenRecord> CreateAsync(RefreshTokenCreateRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var tokenId = $"rt_{Guid.NewGuid():N}";
        var now = DateTimeOffset.UtcNow;
        var record = new RefreshTokenRecord(
            tokenId,
            request.TenantId,
            request.UserId,
            request.TokenHash,
            request.ExpiresAtUtc,
            RevokedAtUtc: null,
            ReplacedByTokenId: null,
            CreatedAtUtc: now);

        _byHash[GetHashKey(request.TenantId, request.TokenHash)] = record;
        _byId[GetIdKey(request.TenantId, tokenId)] = record;
        return Task.FromResult(record);
    }

    public Task<RefreshTokenRecord?> FindActiveByHashAsync(string tenantId, string tokenHash, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(tokenHash)) return Task.FromResult<RefreshTokenRecord?>(null);

        if (!_byHash.TryGetValue(GetHashKey(tenantId, tokenHash), out var record))
        {
            return Task.FromResult<RefreshTokenRecord?>(null);
        }

        var now = DateTimeOffset.UtcNow;
        if (record.RevokedAtUtc.HasValue || record.ExpiresAtUtc <= now)
        {
            return Task.FromResult<RefreshTokenRecord?>(null);
        }

        return Task.FromResult<RefreshTokenRecord?>(record);
    }

    public Task RevokeAsync(string tenantId, string tokenId, string? replacedByTokenId, CancellationToken cancellationToken = default)
    {
        var idKey = GetIdKey(tenantId, tokenId);
        if (!_byId.TryGetValue(idKey, out var existing))
        {
            return Task.CompletedTask;
        }

        if (existing.RevokedAtUtc.HasValue)
        {
            return Task.CompletedTask;
        }

        var now = DateTimeOffset.UtcNow;
        var updated = existing with
        {
            RevokedAtUtc = now,
            ReplacedByTokenId = replacedByTokenId
        };

        _byId[idKey] = updated;
        _byHash[GetHashKey(updated.TenantId, updated.TokenHash)] = updated;

        return Task.CompletedTask;
    }

    private static string GetHashKey(string tenantId, string tokenHash) => $"{tenantId}|{tokenHash}";

    private static string GetIdKey(string tenantId, string tokenId) => $"{tenantId}|{tokenId}";
}
