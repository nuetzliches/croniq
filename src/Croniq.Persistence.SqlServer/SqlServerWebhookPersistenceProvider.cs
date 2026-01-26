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
using Microsoft.AspNetCore.DataProtection;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private const string SecretProtectionPurpose = "Croniq.Webhooks.Secret.v1";
    private const int SecretByteLength = 32;
    private static readonly TimeSpan DefaultGracePeriod = TimeSpan.FromHours(24);
    private static readonly TimeSpan MaxActivationDelay = TimeSpan.FromDays(7);
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly IReadOnlyList<IWebhookEndpointChangeNotifier> _changeNotifiers;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private readonly IDataProtector _secretProtector;

    public SqlServerWebhookPersistenceProvider(
        IDbContextFactory<SqlServerDbContext> dbFactory,
        IDataProtectionProvider dataProtectionProvider,
        IEnumerable<IWebhookEndpointChangeNotifier>? changeNotifiers = null)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _secretProtector = (dataProtectionProvider ?? throw new ArgumentNullException(nameof(dataProtectionProvider)))
            .CreateProtector(SecretProtectionPurpose);
        _changeNotifiers = changeNotifiers?.ToArray() ?? Array.Empty<IWebhookEndpointChangeNotifier>();
    }

    public async Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.HookKey == hookKey
                                     && x.TenantId == scope.TenantId
                                     && x.EnvironmentTag == scope.EnvironmentTag
                                     && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return null;
        }

        var rules = await db.WebhookEndpointIpRules
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey
                        && x.TenantId == scope.TenantId
                        && x.EnvironmentTag == scope.EnvironmentTag
                        && !x.IsDeleted)
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
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && !x.IsDeleted)
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
                return Map(row, rules, includeSecret: false);
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

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey
                                      && x.TenantId == request.TenantId
                                      && x.EnvironmentTag == request.EnvironmentTag, cancellationToken)
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
                IsDeleted = false,
                CreatedAtUtc = now,
                UpdatedAtUtc = now
            };
            db.WebhookEndpoints.Add(entity);
            await ApplySecretAsync(db, entity, request.Secret, now, TimeSpan.Zero, "system:create", null, cancellationToken).ConfigureAwait(false);
            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Created, now));
        }
        else
        {
            if (entity.IsDeleted)
            {
                entity.IsDeleted = false;
            }

            entity.JobKey = request.JobKey;
            entity.RequestsPerMinute = request.RequestsPerMinute;
            entity.Enabled = request.Enabled;
            entity.RequireSignature = request.RequireSignature;
            entity.SignatureVersion = request.SignatureVersion;
            entity.MetadataJson = SerializeMetadata(request.Metadata);
            entity.UpdatedAtUtc = now;

            string currentSecret;
            if (!string.IsNullOrWhiteSpace(request.Secret))
            {
                currentSecret = request.Secret;
            }
            else if (!string.IsNullOrWhiteSpace(entity.Secret))
            {
                currentSecret = UnprotectSecret(entity.Secret);
            }
            else
            {
                throw new InvalidOperationException($"Webhook {request.HookKey} does not have an existing secret to snapshot.");
            }

            await ApplySecretAsync(db, entity, currentSecret, now, TimeSpan.Zero, "system:update", null, cancellationToken).ConfigureAwait(false);

            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Updated, now));
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(request.HookKey, new PartitionScope(request.TenantId, request.EnvironmentTag));
    }

    public async Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == hookKey
                                      && x.TenantId == scope.TenantId
                                      && x.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            var existsInOtherEnvironment = await db.WebhookEndpoints
                .AsNoTracking()
                .AnyAsync(x => x.HookKey == hookKey && x.TenantId == scope.TenantId, cancellationToken)
                .ConfigureAwait(false);

            if (existsInOtherEnvironment)
            {
                throw new InvalidOperationException($"Webhook {hookKey} does not belong to scope '{scope.EnvironmentTag}'.");
            }

            return;
        }

        var now = DateTime.UtcNow;
        if (hardDelete)
        {
            db.WebhookEndpoints.Remove(entity);
        }
        else
        {
            entity.IsDeleted = true;
            entity.UpdatedAtUtc = now;
        }

        db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Deleted, now));
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(entity.HookKey, new PartitionScope(entity.TenantId, entity.EnvironmentTag));
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
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey
                                      && x.TenantId == request.TenantId
                                      && x.EnvironmentTag == request.EnvironmentTag
                                      && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            throw new InvalidOperationException($"Webhook {request.HookKey} not found.");
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
        NotifyEndpointChanged(request.HookKey, new PartitionScope(request.TenantId, request.EnvironmentTag));

        return new WebhookSecretRotationResult(
            entity.HookKey,
            secret,
            entity.SecretHash,
            DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
            null);
    }

    public async Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        var now = DateTime.UtcNow;
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.WebhookSecretHistory
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey
                        && x.TenantId == scope.TenantId
                        && x.EnvironmentTag == scope.EnvironmentTag
                        && x.ActivatedAtUtc <= now
                        && (x.ExpiresAtUtc == null || x.ExpiresAtUtc > now))
            .OrderBy(x => x.ActivatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var materials = new List<WebhookSecretMaterial>(rows.Count);
        foreach (var row in rows)
        {
            var secret = TryUnprotectSecret(row.Secret);
            if (string.IsNullOrWhiteSpace(secret))
            {
                continue;
            }

            materials.Add(new WebhookSecretMaterial(
                secret,
                row.SecretHash,
                DateTime.SpecifyKind(row.ActivatedAtUtc, DateTimeKind.Utc),
                row.ExpiresAtUtc.HasValue ? DateTime.SpecifyKind(row.ExpiresAtUtc.Value, DateTimeKind.Utc) : null));
        }

        return materials;
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
        NotifyEndpointChanged(request.HookKey, new PartitionScope(endpoint.TenantId, endpoint.EnvironmentTag));

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
        NotifyEndpointChanged(entity.HookKey, new PartitionScope(entity.TenantId, entity.EnvironmentTag));
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
        var protectedSecret = ProtectSecret(secret);
        entity.Secret = protectedSecret;
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
            Secret = protectedSecret,
            SecretHash = secretHash,
            ActivatedAtUtc = DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
            ExpiresAtUtc = null,
            RotatedBy = rotatedBy,
            Notes = notes
        });
    }

    private void NotifyEndpointChanged(string hookKey, PartitionScope scope)
    {
        if (_changeNotifiers.Count == 0 || string.IsNullOrWhiteSpace(hookKey))
        {
            return;
        }

        foreach (var notifier in _changeNotifiers)
        {
            notifier.NotifyChanged(hookKey, scope);
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

    private WebhookEndpointDefinition Map(
        WebhookEndpointEntity entity,
        IReadOnlyCollection<WebhookIpRuleDefinition>? ipRules = null,
        bool includeSecret = true)
    {
        var metadata = DeserializeMetadata(entity.MetadataJson);
        var secret = includeSecret ? (TryUnprotectSecret(entity.Secret) ?? string.Empty) : string.Empty;
        return new WebhookEndpointDefinition(
            entity.HookKey,
            entity.JobKey,
            secret,
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

    private string ProtectSecret(string secret)
    {
        return _secretProtector.Protect(secret);
    }

    private string? TryUnprotectSecret(string protectedSecret)
    {
        if (string.IsNullOrWhiteSpace(protectedSecret))
        {
            return null;
        }

        try
        {
            return _secretProtector.Unprotect(protectedSecret);
        }
        catch (CryptographicException)
        {
            return null;
        }
    }

    private string UnprotectSecret(string protectedSecret)
    {
        if (string.IsNullOrWhiteSpace(protectedSecret))
        {
            throw new InvalidOperationException("Webhook secret material is missing.");
        }

        try
        {
            return _secretProtector.Unprotect(protectedSecret);
        }
        catch (CryptographicException ex)
        {
            throw new InvalidOperationException(
                "Webhook secret material could not be decrypted. Ensure DataProtection keys are shared and the key ring matches across hosts.",
                ex);
        }
    }
}

#if false
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
using Microsoft.AspNetCore.DataProtection;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private const string SecretProtectionPurpose = "Croniq.Webhooks.Secret.v1";
    private const int SecretByteLength = 32;
    private static readonly TimeSpan DefaultGracePeriod = TimeSpan.FromHours(24);
    private static readonly TimeSpan MaxActivationDelay = TimeSpan.FromDays(7);
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly IReadOnlyList<IWebhookEndpointChangeNotifier> _changeNotifiers;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private readonly IDataProtector _secretProtector;

    public SqlServerWebhookPersistenceProvider(
        IDbContextFactory<SqlServerDbContext> dbFactory,
        IDataProtectionProvider dataProtectionProvider,
        IEnumerable<IWebhookEndpointChangeNotifier>? changeNotifiers = null)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _secretProtector = (dataProtectionProvider ?? throw new ArgumentNullException(nameof(dataProtectionProvider)))
            .CreateProtector(SecretProtectionPurpose);
        _changeNotifiers = changeNotifiers?.ToArray() ?? Array.Empty<IWebhookEndpointChangeNotifier>();
    }

    public async Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.HookKey == hookKey
                                     && x.TenantId == scope.TenantId
                                     && x.EnvironmentTag == scope.EnvironmentTag
                                     && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return null;
        }

        var rules = await db.WebhookEndpointIpRules
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey
                        && x.TenantId == scope.TenantId
                        && x.EnvironmentTag == scope.EnvironmentTag
                        && !x.IsDeleted)
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
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && !x.IsDeleted)
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
                return Map(row, rules, includeSecret: false);
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

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey
                                      && x.TenantId == request.TenantId
                                      && x.EnvironmentTag == request.EnvironmentTag, cancellationToken)
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
                IsDeleted = false,
                CreatedAtUtc = now,
                UpdatedAtUtc = now
            };
            db.WebhookEndpoints.Add(entity);
            await ApplySecretAsync(db, entity, request.Secret, now, TimeSpan.Zero, "system:create", null, cancellationToken).ConfigureAwait(false);
            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Created, now));
        }
        else
        {
            if (entity.IsDeleted)
            {
                entity.IsDeleted = false;
            }

            entity.JobKey = request.JobKey;
            entity.RequestsPerMinute = request.RequestsPerMinute;
            entity.Enabled = request.Enabled;
            entity.RequireSignature = request.RequireSignature;
            entity.SignatureVersion = request.SignatureVersion;
            entity.MetadataJson = SerializeMetadata(request.Metadata);
            entity.UpdatedAtUtc = now;

            string currentSecret;
            if (!string.IsNullOrWhiteSpace(request.Secret))
            {
                currentSecret = request.Secret;
            }
            else if (!string.IsNullOrWhiteSpace(entity.Secret))
            {
                currentSecret = UnprotectSecret(entity.Secret);
            }
            else
            {
                throw new InvalidOperationException($"Webhook {request.HookKey} does not have an existing secret to snapshot.");
            }

            await ApplySecretAsync(db, entity, currentSecret, now, TimeSpan.Zero, "system:update", null, cancellationToken).ConfigureAwait(false);

            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Updated, now));
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(request.HookKey, new PartitionScope(request.TenantId, request.EnvironmentTag));
    }

    public async Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == hookKey
                                      && x.TenantId == scope.TenantId
                                      && x.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            var existsInOtherEnvironment = await db.WebhookEndpoints
                .AsNoTracking()
                .AnyAsync(x => x.HookKey == hookKey && x.TenantId == scope.TenantId, cancellationToken)
                .ConfigureAwait(false);

            if (existsInOtherEnvironment)
            {
                throw new InvalidOperationException($"Webhook {hookKey} does not belong to scope '{scope.EnvironmentTag}'.");
            }

            return;
        }

        var now = DateTime.UtcNow;
        if (hardDelete)
        {
            db.WebhookEndpoints.Remove(entity);
        }
        else
        {
            entity.IsDeleted = true;
            entity.UpdatedAtUtc = now;
        }

        db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Deleted, now));
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(entity.HookKey, new PartitionScope(entity.TenantId, entity.EnvironmentTag));
    }

    public async Task<WebhookEndpointDefinition> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (!string.IsNullOrWhiteSpace(request.HookKey) && request.HookKey.Length > 128)
        {
            throw new InvalidOperationException("hook key exceeds maximum length");
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == request.HookKey
                                      && x.TenantId == request.TenantId
                                      && x.EnvironmentTag == request.EnvironmentTag
                                      && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            throw new InvalidOperationException($"Webhook {request.HookKey} not found.");
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
        NotifyEndpointChanged(request.HookKey, new PartitionScope(request.TenantId, request.EnvironmentTag));

        return new WebhookSecretRotationResult(
            entity.HookKey,
            secret,
            entity.SecretHash,
            DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
            null);
    }

    public async Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        var now = DateTime.UtcNow;
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.WebhookSecretHistory
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey
                        && x.TenantId == scope.TenantId
                        && x.EnvironmentTag == scope.EnvironmentTag
                        && x.ActivatedAtUtc <= now
                        && (x.ExpiresAtUtc == null || x.ExpiresAtUtc > now))
            .OrderBy(x => x.ActivatedAtUtc)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var materials = new List<WebhookSecretMaterial>(rows.Count);
        foreach (var row in rows)
        {
            var secret = TryUnprotectSecret(row.Secret);
            if (string.IsNullOrWhiteSpace(secret))
            {
                continue;
            }

            materials.Add(new WebhookSecretMaterial(
                secret,
                row.SecretHash,
                DateTime.SpecifyKind(row.ActivatedAtUtc, DateTimeKind.Utc),
                row.ExpiresAtUtc.HasValue ? DateTime.SpecifyKind(row.ExpiresAtUtc.Value, DateTimeKind.Utc) : null));
        }

        return materials;
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

        var now = DateTime.UtcNow;
        var row = new WebhookEndpointIpRuleEntity
        {
            HookKey = request.HookKey,
            TenantId = request.TenantId,
            EnvironmentTag = request.EnvironmentTag,
            Cidr = request.Cidr,
            Description = request.Description,
            CreatedBy = request.CreatedBy,
            IsDeleted = false,
            CreatedAtUtc = now,
            UpdatedAtUtc = now
        };

        db.WebhookEndpointIpRules.Add(row);
        db.WebhookEndpointEvents.Add(CreateEvent(endpoint.HookKey, endpoint.TenantId, endpoint.EnvironmentTag, WebhookEndpointEventTypes.Updated, now, request.CreatedBy, null));
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(endpoint.HookKey, new PartitionScope(endpoint.TenantId, endpoint.EnvironmentTag));
        return MapIpRule(row);
    }

    public async Task<WebhookIpRuleDefinition> UpdateIpRuleAsync(WebhookIpRuleUpdate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.Cidr)) throw new ArgumentNullException(nameof(request.Cidr));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpointIpRules
            .FirstOrDefaultAsync(x => x.Id == request.Id, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            throw new InvalidOperationException($"Webhook IP rule {request.Id} not found.");
        }

        EnsureScope(entity.TenantId, entity.EnvironmentTag, new PartitionScope(request.TenantId, request.EnvironmentTag));

        entity.Cidr = request.Cidr;
        entity.Description = request.Description;
        entity.UpdatedAtUtc = DateTime.UtcNow;

        var endpoint = await db.WebhookEndpoints
            .FirstOrDefaultAsync(x => x.HookKey == entity.HookKey && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (endpoint is not null)
        {
            db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Updated, entity.UpdatedAtUtc, request.UpdatedBy, null));
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        NotifyEndpointChanged(entity.HookKey, new PartitionScope(entity.TenantId, entity.EnvironmentTag));
        return MapIpRule(entity);
    }

    public async Task DeleteIpRuleAsync(Guid id, string deletedBy, string? correlationId, PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpointIpRules
            .FirstOrDefaultAsync(x => x.Id == id, cancellationToken)
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
        NotifyEndpointChanged(entity.HookKey, new PartitionScope(entity.TenantId, entity.EnvironmentTag));
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
        var protectedSecret = ProtectSecret(secret);
        entity.Secret = protectedSecret;
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
            Secret = protectedSecret,
            SecretHash = secretHash,
            ActivatedAtUtc = DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
            ExpiresAtUtc = null,
            RotatedBy = rotatedBy,
            Notes = notes
        });
    }

    private void NotifyEndpointChanged(string hookKey, PartitionScope scope)
    {
        if (_changeNotifiers.Count == 0 || string.IsNullOrWhiteSpace(hookKey))
        {
            return;
        }

        foreach (var notifier in _changeNotifiers)
        {
            notifier.NotifyChanged(hookKey, scope);
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

    private WebhookEndpointDefinition Map(
        WebhookEndpointEntity entity,
        IReadOnlyCollection<WebhookIpRuleDefinition>? ipRules = null,
        bool includeSecret = true)
    {
        var metadata = DeserializeMetadata(entity.MetadataJson);
        var secret = includeSecret ? (TryUnprotectSecret(entity.Secret) ?? string.Empty) : string.Empty;
        return new WebhookEndpointDefinition(
            entity.HookKey,
            entity.JobKey,
            secret,
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

    private string ProtectSecret(string secret)
    {
        return _secretProtector.Protect(secret);
    }

    private string? TryUnprotectSecret(string protectedSecret)
    {
        if (string.IsNullOrWhiteSpace(protectedSecret))
        {
            return null;
        }

        try
        {
            return _secretProtector.Unprotect(protectedSecret);
        }
        catch (CryptographicException)
        {
            return null;
        }
    }

    private string UnprotectSecret(string protectedSecret)
    {
        if (string.IsNullOrWhiteSpace(protectedSecret))
        {
            throw new InvalidOperationException("Webhook secret material is missing.");
        }

        try
        {
            return _secretProtector.Unprotect(protectedSecret);
        }
        catch (CryptographicException ex)
        {
            throw new InvalidOperationException(
                "Webhook secret material could not be decrypted. Ensure DataProtection keys are shared and the key ring matches across hosts.",
                ex);
        }
    }
}using System;
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
using Microsoft.AspNetCore.DataProtection;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private const string SecretProtectionPurpose = "Croniq.Webhooks.Secret.v1";
    private const int SecretByteLength = 32;
    private static readonly TimeSpan DefaultGracePeriod = TimeSpan.FromHours(24);
    private static readonly TimeSpan MaxActivationDelay = TimeSpan.FromDays(7);
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly IReadOnlyList<IWebhookEndpointChangeNotifier> _changeNotifiers;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private readonly IDataProtector _secretProtector;

    public SqlServerWebhookPersistenceProvider(
        IDbContextFactory<SqlServerDbContext> dbFactory,
        IDataProtectionProvider dataProtectionProvider,
        IEnumerable<IWebhookEndpointChangeNotifier>? changeNotifiers = null)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _secretProtector = (dataProtectionProvider ?? throw new ArgumentNullException(nameof(dataProtectionProvider)))
            .CreateProtector(SecretProtectionPurpose);
        _changeNotifiers = changeNotifiers?.ToArray() ?? Array.Empty<IWebhookEndpointChangeNotifier>();
    }

    public async Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.WebhookEndpoints
            .AsNoTracking()
            .FirstOrDefaultAsync(x => x.HookKey == hookKey
                                     && x.TenantId == scope.TenantId
                                     && x.EnvironmentTag == scope.EnvironmentTag
                                     && !x.IsDeleted, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return null;
        }

        var rules = await db.WebhookEndpointIpRules
            .AsNoTracking()
            .Where(x => x.HookKey == hookKey
                        && x.TenantId == scope.TenantId
                        && x.EnvironmentTag == scope.EnvironmentTag
                        && !x.IsDeleted)
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
            .Where(x => x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag && !x.IsDeleted)
            .OrderBy(x => x.HookKey)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        if (rows.Count == 0)
        {
            return Array.Empty<WebhookEndpointDefinition>();
        }

        var secret = includeSecret ? (TryUnprotectSecret(entity.Secret) ?? string.Empty) : string.Empty;
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
                return Map(row, rules, includeSecret: false);
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
        var materials = new List<WebhookSecretMaterial>(rows.Count);
        foreach (var row in rows)
        {
            var secret = TryUnprotectSecret(row.Secret);
            if (string.IsNullOrWhiteSpace(secret))
            {
                continue;
            }

            materials.Add(new WebhookSecretMaterial(
                secret,
                row.SecretHash,
                DateTime.SpecifyKind(row.ActivatedAtUtc, DateTimeKind.Utc),
                row.ExpiresAtUtc.HasValue ? DateTime.SpecifyKind(row.ExpiresAtUtc.Value, DateTimeKind.Utc) : null));
        }

        return materials;
                                      && x.EnvironmentTag == request.EnvironmentTag, cancellationToken)
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
                IsDeleted = false,
                private string? TryUnprotectSecret(string protectedSecret)
    {
        if (string.IsNullOrWhiteSpace(protectedSecret))
        {
            return null;
        }

        try
        {
            return _secretProtector.Unprotect(protectedSecret);
        }
        catch (CryptographicException)
        {
            return null;
        }
    }
    CreatedAtUtc = now,
                UpdatedAtUtc = now
};
db.WebhookEndpoints.Add(entity);
            await ApplySecretAsync(db, entity, request.Secret, now, TimeSpan.Zero, "system:create", null, cancellationToken).ConfigureAwait(false);
