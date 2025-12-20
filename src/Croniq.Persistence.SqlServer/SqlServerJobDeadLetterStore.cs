using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerJobDeadLetterStore : IJobDeadLetterStore
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public SqlServerJobDeadLetterStore(IDbContextFactory<SqlServerDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task<IReadOnlyCollection<JobDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entities = await db.DeadLetters
            .AsNoTracking()
            .Include(x => x.Trigger)
            .ThenInclude(t => t.Job)
            .Where(x => x.Trigger.Job.TenantId == scope.TenantId && x.Trigger.Job.EnvironmentTag == scope.EnvironmentTag)
            .OrderByDescending(x => x.CreatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return entities.Select(Map).ToList();
    }

    public async Task<JobDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.DeadLetters
            .AsNoTracking()
            .Include(x => x.Trigger)
            .ThenInclude(t => t.Job)
            .FirstOrDefaultAsync(x => x.Id == id && x.Trigger.Job.TenantId == scope.TenantId && x.Trigger.Job.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : Map(entity);
    }

    public async Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.DeadLetters
            .Include(x => x.Trigger)
            .ThenInclude(t => t.Job)
            .FirstOrDefaultAsync(x => x.Id == id, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        EnsureScope(entity, scope);
        db.DeadLetters.Remove(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private JobDeadLetterEntry Map(DeadLetterEntity entity)
    {
        var trigger = entity.Trigger;
        var job = trigger.Job;

        return new JobDeadLetterEntry(
            entity.Id,
            trigger.TriggerKey,
            trigger.JobKey,
            job.TenantId,
            job.EnvironmentTag,
            new DateTimeOffset(DateTime.SpecifyKind(entity.FireAtUtc, DateTimeKind.Utc)),
            entity.Reason,
            entity.Payload,
            DeserializeMetadata(entity.MetadataJson),
            new DateTimeOffset(DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc)),
            entity.ExpiresAtUtc is null ? null : new DateTimeOffset(DateTime.SpecifyKind(entity.ExpiresAtUtc.Value, DateTimeKind.Utc)));
    }

    private IReadOnlyDictionary<string, string>? DeserializeMetadata(string? metadataJson)
    {
        if (string.IsNullOrWhiteSpace(metadataJson))
        {
            return null;
        }

        return JsonSerializer.Deserialize<Dictionary<string, string>>(metadataJson, _jsonOptions);
    }

    private static void EnsureScope(DeadLetterEntity entity, PartitionScope scope)
    {
        var job = entity.Trigger.Job;
        if (!string.Equals(job.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(job.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Dead letter scope mismatch.");
        }
    }
}
