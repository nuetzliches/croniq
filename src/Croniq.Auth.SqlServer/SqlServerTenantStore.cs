using System;
using System.Collections.Generic;
using System.Linq;
using Croniq.Auth.Abstractions;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Auth.SqlServer;

/// <summary>
/// EF Core backed tenant metadata store for admin flows.
/// </summary>
public sealed class SqlServerTenantStore : ITenantStore
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbContextFactory;
    private readonly TimeProvider _timeProvider;

    public SqlServerTenantStore(IDbContextFactory<SqlServerDbContext> dbContextFactory, TimeProvider? timeProvider = null)
    {
        _dbContextFactory = dbContextFactory ?? throw new ArgumentNullException(nameof(dbContextFactory));
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task<TenantDescriptor> CreateAsync(TenantCreateRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.Name)) throw new ArgumentException("Name is required", nameof(request));

        var tenantId = string.IsNullOrWhiteSpace(request.TenantId)
            ? Guid.NewGuid().ToString("D")
            : request.TenantId.Trim();
        var trimmedName = request.Name.Trim();
        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var now = _timeProvider.GetUtcNow().UtcDateTime;

        var entity = await db.Tenants
            .FirstOrDefaultAsync(t => t.TenantId == tenantId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            entity = new TenantEntity
            {
                TenantId = tenantId,
                Name = trimmedName,
                IsActive = true,
                CreatedAtUtc = now,
                UpdatedAtUtc = now
            };
            db.Tenants.Add(entity);
        }
        else
        {
            entity.Name = trimmedName;
            entity.IsActive = true;
            entity.UpdatedAtUtc = now;
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return ToDescriptor(entity);
    }

    public async Task<TenantDescriptor?> GetByIdAsync(string tenantId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.Tenants
            .AsNoTracking()
            .FirstOrDefaultAsync(t => t.TenantId == tenantId.Trim(), cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : ToDescriptor(entity);
    }

    public async Task<IReadOnlyCollection<TenantDescriptor>> ListAsync(CancellationToken cancellationToken = default)
    {
        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entities = await db.Tenants
            .AsNoTracking()
            .OrderBy(t => t.CreatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return entities.Select(ToDescriptor).ToArray();
    }

    public async Task<bool> DeactivateAsync(string tenantId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.Tenants.FirstOrDefaultAsync(t => t.TenantId == tenantId.Trim(), cancellationToken).ConfigureAwait(false);
        if (entity is null)
        {
            return false;
        }

        if (!entity.IsActive)
        {
            return true;
        }

        entity.IsActive = false;
        entity.UpdatedAtUtc = _timeProvider.GetUtcNow().UtcDateTime;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return true;
    }

    private static TenantDescriptor ToDescriptor(TenantEntity entity)
    {
        var createdAt = DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc);
        return new TenantDescriptor(
            entity.TenantId,
            entity.Name,
            entity.IsActive,
            new DateTimeOffset(createdAt));
    }
}
