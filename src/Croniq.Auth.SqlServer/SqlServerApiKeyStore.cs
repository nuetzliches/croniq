using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Croniq.Auth.Abstractions;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;

namespace Croniq.Auth.SqlServer;

/// <summary>
/// EF Core backed implementation for issuing and validating Croniq API keys.
/// </summary>
public sealed class SqlServerApiKeyStore : IApiKeyStore
{
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private readonly IDbContextFactory<SqlServerDbContext> _dbContextFactory;
    private readonly ILogger<SqlServerApiKeyStore> _logger;
    private readonly TimeProvider _timeProvider;

    public SqlServerApiKeyStore(
        IDbContextFactory<SqlServerDbContext> dbContextFactory,
        ILogger<SqlServerApiKeyStore> logger,
        TimeProvider? timeProvider = null)
    {
        _dbContextFactory = dbContextFactory ?? throw new ArgumentNullException(nameof(dbContextFactory));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task<ApiKeyIssueResult> IssueAsync(ApiKeyIssueRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.TenantId)) throw new ArgumentException("TenantId is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.ClientId)) throw new ArgumentException("ClientId is required", nameof(request));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var now = _timeProvider.GetUtcNow().UtcDateTime;

        var client = await UpsertClientEntityAsync(
            db,
            new ApiClientUpsertRequest(
                request.TenantId,
                request.ClientId,
                Name: null,
                request.EnvironmentTag,
                request.Scopes,
                IsActive: true),
            now,
            saveChanges: false,
            cancellationToken).ConfigureAwait(false);

        var keyId = $"ak_{Guid.NewGuid():N}";
        var secret = GenerateSecret();
        var salt = GenerateSalt();
        var hash = HashSecret(secret, salt);
        var expires = request.Ttl.HasValue ? now.Add(request.Ttl.Value) : (DateTime?)null;
        var environment = request.EnvironmentTag ?? client.EnvironmentTag;
        var keyScopes = SerializeScopes(request.Scopes) ?? client.ScopesJson;

        var entity = new ApiKeyEntity
        {
            ApiClientId = client.Id,
            KeyId = keyId,
            SecretHash = hash,
            SecretSalt = salt,
            EnvironmentTag = environment,
            ScopesJson = keyScopes,
            ExpiresAtUtc = expires,
            IsActive = true,
            CreatedAtUtc = now,
            UpdatedAtUtc = now
        };
        db.ApiKeys.Add(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);

        return new ApiKeyIssueResult(
            request.ClientId,
            request.TenantId,
            keyId,
            $"{keyId}.{secret}",
            environment,
            expires.HasValue ? new DateTimeOffset(DateTime.SpecifyKind(expires.Value, DateTimeKind.Utc)) : null);
    }

