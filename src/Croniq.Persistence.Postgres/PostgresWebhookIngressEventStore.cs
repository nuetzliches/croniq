using System;
using System.Collections.Generic;
using System.Data;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.Postgres;
using Croniq.Data.Postgres.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.Postgres;

public sealed class PostgresWebhookIngressEventStore : IWebhookIngressEventStore
{
    private const string StatusPending = "Pending";
    private const string StatusLeased = "Leased";
    private const string StatusDelivered = "Delivered";
    private const string StatusFailed = "Failed";
    private const int ErrorMaxLength = 1024;
    private readonly IDbContextFactory<PostgresDbContext> _dbFactory;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public PostgresWebhookIngressEventStore(IDbContextFactory<PostgresDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task EnqueueAsync(WebhookIngressEventCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.EventId)) throw new ArgumentNullException(nameof(request.EventId));
        if (string.IsNullOrWhiteSpace(request.HookKey)) throw new ArgumentNullException(nameof(request.HookKey));
        if (string.IsNullOrWhiteSpace(request.JobKey)) throw new ArgumentNullException(nameof(request.JobKey));
        if (string.IsNullOrWhiteSpace(request.TenantId)) throw new ArgumentNullException(nameof(request.TenantId));
        if (string.IsNullOrWhiteSpace(request.EnvironmentTag)) throw new ArgumentNullException(nameof(request.EnvironmentTag));

        var nowUtc = DateTime.UtcNow;
        var entity = new WebhookIngressEventEntity
        {
            EventId = request.EventId,
            HookKey = request.HookKey,
            JobKey = request.JobKey,
            TenantId = request.TenantId,
            EnvironmentTag = request.EnvironmentTag,
            Payload = request.Payload ?? string.Empty,
            HeadersJson = SerializeDictionary(request.Headers),
            MetadataJson = SerializeDictionary(request.Metadata),
            ReceivedAtUtc = request.ReceivedAtUtc.UtcDateTime,
            Status = StatusPending,
            AttemptCount = 0,
            CreatedAtUtc = nowUtc,
            UpdatedAtUtc = nowUtc
        };

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        db.WebhookIngressEvents.Add(entity);

