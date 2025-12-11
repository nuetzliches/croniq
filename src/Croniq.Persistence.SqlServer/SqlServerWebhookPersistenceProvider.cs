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
    private const int SecretByteLength = 32;
    private static readonly TimeSpan DefaultGracePeriod = TimeSpan.FromHours(24);
    private static readonly TimeSpan MaxActivationDelay = TimeSpan.FromDays(7);
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly IReadOnlyList<IWebhookEndpointChangeNotifier> _changeNotifiers;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public SqlServerWebhookPersistenceProvider(
        IDbContextFactory<SqlServerDbContext> dbFactory,
        IEnumerable<IWebhookEndpointChangeNotifier>? changeNotifiers = null)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _changeNotifiers = changeNotifiers?.ToArray() ?? Array.Empty<IWebhookEndpointChangeNotifier>();
    }

    public async Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.HookKey == hookKey, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return null;
        }

        var rules = await db.WebhookEndpointIpRules
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey && !x.IsDeleted)
            .OrderBy(x => x.Cidr)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return Map(entity, rules.Select(MapIpRule).ToList());
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

        if (rows.Count == 0)
        {
            return Array.Empty<WebhookEndpointDefinition>();
        }

        var hookKeys = rows.Select(x => x.HookKey).ToArray();
        var ruleRows = await db.WebhookEndpointIpRules
            .AsNoTracking()
            .Where(x => !x.IsDeleted
                        && x.TenantId == scope.TenantId
                        && x.EnvironmentTag == scope.EnvironmentTag
                        && hookKeys.Contains(x.HookKey))
            .OrderBy(x => x.Cidr)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var ruleLookup = ruleRows
            .GroupBy(x => x.HookKey, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(
                g => g.Key,
                g => (IReadOnlyCollection<WebhookIpRuleDefinition>)g.Select(MapIpRule).ToList(),
                StringComparer.OrdinalIgnoreCase);

        return rows
            .Select(row =>
            {
                var rules = ruleLookup.TryGetValue(row.HookKey, out var mappedRules)
                    ? mappedRules
                    : Array.Empty<WebhookIpRuleDefinition>();
                return Map(row, rules);
            })
            .ToList();
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
                MetadataJson = SerializeMetadata(request.Metadata),
                CreatedAtUtc = now,
                UpdatedAtUtc = now
            };
            db.WebhookEndpoints.Add(entity);
            await ApplySecretAsync(db, entity, request.Secret, now, TimeSpan.Zero, "system:create", null, cancellationToken).ConfigureAwait(false);
            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Created, now));
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
                await ApplySecretAsync(db, entity, request.Secret, now, TimeSpan.Zero, "system:update", null, cancellationToken).ConfigureAwait(false);
            }
            else if (!string.IsNullOrWhiteSpace(entity.Secret))
            {
                await ApplySecretAsync(db, entity, entity.Secret, now, TimeSpan.Zero, "system:update", null, cancellationToken).ConfigureAwait(false);
            }
            else
            {
                throw new InvalidOperationException($"Webhook {request.HookKey} does not have an existing secret to snapshot.");
            }

            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Updated, now));
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(request.HookKey);
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

        var now = DateTime.UtcNow;
        db.WebhookEndpoints.Remove(entity);
        db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Deleted, now));
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(hookKey);
    }

    public async Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (!string.IsNullOrWhiteSpace(request.HookKey) && request.HookKey.Length > 128)
        {
            throw new InvalidOperationException("hook key exceeds maximum length");
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            throw new InvalidOperationException($"Webhook {request.HookKey} not found.");
        }

        if (!string.Equals(entity.TenantId, request.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(entity.EnvironmentTag, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }

        var now = DateTime.UtcNow;
        var activationDelaySeconds = request.ActivateInSeconds.HasValue
            ? Math.Max(0, request.ActivateInSeconds.Value)
            : 0;

        if (activationDelaySeconds > MaxActivationDelay.TotalSeconds)
        {
            throw new InvalidOperationException($"ActivateInSeconds cannot exceed {(int)MaxActivationDelay.TotalSeconds} seconds.");
        }

        var activatedAtUtc = now.AddSeconds(activationDelaySeconds);
        var graceSeconds = request.GracePeriodSeconds.HasValue
            ? Math.Max(60, request.GracePeriodSeconds.Value)
            : (int)DefaultGracePeriod.TotalSeconds;
        var gracePeriod = TimeSpan.FromSeconds(graceSeconds);
        var secret = GenerateSecret();

        await ApplySecretAsync(db, entity, secret, activatedAtUtc, gracePeriod, request.RotatedBy ?? "system:rotate", request.Notes, cancellationToken).ConfigureAwait(false);
        entity.UpdatedAtUtc = now;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(request.HookKey);

        return new WebhookSecretRotationResult(
            entity.HookKey,
            secret,
            entity.SecretHash,
            DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
            null);
    }

    public async Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        var now = DateTime.UtcNow;
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.WebhookSecretHistory
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey
                        && x.ActivatedAtUtc <= now
                        && (x.ExpiresAtUtc == null || x.ExpiresAtUtc > now))
            .OrderBy(x => x.ActivatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return rows.Select(x => new WebhookSecretMaterial(
            x.Secret,
            x.SecretHash,
            DateTime.SpecifyKind(x.ActivatedAtUtc, DateTimeKind.Utc),
            x.ExpiresAtUtc.HasValue ? DateTime.SpecifyKind(x.ExpiresAtUtc.Value, DateTimeKind.Utc) : null)).ToList();
    }

    public async Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var endpoint = await db.WebhookEndpoints
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.HookKey == hookKey, cancellationToken)
            .ConfigureAwait(false);

        if (endpoint is null)
        {
            return Array.Empty<WebhookIpRuleDefinition>();
        }

        EnsureScope(endpoint.TenantId, endpoint.EnvironmentTag, scope);

        var rows = await db.WebhookEndpointIpRules
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey && !x.IsDeleted)
            .OrderBy(x => x.Cidr)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return rows.Select(MapIpRule).ToList();
    }

    public async Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.Cidr)) throw new ArgumentNullException(nameof(request.Cidr));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var endpoint = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey, cancellationToken)
            .ConfigureAwait(false);

        if (endpoint is null)
        {
            throw new InvalidOperationException($"Webhook {request.HookKey} not found.");
        }

        EnsureScope(endpoint.TenantId, endpoint.EnvironmentTag, new PartitionScope(request.TenantId, request.EnvironmentTag));

        var duplicate = await db.WebhookEndpointIpRules
            .AnyAsync(x => x.HookKey == request.HookKey && x.Cidr == request.Cidr && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (duplicate)
        {
            throw new InvalidOperationException($"CIDR {request.Cidr} already exists for webhook {request.HookKey}.");
        }

        var now = DateTime.UtcNow;
        var entity = new WebhookEndpointIpRuleEntity
        {
            HookKey = request.HookKey,
            TenantId = request.TenantId,
            EnvironmentTag = request.EnvironmentTag,
            Cidr = request.Cidr,
            Description = request.Description,
            CreatedBy = request.CreatedBy,
            CreatedAtUtc = now,
            UpdatedAtUtc = now,
            IsDeleted = false
        };

        db.WebhookEndpointIpRules.Add(entity);
        db.WebhookEndpointEvents.Add(CreateEvent(
            endpoint.HookKey,
            endpoint.TenantId,
            endpoint.EnvironmentTag,
            WebhookEndpointEventTypes.Updated,
            now,
            request.CreatedBy,
            request.CorrelationId));
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(request.HookKey);

        return MapIpRule(entity);
    }

    public async Task DeleteIpRuleAsync(long ruleId, PartitionScope scope, string? deletedBy, string? correlationId, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpointIpRules
            .FirstOrDefaultAsync(x => x.Id == ruleId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        EnsureScope(entity.TenantId, entity.EnvironmentTag, scope);
        var now = DateTime.UtcNow;
        db.WebhookEndpointIpRules.Remove(entity);
        db.WebhookEndpointEvents.Add(CreateEvent(
            entity.HookKey,
            entity.TenantId,
            entity.EnvironmentTag,
            WebhookEndpointEventTypes.Updated,
            now,
            deletedBy,
            correlationId));
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(entity.HookKey);
    }

    private static void EnsureScope(string tenantId, string environmentTag, PartitionScope scope)
    {
        if (!string.Equals(tenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(environmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }
    }

    private async Task ApplySecretAsync(
        SqlServerDbContext db,
        WebhookEndpointEntity entity,
        string secret,
        DateTime activatedAtUtc,
        TimeSpan gracePeriod,
        string? rotatedBy,
        string? notes,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(secret))
        {
            throw new InvalidOperationException("Secret value is required.");
        }

        var secretHash = ComputeSecretHash(secret);
        entity.Secret = secret;
        entity.SecretHash = secretHash;

        var activeRows = await db.WebhookSecretHistory
            .Where(x => x.HookKey == entity.HookKey
                        && x.TenantId == entity.TenantId
                        && x.EnvironmentTag == entity.EnvironmentTag
                        && (x.ExpiresAtUtc == null || x.ExpiresAtUtc > activatedAtUtc))
            .OrderByDescending(x => x.ActivatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);
        var graceExpiry = gracePeriod <= TimeSpan.Zero
            ? activatedAtUtc
            : activatedAtUtc.Add(gracePeriod);

        if (activeRows.Count > 0)
        {
            WebhookSecretHistoryEntity? graceTarget = null;
            foreach (var row in activeRows)
            {
                if (row.ActivatedAtUtc <= activatedAtUtc)
                {
                    if (graceTarget is null)
                    {
                        graceTarget = row;
                        row.ExpiresAtUtc = graceExpiry;
                    }
                    else
                    {
                        row.ExpiresAtUtc = activatedAtUtc;
                    }
                }
                else
                {
                    row.ExpiresAtUtc = activatedAtUtc;
                }
            }
        }

        db.WebhookSecretHistory.Add(new WebhookSecretHistoryEntity
        {
            HookKey = entity.HookKey,
            TenantId = entity.TenantId,
            EnvironmentTag = entity.EnvironmentTag,
            Secret = secret,
            SecretHash = secretHash,
            ActivatedAtUtc = DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
            ExpiresAtUtc = null,
            RotatedBy = rotatedBy,
            Notes = notes
        });
    }

    private void NotifyEndpointChanged(string hookKey)
    {
        if (_changeNotifiers.Count == 0 || string.IsNullOrWhiteSpace(hookKey))
        {
            return;
        }

        foreach (var notifier in _changeNotifiers)
        {
            notifier.NotifyChanged(hookKey);
        }
    }

    private static string GenerateSecret()
    {
        Span<byte> buffer = stackalloc byte[SecretByteLength];
        RandomNumberGenerator.Fill(buffer);
        return Convert.ToHexString(buffer).ToLowerInvariant();
    }

    private static WebhookEndpointEventEntity CreateEvent(
        string hookKey,
        string tenantId,
        string environmentTag,
        string eventType,
        DateTime occurredAtUtc,
        string? actor = null,
        string? correlationId = null)
    {
        return new WebhookEndpointEventEntity
        {
            HookKey = hookKey,
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            EventType = eventType,
            OccurredAtUtc = DateTime.SpecifyKind(occurredAtUtc, DateTimeKind.Utc),
            Actor = string.IsNullOrWhiteSpace(actor) ? null : actor,
            CorrelationId = string.IsNullOrWhiteSpace(correlationId) ? null : correlationId
        };
    }

    private WebhookEndpointDefinition Map(WebhookEndpointEntity entity, IReadOnlyCollection<WebhookIpRuleDefinition>? ipRules = null)
    {
        var metadata = DeserializeMetadata(entity.MetadataJson);
        return new WebhookEndpointDefinition(
            entity.HookKey,
            entity.JobKey,
            entity.Secret,
            entity.Enabled,
            entity.RequireSignature,
            entity.RequestsPerMinute,
            entity.TenantId,
            entity.EnvironmentTag,
            metadata,
            ipRules ?? Array.Empty<WebhookIpRuleDefinition>(),
            entity.SignatureVersion,
            new DateTimeOffset(DateTime.SpecifyKind(entity.CreatedAtUtc, DateTimeKind.Utc)),
            new DateTimeOffset(DateTime.SpecifyKind(entity.UpdatedAtUtc, DateTimeKind.Utc)));
    }

    private static WebhookIpRuleDefinition MapIpRule(WebhookEndpointIpRuleEntity entity)
    {
        return new WebhookIpRuleDefinition(
            entity.Id,
            entity.HookKey,
            entity.TenantId,
            entity.EnvironmentTag,
            entity.Cidr,
            entity.Description,
            entity.CreatedBy,
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