    public async Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(keyId)) throw new ArgumentNullException(nameof(keyId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.ApiKeys
            .Include(k => k.Client)
            .FirstOrDefaultAsync(k => k.KeyId == keyId && k.Client.TenantId == tenantId, cancellationToken: cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return false;
        }

        if (!entity.IsActive)
        {
            return true;
        }

        entity.IsActive = false;
        entity.UpdatedAtUtc = _timeProvider.GetUtcNow().UtcDateTime;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return true;
    }

    public async Task<ApiKeyIssueResult?> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(keyId)) throw new ArgumentNullException(nameof(keyId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.ApiKeys
            .Include(k => k.Client)
            .FirstOrDefaultAsync(k => k.KeyId == keyId && k.Client.TenantId == tenantId, cancellationToken: cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return null;
        }

        var now = _timeProvider.GetUtcNow().UtcDateTime;
        entity.IsActive = false;
        entity.UpdatedAtUtc = now;
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);

        var scopes = ParseScopes(entity.ScopesJson, entity.Client?.ScopesJson);
        TimeSpan? ttl = null;
        if (entity.ExpiresAtUtc.HasValue)
        {
            var remaining = entity.ExpiresAtUtc.Value - now;
            if (remaining > TimeSpan.Zero)
            {
                ttl = remaining;
            }
        }

        var issueRequest = new ApiKeyIssueRequest(
            tenantId,
            entity.Client!.ClientId,
            entity.EnvironmentTag ?? entity.Client.EnvironmentTag,
            scopes,
            ttl);
        var issued = await IssueAsync(issueRequest, cancellationToken).ConfigureAwait(false);
        return issued;
    }

    public async Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(presentedKey))
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "missing");
        }

        var (keyId, secret) = SplitKey(presentedKey);
        if (string.IsNullOrWhiteSpace(keyId) || string.IsNullOrWhiteSpace(secret))
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "invalid-format");
        }

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.ApiKeys
            .Include(k => k.Client)
            .AsNoTracking()
            .FirstOrDefaultAsync(k => k.KeyId == keyId, cancellationToken: cancellationToken)
            .ConfigureAwait(false);

        if (entity is null || entity.Client is null)
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "not-found");
        }

        if (!entity.IsActive)
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "revoked");
        }

        if (entity.ExpiresAtUtc.HasValue && entity.ExpiresAtUtc.Value < _timeProvider.GetUtcNow().UtcDateTime)
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "expired");
        }

        if (!VerifySecret(secret, entity.SecretSalt, entity.SecretHash))
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "invalid-secret");
        }

        var scopes = ParseScopes(entity.ScopesJson, entity.Client.ScopesJson);
        var environment = entity.EnvironmentTag ?? entity.Client.EnvironmentTag;
        return new ApiKeyValidationResult(true, entity.Client.TenantId, environment, entity.KeyId, scopes, null);
    }

    public async Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(clientId)) throw new ArgumentNullException(nameof(clientId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.ApiClients
            .AsNoTracking()
            .FirstOrDefaultAsync(c => c.TenantId == tenantId && c.ClientId == clientId, cancellationToken: cancellationToken)
            .ConfigureAwait(false);

        if (entity is null || entity.IsDeleted)
        {
            return null;
        }

        return ToDescriptor(entity);
    }

    public async Task<ApiClientDescriptor> UpsertClientAsync(ApiClientUpsertRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var now = _timeProvider.GetUtcNow().UtcDateTime;
        var entity = await UpsertClientEntityAsync(db, request, now, saveChanges: true, cancellationToken).ConfigureAwait(false);
        return ToDescriptor(entity);
    }

    public async Task<IReadOnlyCollection<ApiClientDescriptor>> ListClientsAsync(string tenantId, string? environmentTag, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var query = db.ApiClients
            .AsNoTracking()
            .Where(c => c.TenantId == tenantId && !c.IsDeleted);

        if (!string.IsNullOrWhiteSpace(environmentTag))
        {
            query = query.Where(c => c.EnvironmentTag == environmentTag);
        }

        var entities = await query
            .OrderBy(c => c.ClientId)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        return entities.Select(ToDescriptor).ToArray();
    }

    public async Task<bool> DeleteClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(clientId)) throw new ArgumentNullException(nameof(clientId));

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.ApiClients
            .Include(c => c.ApiKeys)
            .FirstOrDefaultAsync(c => c.TenantId == tenantId && c.ClientId == clientId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return false;
        }

        var now = _timeProvider.GetUtcNow().UtcDateTime;
        entity.IsDeleted = true;
        entity.IsActive = false;
        entity.UpdatedAtUtc = now;

        foreach (var key in entity.ApiKeys)
        {
            if (!key.IsActive)
            {
                continue;
            }

            key.IsActive = false;
            key.UpdatedAtUtc = now;
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return true;
    }

    private async Task<ApiClientEntity> UpsertClientEntityAsync(
        SqlServerDbContext db,
        ApiClientUpsertRequest request,
        DateTime now,
        bool saveChanges,
        CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var serializedScopes = SerializeScopes(request.Scopes);
        var entity = await db.ApiClients
            .FirstOrDefaultAsync(c => c.TenantId == request.TenantId && c.ClientId == request.ClientId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            entity = new ApiClientEntity
            {
                TenantId = request.TenantId,
                ClientId = request.ClientId,
                Name = request.Name,
                EnvironmentTag = request.EnvironmentTag,
                ScopesJson = serializedScopes,
                CreatedAtUtc = now,
                UpdatedAtUtc = now,
                IsActive = request.IsActive,
                IsDeleted = false
            };
            db.ApiClients.Add(entity);
        }
        else
        {
            if (!string.IsNullOrWhiteSpace(request.Name))
            {
                entity.Name = request.Name;
            }

            if (!string.IsNullOrWhiteSpace(request.EnvironmentTag))
            {
                entity.EnvironmentTag = request.EnvironmentTag;
            }

            if (!string.IsNullOrWhiteSpace(serializedScopes))
            {
                entity.ScopesJson = serializedScopes;
            }

            entity.IsActive = request.IsActive;
            entity.IsDeleted = false;
            entity.UpdatedAtUtc = now;
        }

        if (saveChanges)
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }

        return entity;
    }

    private static ApiClientDescriptor ToDescriptor(ApiClientEntity entity)
    {
        var scopes = ParseScopes(entity.ScopesJson, defaultValueJson: null);
        return new ApiClientDescriptor(
            entity.ClientId,
            entity.TenantId,
            entity.Name,
            entity.EnvironmentTag,
            scopes,
            entity.IsActive && !entity.IsDeleted,
            null);
    }

    private static string GenerateSecret()
    {
        Span<byte> buffer = stackalloc byte[32];
        RandomNumberGenerator.Fill(buffer);
        return Convert.ToBase64String(buffer);
    }

    private static string GenerateSalt()
    {
        Span<byte> buffer = stackalloc byte[16];
        RandomNumberGenerator.Fill(buffer);
        return Convert.ToBase64String(buffer);
    }

    private static string HashSecret(string secret, string salt)
    {
        var bytes = Encoding.UTF8.GetBytes(secret + salt);
        var hash = SHA256.HashData(bytes);
        return Convert.ToBase64String(hash);
    }

    private static bool VerifySecret(string candidate, string salt, string expectedHash)
    {
        var computedHash = SHA256.HashData(Encoding.UTF8.GetBytes(candidate + salt));
        var expectedBytes = Convert.FromBase64String(expectedHash);
        return computedHash.Length == expectedBytes.Length && CryptographicOperations.FixedTimeEquals(computedHash, expectedBytes);
    }

    private static (string? KeyId, string? Secret) SplitKey(string presented)
    {
        var idx = presented.IndexOf('.');
        if (idx <= 0 || idx == presented.Length - 1)
        {
            return (null, null);
        }

        return (presented[..idx], presented[(idx + 1)..]);
    }

    private static IReadOnlyCollection<string> ParseScopes(string? scopesJson, string? defaultValueJson)
    {
        if (!string.IsNullOrWhiteSpace(scopesJson))
        {
            return JsonSerializer.Deserialize<string[]>(scopesJson, JsonOptions) ?? Array.Empty<string>();
        }

        if (!string.IsNullOrWhiteSpace(defaultValueJson))
        {
            return JsonSerializer.Deserialize<string[]>(defaultValueJson, JsonOptions) ?? Array.Empty<string>();
        }

        return Array.Empty<string>();
    }

    private static string? SerializeScopes(IReadOnlyCollection<string>? scopes)
    {
        if (scopes is null || scopes.Count == 0)
        {
            return null;
        }

        return JsonSerializer.Serialize(scopes, JsonOptions);
    }
}
