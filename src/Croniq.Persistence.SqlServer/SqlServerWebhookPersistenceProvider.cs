using System;
using System.Collections.Generic;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public SqlServerWebhookPersistenceProvider(
        IDbContextFactory<SqlServerDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.HookKey == hookKey, cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : Map(entity);
    }

    public async Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.WebhookEndpoints
            .AsNoTracking()
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .OrderBy(x => x.HookKey)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return rows.Select(Map).ToList();
    }

    public async Task UpsertAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (!JobKey.TryParse(request.JobKey, out var jobKey))
        {
            throw new InvalidOperationException($"JobKey '{request.JobKey}' is invalid.");
        }

        if (!string.Equals(jobKey.TenantId, request.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(jobKey.EnvironmentTag, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("JobKey tenant/environment must match webhook scope.");
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey, cancellationToken)
            .ConfigureAwait(false);

        var now = DateTime.UtcNow;
        if (entity is null)
        {
            if (string.IsNullOrWhiteSpace(request.Secret))
            {
                throw new InvalidOperationException("Secret is required when creating a webhook endpoint.");
            }

            entity = new WebhookEndpointEntity
            {
                HookKey = request.HookKey,
                TenantId = request.TenantId,
                EnvironmentTag = request.EnvironmentTag,
                JobKey = request.JobKey,
                RequestsPerMinute = request.RequestsPerMinute,
                Enabled = request.Enabled,
                RequireSignature = request.RequireSignature,
                SignatureVersion = request.SignatureVersion,
                Secret = request.Secret,
                SecretHash = ComputeSecretHash(request.Secret),
                MetadataJson = SerializeMetadata(request.Metadata),
                CreatedAtUtc = now,
                UpdatedAtUtc = now
            };
            db.WebhookEndpoints.Add(entity);
        }
        else
        {
            if (!string.Equals(entity.TenantId, request.TenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(entity.EnvironmentTag, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            {
                throw new InvalidOperationException("Webhook scope mismatch.");
            }

            entity.JobKey = request.JobKey;
            entity.RequestsPerMinute = request.RequestsPerMinute;
            entity.Enabled = request.Enabled;
            entity.RequireSignature = request.RequireSignature;
            entity.SignatureVersion = request.SignatureVersion;
            entity.MetadataJson = SerializeMetadata(request.Metadata);
            entity.UpdatedAtUtc = now;

            if (!string.IsNullOrWhiteSpace(request.Secret))
            {
                entity.Secret = request.Secret;
                entity.SecretHash = ComputeSecretHash(request.Secret);
            }
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task DeleteAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == hookKey, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        if (!string.Equals(entity.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(entity.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }

        db.WebhookEndpoints.Remove(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private WebhookEndpointDefinition Map(WebhookEndpointEntity entity)
    {
        return new WebhookEndpointDefinition(
            entity.HookKey,
            entity.JobKey,
            entity.Secret,
            entity.Enabled,
            entity.RequireSignature,
            entity.RequestsPerMinute,
            entity.TenantId,
            entity.EnvironmentTag,
            DeserializeMetadata(entity.MetadataJson),
            entity.SignatureVersion,
            new DateTimeOffset(DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc)),
            new DateTimeOffset(DateTime.SpecifyKind(entity.UpdatedAtUtc, DateTimeKind.Utc)));
    }

    private string? SerializeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        return JsonSerializer.Serialize(metadata, _jsonOptions);
    }

    private IReadOnlyDictionary<string, string>? DeserializeMetadata(string? metadataJson)
    {
        if (string.IsNullOrWhiteSpace(metadataJson))
        {
            return null;
        }

        return JsonSerializer.Deserialize<Dictionary<string, string>>(metadataJson, _jsonOptions);
    }

    private static string ComputeSecretHash(string secret)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(secret);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
