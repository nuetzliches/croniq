using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.Postgres;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.Postgres;

/// <summary>
/// Postgres-backed changefeed source for webhook endpoint events.
/// </summary>
public sealed class PostgresWebhookEndpointChangefeed : IWebhookEndpointChangefeed
{
    private readonly IDbContextFactory<PostgresDbContext> _dbFactory;

    public PostgresWebhookEndpointChangefeed(IDbContextFactory<PostgresDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task<IReadOnlyCollection<WebhookEndpointEvent>> FetchAsync(long afterEventId, int maxBatchSize, CancellationToken cancellationToken)
    {
        var take = Math.Max(1, maxBatchSize);
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var events = await db.WebhookEndpointEvents
            .AsNoTracking()
            .Where(x => x.Id > afterEventId)
            .OrderBy(x => x.Id)
            .Take(take)
            .Select(x => new WebhookEndpointEvent(
                x.Id,
                x.HookKey,
                x.TenantId,
                x.EnvironmentTag,
                x.EventType,
                DateTime.SpecifyKind(x.OccurredAtUtc, DateTimeKind.Utc),
                x.Actor,
                x.CorrelationId))
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return events;
    }
}
