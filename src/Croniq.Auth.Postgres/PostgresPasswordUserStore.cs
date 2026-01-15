using System.Text.Json;
using Croniq.Auth.Abstractions;
using Croniq.Data.Postgres;
using Croniq.Data.Postgres.Entities;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Auth.Postgres;

public sealed class PostgresPasswordUserStore : IPasswordUserStore
{
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);

    private readonly IDbContextFactory<PostgresDbContext> _dbContextFactory;
    private readonly TimeProvider _timeProvider;

    public PostgresPasswordUserStore(
        IDbContextFactory<PostgresDbContext> dbContextFactory,
        TimeProvider? timeProvider = null)
    {
        _dbContextFactory = dbContextFactory ?? throw new ArgumentNullException(nameof(dbContextFactory));
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task<PasswordUserRecord?> FindByUsernameAsync(string tenantId, string username, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(username)) return null;

        var normalized = NormalizeUsername(username);

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.PasswordUsers
            .AsNoTracking()
            .FirstOrDefaultAsync(u => u.TenantId == tenantId && u.UsernameNormalized == normalized, cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : ToRecord(entity);
    }

    public async Task<PasswordUserRecord?> FindByIdAsync(string tenantId, string userId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(userId)) return null;

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.PasswordUsers
            .AsNoTracking()
            .FirstOrDefaultAsync(u => u.TenantId == tenantId && u.UserId == userId, cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : ToRecord(entity);
    }

    public async Task<PasswordUserRecord> UpsertAsync(PasswordUserUpsertRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.TenantId)) throw new ArgumentException("TenantId is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.Username)) throw new ArgumentException("Username is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.PasswordHash)) throw new ArgumentException("PasswordHash is required", nameof(request));

        var normalized = NormalizeUsername(request.Username);
        var now = _timeProvider.GetUtcNow().UtcDateTime;

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        var entity = await db.PasswordUsers
            .FirstOrDefaultAsync(u => u.TenantId == request.TenantId && u.UsernameNormalized == normalized, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            entity = new PasswordUserEntity
            {
                UserId = $"usr_{Guid.NewGuid():N}",
                TenantId = request.TenantId,
                Username = request.Username.Trim(),
                UsernameNormalized = normalized,
                PasswordHash = request.PasswordHash,
                ScopesJson = SerializeScopes(request.Scopes),
                IsActive = request.IsActive,
                PasswordChangeRequired = request.PasswordChangeRequired,
                FailedLoginCount = 0,
                LockoutEndUtc = null,
                CreatedAtUtc = now,
                UpdatedAtUtc = now
            };

            db.PasswordUsers.Add(entity);
        }
        else
        {
            entity.Username = request.Username.Trim();
            entity.UsernameNormalized = normalized;
            entity.PasswordHash = request.PasswordHash;
            entity.ScopesJson = SerializeScopes(request.Scopes);
            entity.IsActive = request.IsActive;
            entity.PasswordChangeRequired = request.PasswordChangeRequired;
            entity.UpdatedAtUtc = now;
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return ToRecord(entity);
    }

    public async Task RecordLoginFailureAsync(string tenantId, string userId, DateTimeOffset? lockoutEndUtc, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(userId)) throw new ArgumentNullException(nameof(userId));

        var now = _timeProvider.GetUtcNow().UtcDateTime;

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.PasswordUsers
            .FirstOrDefaultAsync(u => u.TenantId == tenantId && u.UserId == userId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        entity.FailedLoginCount += 1;
        entity.LockoutEndUtc = lockoutEndUtc?.UtcDateTime;
        entity.UpdatedAtUtc = now;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task RecordLoginSuccessAsync(string tenantId, string userId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(userId)) throw new ArgumentNullException(nameof(userId));

        var now = _timeProvider.GetUtcNow().UtcDateTime;

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.PasswordUsers
            .FirstOrDefaultAsync(u => u.TenantId == tenantId && u.UserId == userId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        if (entity.FailedLoginCount == 0 && entity.LockoutEndUtc is null)
        {
            return;
        }

        entity.FailedLoginCount = 0;
        entity.LockoutEndUtc = null;
        entity.UpdatedAtUtc = now;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private static string NormalizeUsername(string username) => username.Trim().ToUpperInvariant();

    private static PasswordUserRecord ToRecord(PasswordUserEntity entity)
    {
        return new PasswordUserRecord(
            entity.UserId,
            entity.TenantId,
            entity.Username,
            ParseScopes(entity.ScopesJson),
            entity.PasswordHash,
            entity.IsActive,
            entity.FailedLoginCount,
            entity.LockoutEndUtc.HasValue ? new DateTimeOffset(DateTime.SpecifyKind(entity.LockoutEndUtc.Value, DateTimeKind.Utc)) : null,
            new DateTimeOffset(DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc)),
            new DateTimeOffset(DateTime.SpecifyKind(entity.UpdatedAtUtc, DateTimeKind.Utc)),
            entity.PasswordChangeRequired);
    }

    private static string? SerializeScopes(IReadOnlyCollection<string> scopes)
    {
        if (scopes is null || scopes.Count == 0)
        {
            return null;
        }

        return JsonSerializer.Serialize(scopes, JsonOptions);
    }

    private static IReadOnlyCollection<string> ParseScopes(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
        {
            return Array.Empty<string>();
        }

        try
        {
            var parsed = JsonSerializer.Deserialize<string[]>(json, JsonOptions);
            return parsed?.Where(s => !string.IsNullOrWhiteSpace(s)).Select(s => s.Trim()).ToArray() ?? Array.Empty<string>();
        }
        catch
        {
            return Array.Empty<string>();
        }
    }
}
