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

public sealed class SqlServerWebhookDeadLetterStore : IWebhookDeadLetterStore
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public SqlServerWebhookDeadLetterStore(IDbContextFactory<SqlServerDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task<long> CreateAsync(WebhookDeadLetterCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = new WebhookDeadLetterEntity
        {
            HookKey = request.HookKey,
            JobKey = request.JobKey,
            TenantId = request.TenantId,
            EnvironmentTag = request.EnvironmentTag,
            Payload = request.Payload,
            HeadersJson = SerializeDictionary(request.Headers),
            MetadataJson = SerializeDictionary(request.Metadata),
            FailureReason = request.FailureReason,
            StatusCode = request.StatusCode,
            ErrorDetails = request.ErrorDetails,
            Attempts = 0,
            CreatedAtUtc = DateTime.UtcNow,
            ExpiresAtUtc = request.ExpiresAtUtc?.UtcDateTime
        };

        db.WebhookDeadLetters.Add(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return entity.Id;
    }

    public async Task<IReadOnlyCollection<WebhookDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entities = await db.WebhookDeadLetters
            .AsNoTracking()
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .OrderByDescending(x => x.CreatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return entities.Select(Map).ToList();
    }

    public async Task<WebhookDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookDeadLetters
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.Id == id && x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : Map(entity);
    }

    public async Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookDeadLetters
            .FirstOrDefaultAsync(x => x.Id == id, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        EnsureScope(entity, scope);
        db.WebhookDeadLetters.Remove(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task RecordFailureAsync(long id, PartitionScope scope, WebhookDeadLetterFailure failure, CancellationToken cancellationToken)
    {
        if (failure is null) throw new ArgumentNullException(nameof(failure));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookDeadLetters
            .FirstOrDefaultAsync(x => x.Id == id, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        EnsureScope(entity, scope);
        entity.Attempts += 1;
        entity.FailureReason = failure.FailureReason;
        entity.StatusCode = failure.StatusCode;
        entity.ErrorDetails = failure.ErrorDetails;
        entity.LastAttemptAtUtc = DateTime.UtcNow;
        entity.NextAttemptAtUtc = failure.NextAttemptAtUtc?.UtcDateTime;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private WebhookDeadLetterEntry Map(WebhookDeadLetterEntity entity)
    {
        return new WebhookDeadLetterEntry(
            entity.Id,
            entity.HookKey,
            entity.JobKey,
            entity.TenantId,
            entity.EnvironmentTag,
            entity.Payload,
            DeserializeDictionary(entity.HeadersJson),
            DeserializeDictionary(entity.MetadataJson),
            entity.FailureReason,
            entity.Attempts,
            entity.StatusCode,
            entity.ErrorDetails,
            new DateTimeOffset(DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc)),
            entity.LastAttemptAtUtc is null ? null : new DateTimeOffset(DateTime.SpecifyKind(entity.LastAttemptAtUtc.Value, DateTimeKind.Utc)),
            entity.NextAttemptAtUtc is null ? null : new DateTimeOffset(DateTime.SpecifyKind(entity.NextAttemptAtUtc.Value, DateTimeKind.Utc)),
            entity.ExpiresAtUtc is null ? null : new DateTimeOffset(DateTime.SpecifyKind(entity.ExpiresAtUtc.Value, DateTimeKind.Utc)));
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

    private static void EnsureScope(WebhookDeadLetterEntity entity, PartitionScope scope)
    {
        if (!string.Equals(entity.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(entity.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Webhook dead letter scope mismatch.");
        }
    }
}
