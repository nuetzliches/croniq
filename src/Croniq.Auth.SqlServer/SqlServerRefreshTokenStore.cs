using Croniq.Auth.Abstractions;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Auth.SqlServer;

public sealed class SqlServerRefreshTokenStore : IRefreshTokenStore
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbContextFactory;
    private readonly TimeProvider _timeProvider;

    public SqlServerRefreshTokenStore(
        IDbContextFactory<SqlServerDbContext> dbContextFactory,
        TimeProvider? timeProvider = null)
    {
        _dbContextFactory = dbContextFactory ?? throw new ArgumentNullException(nameof(dbContextFactory));
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task<RefreshTokenRecord> CreateAsync(RefreshTokenCreateRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.TenantId)) throw new ArgumentException("TenantId is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.UserId)) throw new ArgumentException("UserId is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.TokenHash)) throw new ArgumentException("TokenHash is required", nameof(request));

        var now = _timeProvider.GetUtcNow().UtcDateTime;

        var entity = new RefreshTokenEntity
        {
            TokenId = $"rt_{Guid.NewGuid():N}",
            TenantId = request.TenantId,
            UserId = request.UserId,
            TokenHash = request.TokenHash,
            ExpiresAtUtc = request.ExpiresAtUtc.UtcDateTime,
            RevokedAtUtc = null,
            ReplacedByTokenId = null,
            CreatedAtUtc = now
        };

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        db.RefreshTokens.Add(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);

        return ToRecord(entity);
    }

    public async Task<RefreshTokenRecord?> FindActiveByHashAsync(string tenantId, string tokenHash, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(tokenHash)) return null;

        var now = _timeProvider.GetUtcNow().UtcDateTime;

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.RefreshTokens
            .AsNoTracking()
            .FirstOrDefaultAsync(
                t => t.TenantId == tenantId
                     && t.TokenHash == tokenHash
                     && t.RevokedAtUtc == null
                     && t.ExpiresAtUtc > now,
                cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : ToRecord(entity);
    }

    public async Task RevokeAsync(string tenantId, string tokenId, string? replacedByTokenId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(tokenId)) throw new ArgumentNullException(nameof(tokenId));

        var now = _timeProvider.GetUtcNow().UtcDateTime;

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.RefreshTokens
            .FirstOrDefaultAsync(t => t.TenantId == tenantId && t.TokenId == tokenId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        if (entity.RevokedAtUtc.HasValue)
        {
            return;
        }

        entity.RevokedAtUtc = now;
        entity.ReplacedByTokenId = replacedByTokenId;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private static RefreshTokenRecord ToRecord(RefreshTokenEntity entity)
    {
        return new RefreshTokenRecord(
            entity.TokenId,
            entity.TenantId,
            entity.UserId,
            entity.TokenHash,
            new DateTimeOffset(DateTime.SpecifyKind(entity.ExpiresAtUtc, DateTimeKind.Utc)),
            entity.RevokedAtUtc.HasValue ? new DateTimeOffset(DateTime.SpecifyKind(entity.RevokedAtUtc.Value, DateTimeKind.Utc)) : null,
            entity.ReplacedByTokenId,
            new DateTimeOffset(DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc)));
    }
}
