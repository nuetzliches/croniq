using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Options;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerWorkerStore : IWorkerStore
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly WorkerStoreOptions _options;

    public SqlServerWorkerStore(IDbContextFactory<SqlServerDbContext> dbFactory, IOptions<WorkerStoreOptions> options)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _options = options?.Value ?? new WorkerStoreOptions();
        _options.Normalize();
    }

    public async Task UpsertHeartbeatAsync(WorkerHeartbeat heartbeat, CancellationToken cancellationToken)
    {
        if (heartbeat is null) throw new ArgumentNullException(nameof(heartbeat));
        if (string.IsNullOrWhiteSpace(heartbeat.InstanceId)) throw new ArgumentNullException(nameof(heartbeat.InstanceId));

        var scope = heartbeat.Scope;
        var instanceId = heartbeat.InstanceId.Trim();
        var lastSeenAtUtc = heartbeat.SeenAtUtc.UtcDateTime;
        var expiresAtUtc = heartbeat.SeenAtUtc.Add(_options.OnlineTtl).UtcDateTime;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, heartbeat.SeenAtUtc, cancellationToken).ConfigureAwait(false);

        var updated = await db.WorkerInstances
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && x.InstanceId == instanceId)
            .ExecuteUpdateAsync(setters => setters
                .SetProperty(x => x.LastSeenAtUtc, lastSeenAtUtc)
                .SetProperty(x => x.ExpiresAtUtc, expiresAtUtc)
                .SetProperty(x => x.MetadataJson, heartbeat.MetadataJson)
                .SetProperty(x => x.UpdatedAtUtc, DateTime.UtcNow), cancellationToken)
            .ConfigureAwait(false);

        if (updated > 0)
        {
            return;
        }

        db.WorkerInstances.Add(new WorkerInstanceEntity
        {
            TenantId = scope.TenantId,
            EnvironmentTag = scope.EnvironmentTag,
            InstanceId = instanceId,
            LastSeenAtUtc = lastSeenAtUtc,
            ExpiresAtUtc = expiresAtUtc,
            MetadataJson = heartbeat.MetadataJson,
            CreatedAtUtc = DateTime.UtcNow,
            UpdatedAtUtc = DateTime.UtcNow
        });

        try
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (DbUpdateException)
        {
            // Race with another worker heartbeat insert. Retry as update.
            await db.WorkerInstances
                .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && x.InstanceId == instanceId)
                .ExecuteUpdateAsync(setters => setters
                    .SetProperty(x => x.LastSeenAtUtc, lastSeenAtUtc)
                    .SetProperty(x => x.ExpiresAtUtc, expiresAtUtc)
                    .SetProperty(x => x.MetadataJson, heartbeat.MetadataJson)
                    .SetProperty(x => x.UpdatedAtUtc, DateTime.UtcNow), cancellationToken)
                .ConfigureAwait(false);
        }
    }

    public async Task<IReadOnlyCollection<WorkerStatus>> ListAsync(WorkerQuery query, CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var scope = query.Scope;
        var now = query.NowUtc;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, now, cancellationToken).ConfigureAwait(false);

        var rows = await db.WorkerInstances
            .AsNoTracking()
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .OrderBy(x => x.InstanceId)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return rows.Select(x =>
        {
            var lastSeen = new DateTimeOffset(DateTime.SpecifyKind(x.LastSeenAtUtc, DateTimeKind.Utc));
            var expiresAt = new DateTimeOffset(DateTime.SpecifyKind(x.ExpiresAtUtc, DateTimeKind.Utc));
            return new WorkerStatus(
                x.InstanceId,
                lastSeen,
                expiresAt,
                expiresAt > now,
                x.MetadataJson);
        }).ToList();
    }

    private static Task<int> PruneExpiredAsync(SqlServerDbContext db, PartitionScope scope, DateTimeOffset nowUtc, CancellationToken cancellationToken)
    {
        return db.WorkerInstances
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && x.ExpiresAtUtc <= nowUtc.UtcDateTime)
            .ExecuteDeleteAsync(cancellationToken);
    }
}