db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Created, now));
        }
        else
{
    if (entity.IsDeleted)
    {
        entity.IsDeleted = false;
    }

    entity.JobKey = request.JobKey;
    entity.RequestsPerMinute = request.RequestsPerMinute;
    entity.Enabled = request.Enabled;
    entity.RequireSignature = request.RequireSignature;
    entity.SignatureVersion = request.SignatureVersion;
    entity.MetadataJson = SerializeMetadata(request.Metadata);
    entity.UpdatedAtUtc = now;

    string currentSecret;
    if (!string.IsNullOrWhiteSpace(request.Secret))
    {
        currentSecret = request.Secret;
    }
    else if (!string.IsNullOrWhiteSpace(entity.Secret))
    {
        currentSecret = UnprotectSecret(entity.Secret);
    }
    else
    {
        throw new InvalidOperationException($"Webhook {request.HookKey} does not have an existing secret to snapshot.");
    }

    await ApplySecretAsync(db, entity, currentSecret, now, TimeSpan.Zero, "system:update", null, cancellationToken).ConfigureAwait(false);

    db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Updated, now));
}

await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
NotifyEndpointChanged(request.HookKey, new PartitionScope(request.TenantId, request.EnvironmentTag));
    }

    public async Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
{
    if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

    await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
    var entity = await db.WebhookEndpoints
        .FirstOrDefaultAsync(x => x.HookKey == hookKey
                                  && x.TenantId == scope.TenantId
                                  && x.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
        .ConfigureAwait(false);

    if (entity is null)
    {
        var existsInOtherEnvironment = await db.WebhookEndpoints
            .AsNoTracking()
            .AnyAsync(x => x.HookKey == hookKey && x.TenantId == scope.TenantId, cancellationToken)
            .ConfigureAwait(false);

        if (existsInOtherEnvironment)
        {
            throw new InvalidOperationException($"Webhook {hookKey} does not belong to scope '{scope.EnvironmentTag}'.");
        }

        return;
    }

    var now = DateTime.UtcNow;

    if (hardDelete)
    {
        // purge dependents first (DeleteBehavior is expected to be Restrict)
        var ipRules = await db.WebhookEndpointIpRules
            .Where(x => x.HookKey == hookKey && x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);
        db.WebhookEndpointIpRules.RemoveRange(ipRules);

        var secrets = await db.WebhookSecretHistory
            .Where(x => x.HookKey == hookKey && x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);
        db.WebhookSecretHistory.RemoveRange(secrets);

        var deadLetters = await db.WebhookDeadLetters
            .Where(x => x.HookKey == hookKey && x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);
        db.WebhookDeadLetters.RemoveRange(deadLetters);

        var events = await db.WebhookEndpointEvents
            .Where(x => x.HookKey == hookKey && x.TenantId == scope.TenantId && x.EnvironmentTag == scope.EnvironmentTag)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);
        db.WebhookEndpointEvents.RemoveRange(events);

        db.WebhookEndpoints.Remove(entity);
    }
    else
    {
        entity.IsDeleted = true;
        entity.Enabled = false;
        entity.UpdatedAtUtc = now;
        db.WebhookEndpointEvents.Add(CreateEvent(entity.HookKey, entity.TenantId, entity.EnvironmentTag, WebhookEndpointEventTypes.Deleted, now));
    }

    await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    NotifyEndpointChanged(hookKey, scope);
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
        .FirstOrDefaultAsync(x => x.HookKey == request.HookKey
                                  && x.TenantId == request.TenantId
                                  && x.EnvironmentTag == request.EnvironmentTag
                                  && !x.IsDeleted, cancellationToken)
        .ConfigureAwait(false);

    if (entity is null)
    {
        throw new InvalidOperationException($"Webhook {request.HookKey} not found.");
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
    NotifyEndpointChanged(request.HookKey, new PartitionScope(request.TenantId, request.EnvironmentTag));

    return new WebhookSecretRotationResult(
        entity.HookKey,
        secret,
        entity.SecretHash,
        DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
        null);
}

public async Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
{
    if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

    var now = DateTime.UtcNow;
    await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
    var rows = await db.WebhookSecretHistory
        .AsNoTracking()
        .Where(x => x.HookKey == hookKey
                    && x.TenantId == scope.TenantId
                    && x.EnvironmentTag == scope.EnvironmentTag
                    && x.ActivatedAtUtc <= now
                    && (x.ExpiresAtUtc == null || x.ExpiresAtUtc > now))
        .OrderBy(x => x.ActivatedAtUtc)
        .ToListAsync(cancellationToken)
        .ConfigureAwait(false);

    return rows.Select(x => new WebhookSecretMaterial(
        UnprotectSecret(x.Secret),
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
    NotifyEndpointChanged(request.HookKey, new PartitionScope(endpoint.TenantId, endpoint.EnvironmentTag));

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
    NotifyEndpointChanged(entity.HookKey, new PartitionScope(entity.TenantId, entity.EnvironmentTag));
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
    var protectedSecret = ProtectSecret(secret);
    entity.Secret = protectedSecret;
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
        Secret = protectedSecret,
        SecretHash = secretHash,
        ActivatedAtUtc = DateTime.SpecifyKind(activatedAtUtc, DateTimeKind.Utc),
        ExpiresAtUtc = null,
        RotatedBy = rotatedBy,
        Notes = notes
    });
}

private void NotifyEndpointChanged(string hookKey, PartitionScope scope)
{
    if (_changeNotifiers.Count == 0 || string.IsNullOrWhiteSpace(hookKey))
    {
        return;
    }

    foreach (var notifier in _changeNotifiers)
    {
        notifier.NotifyChanged(hookKey, scope);
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

private WebhookEndpointDefinition Map(
    WebhookEndpointEntity entity,
    IReadOnlyCollection<WebhookIpRuleDefinition>? ipRules = null,
    bool includeSecret = true)
{
    var metadata = DeserializeMetadata(entity.MetadataJson);
    var secret = includeSecret ? UnprotectSecret(entity.Secret) : string.Empty;
    return new WebhookEndpointDefinition(
        entity.HookKey,
        entity.JobKey,
        secret,
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

private string ProtectSecret(string secret)
{
    return _secretProtector.Protect(secret);
}

private string UnprotectSecret(string protectedSecret)
{
    if (string.IsNullOrWhiteSpace(protectedSecret))
    {
        throw new InvalidOperationException("Webhook secret material is missing.");
    }

    try
    {
        return _secretProtector.Unprotect(protectedSecret);
    }
    catch (CryptographicException ex)
    {
        throw new InvalidOperationException(
            "Webhook secret material could not be decrypted. Ensure DataProtection keys are shared and the key ring matches across hosts.",
            ex);
    }
}
 }

#endif
