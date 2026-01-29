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

public sealed class SqlServerRunnerStore : IRunnerStore
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly RunnerStoreOptions _options;

    public SqlServerRunnerStore(IDbContextFactory<SqlServerDbContext> dbFactory, IOptions<RunnerStoreOptions> options)
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
        var nowUtc = DateTime.UtcNow;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, heartbeat.SeenAtUtc, cancellationToken).ConfigureAwait(false);

        await db.Database.ExecuteSqlInterpolatedAsync($@"
MERGE [croniq].[Runners] AS target
USING (SELECT {scope.TenantId} AS TenantId, {scope.EnvironmentTag} AS EnvironmentTag, {runnerId} AS RunnerId) AS source
ON target.TenantId = source.TenantId AND target.EnvironmentTag = source.EnvironmentTag AND target.RunnerId = source.RunnerId
WHEN MATCHED THEN
    UPDATE SET
        LastSeenAtUtc = {lastSeenAtUtc},
        ExpiresAtUtc = {expiresAtUtc},
        MetadataJson = {heartbeat.MetadataJson},
        UpdatedAtUtc = {nowUtc}
WHEN NOT MATCHED THEN
    INSERT (TenantId, EnvironmentTag, RunnerId, LastSeenAtUtc, ExpiresAtUtc, MetadataJson, CreatedAtUtc, UpdatedAtUtc)
    VALUES ({scope.TenantId}, {scope.EnvironmentTag}, {runnerId}, {lastSeenAtUtc}, {expiresAtUtc}, {heartbeat.MetadataJson}, {nowUtc}, {nowUtc});
", cancellationToken).ConfigureAwait(false);
    }

    public async Task<IReadOnlyCollection<RunnerStatus>> ListAsync(RunnerQuery query, CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var scope = query.Scope;
        var now = query.NowUtc;
        var includeOffline = query.IncludeOffline;
        var nowUtc = now.UtcDateTime;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, now, cancellationToken).ConfigureAwait(false);

        var rowsQuery = db.Runners
            .AsNoTracking()
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag);

        if (!includeOffline)
        {
            rowsQuery = rowsQuery.Where(x => x.ExpiresAtUtc > nowUtc);
        }

        var rows = await rowsQuery
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

    public async Task<RunnerStatus?> TryGetAsync(RunnerLookup lookup, CancellationToken cancellationToken)
    {
        if (lookup is null) throw new ArgumentNullException(nameof(lookup));

        var scope = lookup.Scope;
        var now = lookup.NowUtc;
        var runnerId = lookup.RunnerId?.Trim();
        if (string.IsNullOrWhiteSpace(runnerId))
        {
            return null;
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        await PruneExpiredAsync(db, scope, now, cancellationToken).ConfigureAwait(false);

        var row = await db.Runners
            .AsNoTracking()
            .FirstOrDefaultAsync(x =>
                x.TenantId == scope.TenantId
                && x.EnvironmentTag == scope.EnvironmentTag
                && x.RunnerId == runnerId, cancellationToken)
            .ConfigureAwait(false);

        if (row is null)
        {
            return null;
        }

        var lastSeen = new DateTimeOffset(DateTime.SpecifyKind(row.LastSeenAtUtc, DateTimeKind.Utc));
        var expiresAt = new DateTimeOffset(DateTime.SpecifyKind(row.ExpiresAtUtc, DateTimeKind.Utc));
        if (expiresAt <= now)
        {
            return null;
        }

        return new RunnerStatus(
            row.RunnerId,
            lastSeen,
            expiresAt,
            IsOnline: true,
            row.MetadataJson);
    }

    public async Task<bool> DeleteAsync(RunnerLookup lookup, CancellationToken cancellationToken)
    {
        if (lookup is null) throw new ArgumentNullException(nameof(lookup));

        var scope = lookup.Scope;
        var runnerId = lookup.RunnerId?.Trim();
        if (string.IsNullOrWhiteSpace(runnerId))
        {
            return false;
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        var removed = await db.Runners
            .Where(x => x.TenantId == scope.TenantId
                && x.EnvironmentTag == scope.EnvironmentTag
                && x.RunnerId == runnerId)
            .ExecuteDeleteAsync(cancellationToken)
            .ConfigureAwait(false);

        return removed > 0;
    }

    private Task<int> PruneExpiredAsync(SqlServerDbContext db, PartitionScope scope, DateTimeOffset nowUtc, CancellationToken cancellationToken)
    {
        var retentionCutoffUtc = nowUtc.Add(-_options.OfflineRetentionTtl).UtcDateTime;
        return db.Runners
            .Where(x => x.TenantId == scope.TenantId
                && x.EnvironmentTag == scope.EnvironmentTag
                && x.LastSeenAtUtc <= retentionCutoffUtc)
            .ExecuteDeleteAsync(cancellationToken);
    }
}