        try
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (DbUpdateException)
        {
            var exists = await db.WebhookIngressEvents
                .AsNoTracking()
                .AnyAsync(x => x.EventId == request.EventId, cancellationToken)
                .ConfigureAwait(false);
            if (!exists)
            {
                throw;
            }
        }
    }

    public async Task<IReadOnlyCollection<WebhookIngressLease>> AcquireAsync(
        WebhookIngressAcquireRequest request,
        CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (request.MaxCount <= 0) return Array.Empty<WebhookIngressLease>();
        if (request.LeaseDuration <= TimeSpan.Zero) throw new ArgumentOutOfRangeException(nameof(request.LeaseDuration));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var strategy = db.Database.CreateExecutionStrategy();

        return await strategy.ExecuteAsync(async () =>
        {
            await using var tx = await db.Database.BeginTransactionAsync(IsolationLevel.Serializable, cancellationToken).ConfigureAwait(false);

            var nowUtc = request.NowUtc.UtcDateTime;
            var expiresAt = nowUtc.Add(request.LeaseDuration);

            var entities = await db.WebhookIngressEvents
                .Where(x => x.TenantId == request.Scope.TenantId && x.EnvironmentTag == request.Scope.EnvironmentTag)
                .Where(x => x.Status == StatusPending || x.Status == StatusLeased)
                .Where(x => x.LeaseExpiresAtUtc == null || x.LeaseExpiresAtUtc <= nowUtc)
                .OrderBy(x => x.ReceivedAtUtc)
                .ThenBy(x => x.Id)
                .Take(request.MaxCount)
                .ToListAsync(cancellationToken)
                .ConfigureAwait(false);

            foreach (var entity in entities)
            {
                entity.LeaseId = Guid.NewGuid().ToString("N");
                entity.LeaseExpiresAtUtc = expiresAt;
                entity.Status = StatusLeased;
                entity.AttemptCount += 1;
                entity.UpdatedAtUtc = nowUtc;
            }

            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
            await tx.CommitAsync(cancellationToken).ConfigureAwait(false);

            return (IReadOnlyCollection<WebhookIngressLease>)entities.Select(MapLease).ToList();
        }).ConfigureAwait(false);
    }

    public async Task<bool> TryExtendLeaseAsync(WebhookIngressLeaseRenewal renewal, CancellationToken cancellationToken)
    {
        if (renewal is null) throw new ArgumentNullException(nameof(renewal));
        if (string.IsNullOrWhiteSpace(renewal.EventId)) throw new ArgumentNullException(nameof(renewal.EventId));
        if (string.IsNullOrWhiteSpace(renewal.LeaseId)) throw new ArgumentNullException(nameof(renewal.LeaseId));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookIngressEvents
            .FirstOrDefaultAsync(x => x.EventId == renewal.EventId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null || !string.Equals(entity.LeaseId, renewal.LeaseId, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (entity.LeaseExpiresAtUtc is null || entity.LeaseExpiresAtUtc <= renewal.RenewedAtUtc.UtcDateTime)
        {
            return false;
        }

        entity.LeaseExpiresAtUtc = renewal.LeaseExpiresAtUtc.UtcDateTime;
        entity.UpdatedAtUtc = renewal.RenewedAtUtc.UtcDateTime;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return true;
    }

    public async Task AcknowledgeAsync(WebhookIngressAck ack, CancellationToken cancellationToken)
    {
        if (ack is null) throw new ArgumentNullException(nameof(ack));
        if (string.IsNullOrWhiteSpace(ack.EventId)) throw new ArgumentNullException(nameof(ack.EventId));
        var leaseId = string.IsNullOrWhiteSpace(ack.LeaseId) ? null : ack.LeaseId.Trim();

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookIngressEvents
            .FirstOrDefaultAsync(x => x.EventId == ack.EventId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        if (leaseId is null)
        {
            if (!string.IsNullOrWhiteSpace(entity.LeaseId))
            {
                return;
            }
        }
        else if (!string.Equals(entity.LeaseId, leaseId, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        entity.Status = ack.Succeeded ? StatusDelivered : StatusFailed;
        entity.LeaseId = null;
        entity.LeaseExpiresAtUtc = null;
        entity.LastError = ack.Succeeded ? null : TruncateError(ack.ErrorMessage);
        entity.UpdatedAtUtc = ack.AcknowledgedAtUtc.UtcDateTime;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task NackAsync(WebhookIngressNack nack, CancellationToken cancellationToken)
    {
        if (nack is null) throw new ArgumentNullException(nameof(nack));
        if (string.IsNullOrWhiteSpace(nack.EventId)) throw new ArgumentNullException(nameof(nack.EventId));
        if (string.IsNullOrWhiteSpace(nack.LeaseId)) throw new ArgumentNullException(nameof(nack.LeaseId));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookIngressEvents
            .FirstOrDefaultAsync(x => x.EventId == nack.EventId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null || !string.Equals(entity.LeaseId, nack.LeaseId, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        entity.Status = StatusPending;
        entity.LeaseId = null;
        entity.LeaseExpiresAtUtc = null;
        entity.LastError = TruncateError(nack.Reason);
        entity.UpdatedAtUtc = nack.NackedAtUtc.UtcDateTime;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private WebhookIngressLease MapLease(WebhookIngressEventEntity entity)
    {
        return new WebhookIngressLease(
            entity.EventId,
            entity.LeaseId ?? string.Empty,
            new DateTimeOffset(DateTime.SpecifyKind(entity.LeaseExpiresAtUtc ?? DateTime.UtcNow, DateTimeKind.Utc)),
            entity.HookKey,
            entity.JobKey,
            entity.TenantId,
            entity.EnvironmentTag,
            entity.Payload,
            DeserializeDictionary(entity.HeadersJson),
            DeserializeDictionary(entity.MetadataJson),
            new DateTimeOffset(DateTime.SpecifyKind(entity.ReceivedAtUtc, DateTimeKind.Utc)));
    }

    private string? SerializeDictionary(IReadOnlyDictionary<string, string>? values)
    {
        if (values is null || values.Count == 0)
        {
            return null;
        }

        return JsonSerializer.Serialize(values, _jsonOptions);
    }

    private IReadOnlyDictionary<string, string>? DeserializeDictionary(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
        {
            return null;
        }

        return JsonSerializer.Deserialize<Dictionary<string, string>>(json, _jsonOptions);
    }

    private static string? TruncateError(string? error)
    {
        if (string.IsNullOrWhiteSpace(error))
        {
            return null;
        }

        return error.Length <= ErrorMaxLength ? error : error[..ErrorMaxLength];
    }
}
