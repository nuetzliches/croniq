using System.Collections.Concurrent;
using System.Linq;
using System.Text;
using System.Threading;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Http;

namespace Croniq.Webhooks.InMemory;

/// <summary>
/// Simple in-memory implementation of the webhook persistence provider for samples and tests.
/// </summary>
public sealed class InMemoryWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private readonly ConcurrentDictionary<string, WebhookEndpointDefinition> _store = new(StringComparer.OrdinalIgnoreCase);
    private long _ipRuleIdentity;
    private readonly object _ipRuleLock = new();

    public WebhookEndpointDefinition? Find(string hookKey)
    {
        _ = hookKey ?? throw new ArgumentNullException(nameof(hookKey));
        return _store.TryGetValue(hookKey, out var definition) ? definition : null;
    }

    public WebhookEndpointDefinition Seed(
        string hookKey,
        string jobKey,
        PartitionScope scope,
        string secret,
        bool requireSignature = true,
        bool enabled = true,
        int requestsPerMinute = 120,
        IReadOnlyDictionary<string, string>? metadata = null,
        int signatureVersion = 1)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        var now = DateTimeOffset.UtcNow;
        var materializedMetadata = metadata is null ? null : new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);

        var definition = new WebhookEndpointDefinition(
            hookKey,
            jobKey,
            secret,
            enabled,
            requireSignature,
            requestsPerMinute,
            scope.TenantId,
            scope.EnvironmentTag,
            materializedMetadata,
            Array.Empty<WebhookIpRuleDefinition>(),
            signatureVersion,
            now,
            now);

        _store[hookKey] = definition;
        return definition;
    }

    public Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, CancellationToken cancellationToken)
    {
        return Task.FromResult(Find(hookKey));
    }

    public Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        IReadOnlyCollection<WebhookEndpointDefinition> result = _store.Values
            .Where(def => MatchesScope(def, scope))
            .OrderBy(def => def.HookKey, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        return Task.FromResult(result);
    }

    public Task UpsertAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        var now = DateTimeOffset.UtcNow;
        var metadata = request.Metadata is null ? null : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);

        _store.AddOrUpdate(
            request.HookKey,
            _ => new WebhookEndpointDefinition(
                request.HookKey,
                request.JobKey,
                request.Secret ?? GenerateSecret(),
                request.Enabled,
                request.RequireSignature,
                request.RequestsPerMinute,
                request.TenantId,
                request.EnvironmentTag,
                metadata,
                Array.Empty<WebhookIpRuleDefinition>(),
                request.SignatureVersion,
                now,
                now),
            (_, current) => current with
            {
                JobKey = request.JobKey,
                Enabled = request.Enabled,
                RequireSignature = request.RequireSignature,
                RequestsPerMinute = request.RequestsPerMinute,
                TenantId = request.TenantId,
                EnvironmentTag = request.EnvironmentTag,
                Secret = request.Secret ?? current.Secret,
                Metadata = metadata,
                SignatureVersion = request.SignatureVersion,
                UpdatedAtUtc = now
            });

        return Task.CompletedTask;
    }

    public Task DeleteAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        _store.TryRemove(hookKey, out _);
        return Task.CompletedTask;
    }

    public Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        if (!_store.TryGetValue(request.HookKey, out var current))
        {
            throw new InvalidOperationException($"webhook {request.HookKey} not found");
        }

        var nowOffset = DateTimeOffset.UtcNow;
        var secret = GenerateSecret();
        var updated = current with { Secret = secret, UpdatedAtUtc = nowOffset };
        _store[request.HookKey] = updated;

        var activated = DateTime.UtcNow;
        var result = new WebhookSecretRotationResult(
            request.HookKey,
            secret,
            ComputeHash(secret),
            activated,
            request.GracePeriodSeconds.HasValue ? activated.AddSeconds(request.GracePeriodSeconds.Value) : null);
        return Task.FromResult(result);
    }

    public Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        if (!_store.TryGetValue(hookKey, out var definition))
        {
            return Task.FromResult<IReadOnlyCollection<WebhookSecretMaterial>>(Array.Empty<WebhookSecretMaterial>());
        }

        IReadOnlyCollection<WebhookSecretMaterial> secrets = new[]
        {
            new WebhookSecretMaterial(
                definition.Secret,
                ComputeHash(definition.Secret),
                DateTime.UtcNow,
                null)
        };

        return Task.FromResult(secrets);
    }

    public Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        if (!_store.TryGetValue(hookKey, out var definition))
        {
            return Task.FromResult<IReadOnlyCollection<WebhookIpRuleDefinition>>(Array.Empty<WebhookIpRuleDefinition>());
        }

        if (!MatchesScope(definition, scope))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }

        return Task.FromResult(definition.IpRules);
    }

    public Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (!_store.TryGetValue(request.HookKey, out var definition))
        {
            throw new InvalidOperationException($"webhook {request.HookKey} not found");
        }

        if (!string.Equals(definition.TenantId, request.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(definition.EnvironmentTag, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }

        lock (_ipRuleLock)
        {
            if (definition.IpRules.Any(rule => string.Equals(rule.Cidr, request.Cidr, StringComparison.OrdinalIgnoreCase)))
            {
                throw new InvalidOperationException($"CIDR {request.Cidr} already exists for webhook {request.HookKey}");
            }

            var id = Interlocked.Increment(ref _ipRuleIdentity);
            var now = DateTimeOffset.UtcNow;
            var rule = new WebhookIpRuleDefinition(
                id,
                request.HookKey,
                request.TenantId,
                request.EnvironmentTag,
                request.Cidr,
                request.Description,
                request.CreatedBy,
                now,
                now);

            var updated = definition.IpRules
                .Concat(new[] { rule })
                .OrderBy(r => r.Cidr, StringComparer.OrdinalIgnoreCase)
                .ToArray();

            _store[request.HookKey] = definition with { IpRules = updated };
            return Task.FromResult(rule);
        }
    }

    public Task DeleteIpRuleAsync(long ruleId, PartitionScope scope, string? deletedBy, string? correlationId, CancellationToken cancellationToken)
    {
        lock (_ipRuleLock)
        {
            foreach (var kvp in _store)
            {
                var definition = kvp.Value;
                if (!MatchesScope(definition, scope))
                {
                    continue;
                }

                if (!definition.IpRules.Any(rule => rule.Id == ruleId))
                {
                    continue;
                }

                var updated = definition.IpRules
                    .Where(rule => rule.Id != ruleId)
                    .ToArray();
                _store[kvp.Key] = definition with { IpRules = updated };
                break;
            }
        }

        return Task.CompletedTask;
    }

    public void Clear()
    {
        _store.Clear();
        Interlocked.Exchange(ref _ipRuleIdentity, 0);
    }

    private static bool MatchesScope(WebhookEndpointDefinition definition, PartitionScope scope)
    {
        return string.Equals(definition.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(definition.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static string GenerateSecret() => $"whsec_{Guid.NewGuid():N}";

    private static string ComputeHash(string secret)
    {
        var bytes = Encoding.UTF8.GetBytes(secret);
        var hash = System.Security.Cryptography.SHA256.HashData(bytes);
        return Convert.ToHexString(hash);
    }
}

/// <summary>
/// In-memory dead-letter store used for development scenarios.
/// </summary>
public sealed class InMemoryWebhookDeadLetterStore : IWebhookDeadLetterStore
{
    private readonly ConcurrentDictionary<long, WebhookDeadLetterEntry> _entries = new();
    private long _identity;

    public WebhookDeadLetterEntry Seed(
        string hookKey,
        string jobKey,
        PartitionScope scope,
        string payload,
        string failureReason = "failed",
        IReadOnlyDictionary<string, string>? metadata = null)
    {
        var id = Interlocked.Increment(ref _identity);
        var now = DateTimeOffset.UtcNow;
        var entry = new WebhookDeadLetterEntry(
            id,
            hookKey,
            jobKey,
            scope.TenantId,
            scope.EnvironmentTag,
            payload,
            null,
            metadata,
            failureReason,
            Attempts: 1,
            StatusCode: StatusCodes.Status500InternalServerError,
            ErrorDetails: "seeded",
            CreatedAtUtc: now,
            LastAttemptAtUtc: now,
            NextAttemptAtUtc: null,
            ExpiresAtUtc: now.AddDays(7));

        _entries[id] = entry;
        return entry;
    }

    public Task<long> CreateAsync(WebhookDeadLetterCreate request, CancellationToken cancellationToken)
    {
        var id = Interlocked.Increment(ref _identity);
        var now = DateTimeOffset.UtcNow;
        var entry = new WebhookDeadLetterEntry(
            id,
            request.HookKey,
            request.JobKey,
            request.TenantId,
            request.EnvironmentTag,
            request.Payload,
            request.Headers,
            request.Metadata,
            request.FailureReason,
            Attempts: 1,
            StatusCode: request.StatusCode,
            ErrorDetails: request.ErrorDetails,
            CreatedAtUtc: now,
            LastAttemptAtUtc: now,
            NextAttemptAtUtc: null,
            ExpiresAtUtc: request.ExpiresAtUtc);
        _entries[id] = entry;
        return Task.FromResult(id);
    }

    public Task<IReadOnlyCollection<WebhookDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        IReadOnlyCollection<WebhookDeadLetterEntry> result = _entries.Values
            .Where(entry => MatchesScope(entry, scope))
            .OrderBy(entry => entry.Id)
            .ToArray();
        return Task.FromResult(result);
    }

    public Task<WebhookDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (_entries.TryGetValue(id, out var entry) && MatchesScope(entry, scope))
        {
            return Task.FromResult<WebhookDeadLetterEntry?>(entry);
        }

        return Task.FromResult<WebhookDeadLetterEntry?>(null);
    }

    public Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (_entries.TryGetValue(id, out var entry) && MatchesScope(entry, scope))
        {
            _entries.TryRemove(id, out _);
        }

        return Task.CompletedTask;
    }

    public Task RecordFailureAsync(long id, PartitionScope scope, WebhookDeadLetterFailure failure, CancellationToken cancellationToken)
    {
        if (_entries.TryGetValue(id, out var entry) && MatchesScope(entry, scope))
        {
            var updated = entry with
            {
                FailureReason = failure.FailureReason,
                StatusCode = failure.StatusCode,
                ErrorDetails = failure.ErrorDetails,
                Attempts = entry.Attempts + 1,
                LastAttemptAtUtc = DateTimeOffset.UtcNow,
                NextAttemptAtUtc = failure.NextAttemptAtUtc
            };
            _entries[id] = updated;
        }

        return Task.CompletedTask;
    }

    public bool Contains(long id) => _entries.ContainsKey(id);

    public void Clear()
    {
        _entries.Clear();
        Interlocked.Exchange(ref _identity, 0);
    }

    private static bool MatchesScope(WebhookDeadLetterEntry entry, PartitionScope scope)
    {
        return string.Equals(entry.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(entry.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }
}
