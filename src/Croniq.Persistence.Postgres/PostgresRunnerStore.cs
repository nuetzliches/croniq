using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.Postgres;
using Croniq.Data.Postgres.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Options;

namespace Croniq.Persistence.Postgres;

public sealed class PostgresRunnerStore : IRunnerStore
{
    private readonly IDbContextFactory<PostgresDbContext> _dbFactory;
    private readonly RunnerStoreOptions _options;

    public PostgresRunnerStore(IDbContextFactory<PostgresDbContext> dbFactory, IOptions<RunnerStoreOptions> options)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _options = options?.Value ?? new RunnerStoreOptions();
        _options.Normalize();
    }

    public async Task UpsertHeartbeatAsync(RunnerHeartbeat heartbeat, CancellationToken cancellationToken)
    {
        if (heartbeat is null) throw new ArgumentNullException(nameof(heartbeat));
        if (string.IsNullOrWhiteSpace(heartbeat.RunnerId)) throw new ArgumentNullException(nameof(heartbeat.RunnerId));

        var scope = heartbeat.Scope;
        var runnerId = heartbeat.RunnerId.Trim();
        var lastSeenAtUtc = heartbeat.SeenAtUtc.UtcDateTime;
        var expiresAtUtc = heartbeat.SeenAtUtc.Add(_options.OnlineTtl).UtcDateTime;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, heartbeat.SeenAtUtc, cancellationToken).ConfigureAwait(false);

        var updated = await db.Runners
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && x.RunnerId == runnerId)
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

        db.Runners.Add(new RunnerEntity
        {
            TenantId = scope.TenantId,
            EnvironmentTag = scope.EnvironmentTag,
            RunnerId = runnerId,
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
            // Race with another runner heartbeat insert. Retry as update.
            await db.Runners
                .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && x.RunnerId == runnerId)
                .ExecuteUpdateAsync(setters => setters
                    .SetProperty(x => x.LastSeenAtUtc, lastSeenAtUtc)
                    .SetProperty(x => x.ExpiresAtUtc, expiresAtUtc)
                    .SetProperty(x => x.MetadataJson, heartbeat.MetadataJson)
                    .SetProperty(x => x.UpdatedAtUtc, DateTime.UtcNow), cancellationToken)
                .ConfigureAwait(false);
        }
    }

    public async Task<IReadOnlyCollection<RunnerStatus>> ListAsync(RunnerQuery query, CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var scope = query.Scope;
        var now = query.NowUtc;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, now, cancellationToken).ConfigureAwait(false);

        var rows = await db.Runners
            .AsNoTracking()
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .OrderBy(x => x.RunnerId)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return rows.Select(x =>
        {
            var lastSeen = new DateTimeOffset(DateTime.SpecifyKind(x.LastSeenAtUtc, DateTimeKind.Utc));
            var expiresAt = new DateTimeOffset(DateTime.SpecifyKind(x.ExpiresAtUtc, DateTimeKind.Utc));
            return new RunnerStatus(
                x.RunnerId,
                lastSeen,
                expiresAt,
                expiresAt > now,
                x.MetadataJson);
        }).ToList();
    }

    private static Task<int> PruneExpiredAsync(PostgresDbContext db, PartitionScope scope, DateTimeOffset nowUtc, CancellationToken cancellationToken)
    {
        return db.Runners
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && x.ExpiresAtUtc <= nowUtc.UtcDateTime)
            .ExecuteDeleteAsync(cancellationToken);
    }
}
