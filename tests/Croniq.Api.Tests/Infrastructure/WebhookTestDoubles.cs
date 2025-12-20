using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;

namespace Croniq.Api.Tests.Infrastructure;

public sealed class RecordingJobExecutionPipeline : IJobExecutionPipeline
{
    private readonly List<JobExecutionRequest> _executions = new();

    public IReadOnlyList<JobExecutionRequest> Executions => _executions;

    public Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken)
    {
        _executions.Add(request);
        return Task.CompletedTask;
    }

    public void Clear() => _executions.Clear();
}

public sealed class FakeJobRegistry : IJobRegistry
{
    private readonly Dictionary<JobKey, JobDescriptor> _entries = new();

    public IReadOnlyCollection<JobDescriptor> Descriptors => _entries.Values.ToList();

    public bool TryGet(JobKey jobKey, out JobDescriptor descriptor)
    {
        return _entries.TryGetValue(jobKey, out descriptor!);
    }

    public JobDescriptor EnsureJob(string jobKey)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new ArgumentException("Invalid JobKey", nameof(jobKey));
        }

        return EnsureJob(parsed);
    }

    public JobDescriptor EnsureJob(JobKey jobKey)
    {
        if (_entries.TryGetValue(jobKey, out var descriptor))
        {
            return descriptor;
        }

        var attribute = new CroniqJobAttribute(jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant);
        descriptor = new JobDescriptor(typeof(FakeWebhookJob), attribute, jobKey);
        _entries[jobKey] = descriptor;
        return descriptor;
    }

    public void Clear() => _entries.Clear();

    [CroniqJob("test", "webhook")]
    private sealed class FakeWebhookJob
    {
    }
}

public sealed class FakePolicyResolver : IPolicyResolver
{
    public ExecutionPolicyOptions ResolveExecution(JobKey jobKey) => new();

    public MisfirePolicyOptions ResolveMisfire(JobKey jobKey) => new();

    public QuotaOptions ResolveQuota(JobKey jobKey) => new();

    public void Reset()
    {
        // nothing to reset for now
    }
}

public sealed class NoopJobPersistenceProvider : IJobPersistenceProvider, IPersistenceHealth
{
    private readonly object _sync = new();
    private readonly Dictionary<string, JobDefinition> _jobs = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, TriggerDefinition> _triggers = new(StringComparer.OrdinalIgnoreCase);

    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        return Task.FromResult<IReadOnlyCollection<TriggerLease>>(Array.Empty<TriggerLease>());
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken)
    {
        if (job is null)
        {
            throw new ArgumentNullException(nameof(job));
        }

        lock (_sync)
        {
            _jobs[job.JobKey] = CloneJob(job);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {

        lock (_sync)
        {
            var matches = _jobs.Values
                .Where(job => JobMatchesScope(job.JobKey, scope))
                .Select(CloneJob)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<JobDefinition>>(matches);
        }
    }

    public Task<JobDefinition?> GetJobAsync(string jobKey, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        lock (_sync)
        {
            if (!_jobs.TryGetValue(jobKey, out var job))
            {
                return Task.FromResult<JobDefinition?>(null);
            }

            return Task.FromResult<JobDefinition?>(CloneJob(job));
        }
    }

    public Task DeleteJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        lock (_sync)
        {
            if (!_jobs.TryGetValue(jobKey, out var job) || !JobMatchesScope(job.JobKey, scope))
            {
                return Task.CompletedTask;
            }

            _jobs.Remove(jobKey);

            var triggerIds = _triggers
                .Where(pair => string.Equals(pair.Value.JobKey, jobKey, StringComparison.OrdinalIgnoreCase))
                .Select(pair => pair.Key)
                .ToList();

            foreach (var triggerId in triggerIds)
            {
                _triggers.Remove(triggerId);
            }
        }

        return Task.CompletedTask;
    }

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
    {
        if (trigger is null)
        {
            throw new ArgumentNullException(nameof(trigger));
        }

        lock (_sync)
        {
            _triggers[trigger.TriggerId] = CloneTrigger(trigger);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            var matches = _triggers.Values
                .Where(t => string.Equals(t.Scope.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
                            && string.Equals(t.Scope.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
                .Select(CloneTrigger)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(matches);
        }
    }

    public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            if (_triggers.TryGetValue(triggerId, out var trigger)
                && string.Equals(trigger.Scope.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
                && string.Equals(trigger.Scope.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            {
                _triggers.Remove(triggerId);
            }
        }

        return Task.CompletedTask;
    }

    public Task<PersistenceHealthResult> CheckAsync(CancellationToken cancellationToken = default)
    {
        return Task.FromResult(new PersistenceHealthResult(true, "noop"));
    }

    public void Reset()
    {
        lock (_sync)
        {
            _jobs.Clear();
            _triggers.Clear();
        }
    }

    private static TriggerDefinition CloneTrigger(TriggerDefinition source)
    {
        IReadOnlyDictionary<string, string>? metadata = source.Metadata is null
            ? null
            : new Dictionary<string, string>(source.Metadata, StringComparer.OrdinalIgnoreCase);

        return new TriggerDefinition(
            source.TriggerId,
            source.JobKey,
            source.ScheduleExpression,
            source.Scope,
            source.StartAtUtc,
            source.EndAtUtc,
            source.Enabled,
            metadata);
    }

    private static JobDefinition CloneJob(JobDefinition job)
    {
        IReadOnlyDictionary<string, string>? metadata = job.Metadata is null
            ? null
            : new Dictionary<string, string>(job.Metadata, StringComparer.OrdinalIgnoreCase);

        return new JobDefinition(job.JobKey, job.Namespace, job.Name, job.Variant, job.Description, metadata);
    }

    private static bool JobMatchesScope(string jobKey, PartitionScope scope)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            return false;
        }

        return string.Equals(parsed.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(parsed.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }
}

public sealed class InMemoryJobDeadLetterStore : IJobDeadLetterStore
{
    private readonly object _sync = new();
    private readonly List<JobDeadLetterEntry> _entries = new();
    private long _sequence;

    public JobDeadLetterEntry Add(JobDeadLetterEntry entry)
    {
        lock (_sync)
        {
            var id = entry.Id > 0 ? entry.Id : Interlocked.Increment(ref _sequence);
            var stored = entry with { Id = id };
            _entries.Add(stored);
            return stored;
        }
    }

    public void Clear()
    {
        lock (_sync)
        {
            _entries.Clear();
        }
    }

    public Task<IReadOnlyCollection<JobDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            var result = _entries
                .Where(entry => MatchesScope(entry, scope))
                .OrderByDescending(entry => entry.CreatedAtUtc)
                .ToArray();
            return Task.FromResult<IReadOnlyCollection<JobDeadLetterEntry>>(result);
        }
    }

    public Task<JobDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            var match = _entries.FirstOrDefault(entry => entry.Id == id && MatchesScope(entry, scope));
            return Task.FromResult(match);
        }
    }

    public Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            var index = _entries.FindIndex(entry => entry.Id == id && MatchesScope(entry, scope));
            if (index >= 0)
            {
                _entries.RemoveAt(index);
            }
        }

        return Task.CompletedTask;
    }

    private static bool MatchesScope(JobDeadLetterEntry entry, PartitionScope scope)
    {
        return string.Equals(entry.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(entry.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }
}

public sealed class TestCallerContextFactory : ICallerContextFactory
{
    public const string ApiKey = "ak_itest.default";
    public const string DefaultTenantId = "tenant-itest";
    public const string DefaultEnvironment = "dev";

    private Dictionary<string, ICallerContext> _contexts;

    public TestCallerContextFactory()
    {
        _contexts = new Dictionary<string, ICallerContext>(StringComparer.Ordinal);
        Reset();
    }

    public Task<ICallerContext?> FromApiKeyAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(presentedKey))
        {
            return Task.FromResult<ICallerContext?>(null);
        }

        _contexts.TryGetValue(presentedKey, out var context);
        return Task.FromResult(context);
    }

    public Task<ICallerContext?> FromBearerTokenAsync(string bearerToken, CancellationToken cancellationToken = default)
    {
        return Task.FromResult<ICallerContext?>(null);
    }

    public void Reset()
    {
        var defaultContext = new CallerContext(
            DefaultTenantId,
            DefaultEnvironment,
            CallerType.ApiKey,
            CallerId: "itest-client",
            Scopes: new[]
            {
                CroniqScopes.SchedulesWrite,
                CroniqScopes.SchedulesDeadLetter,
                CroniqScopes.JobsRead,
                CroniqScopes.ExecutionsRead,
                CroniqScopes.JobsWrite,
                CroniqScopes.JobsTrigger,
                CroniqScopes.WebhooksRead,
                CroniqScopes.WebhooksWrite,
                CroniqScopes.WebhooksRotate,
                CroniqScopes.WebhooksDeadLetter,
                CroniqScopes.ApiKeysManage,
                CroniqScopes.TenantsAdmin
            });

        _contexts = new Dictionary<string, ICallerContext>(StringComparer.Ordinal)
        {
            [ApiKey] = defaultContext
        };
    }

    public void AddContext(string apiKey, ICallerContext context)
    {
        if (string.IsNullOrWhiteSpace(apiKey)) throw new ArgumentNullException(nameof(apiKey));
        _contexts[apiKey] = context ?? throw new ArgumentNullException(nameof(context));
    }
}

public sealed class FakeApiKeyStore : IApiKeyStore
{
    private readonly object _sync = new();
    private readonly Dictionary<string, ApiKeyRecord> _keys = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<(string TenantId, string ClientId), ClientRecord> _clients = new();

    public Task<ApiKeyIssueResult> IssueAsync(ApiKeyIssueRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        var keyId = $"ak_{Guid.NewGuid():N}";
        var secret = Guid.NewGuid().ToString("N");
        var expiresAt = request.Ttl.HasValue ? DateTimeOffset.UtcNow.Add(request.Ttl.Value) : (DateTimeOffset?)null;
        var scopes = request.Scopes?.ToArray() ?? Array.Empty<string>();

        lock (_sync)
        {
            _keys[keyId] = new ApiKeyRecord(
                keyId,
                request.TenantId,
                request.ClientId,
                request.EnvironmentTag,
                scopes,
                expiresAt,
                secret,
                IsActive: true);

            UpsertClientInternal(new ApiClientUpsertRequest(
                request.TenantId,
                request.ClientId,
                request.ClientId,
                request.EnvironmentTag,
                scopes,
                IsActive: true));
        }

        return Task.FromResult(new ApiKeyIssueResult(
            request.ClientId,
            request.TenantId,
            keyId,
            $"{keyId}.{secret}",
            request.EnvironmentTag,
            expiresAt));
    }

    public Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(keyId)) throw new ArgumentNullException(nameof(keyId));

        lock (_sync)
        {
            if (!_keys.TryGetValue(keyId, out var record) || !string.Equals(record.TenantId, tenantId, StringComparison.OrdinalIgnoreCase))
            {
                return Task.FromResult(false);
            }

            if (!record.IsActive)
            {
                return Task.FromResult(true);
            }

            _keys[keyId] = record with { IsActive = false };
            return Task.FromResult(true);
        }
    }

    public async Task<ApiKeyIssueResult?> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentNullException(nameof(tenantId));
        if (string.IsNullOrWhiteSpace(keyId)) throw new ArgumentNullException(nameof(keyId));

        ApiKeyRecord record;
        lock (_sync)
        {
            if (!_keys.TryGetValue(keyId, out var current) || !string.Equals(current.TenantId, tenantId, StringComparison.OrdinalIgnoreCase) || !current.IsActive)
            {
                return null;
            }

            _keys[keyId] = current with { IsActive = false };
            record = current;
        }

        TimeSpan? ttl = null;
        if (record.ExpiresAtUtc.HasValue)
        {
            var remaining = record.ExpiresAtUtc.Value - DateTimeOffset.UtcNow;
            if (remaining > TimeSpan.Zero)
            {
                ttl = remaining;
            }
        }

        var issueRequest = new ApiKeyIssueRequest(
            tenantId,
            record.ClientId,
            record.EnvironmentTag,
            record.Scopes,
            ttl);
        var result = await IssueAsync(issueRequest, cancellationToken).ConfigureAwait(false);
        return result;
    }

    public Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(presentedKey))
        {
            return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "missing"));
        }

        var (keyId, secret) = Split(presentedKey);
        if (keyId is null || secret is null)
        {
            return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "invalid"));
        }

        lock (_sync)
        {
            if (!_keys.TryGetValue(keyId, out var record) || !record.IsActive)
            {
                return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "revoked"));
            }

            if (!string.Equals(record.Secret, secret, StringComparison.Ordinal))
            {
                return Task.FromResult(new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "invalid-secret"));
            }

            return Task.FromResult(new ApiKeyValidationResult(true, record.TenantId, record.EnvironmentTag, keyId, record.Scopes, null));
        }
    }

    public Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        lock (_sync)
        {
            if (!_clients.TryGetValue((tenantId, clientId), out var record) || record.IsDeleted)
            {
                return Task.FromResult<ApiClientDescriptor?>(null);
            }

            return Task.FromResult<ApiClientDescriptor?>(ToDescriptor(record));
        }
    }

    public Task<ApiClientDescriptor> UpsertClientAsync(ApiClientUpsertRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        lock (_sync)
        {
            var record = UpsertClientInternal(request);
            return Task.FromResult(ToDescriptor(record));
        }
    }

    public Task<IReadOnlyCollection<ApiClientDescriptor>> ListClientsAsync(string tenantId, string? environmentTag, CancellationToken cancellationToken = default)
    {
        lock (_sync)
        {
            var comparer = StringComparer.OrdinalIgnoreCase;
            var matches = _clients.Values
                .Where(record => comparer.Equals(record.TenantId, tenantId) && !record.IsDeleted)
                .Where(record => string.IsNullOrWhiteSpace(environmentTag) || comparer.Equals(record.EnvironmentTag ?? string.Empty, environmentTag ?? string.Empty))
                .Select(ToDescriptor)
                .OrderBy(descriptor => descriptor.ClientId, comparer)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<ApiClientDescriptor>>(matches);
        }
    }

    public Task<bool> DeleteClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        lock (_sync)
        {
            if (!_clients.TryGetValue((tenantId, clientId), out var record))
            {
                return Task.FromResult(false);
            }

            _clients[(tenantId, clientId)] = record with { IsActive = false, IsDeleted = true };

            foreach (var pair in _keys.Where(kvp => string.Equals(kvp.Value.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(kvp.Value.ClientId, clientId, StringComparison.OrdinalIgnoreCase)).ToList())
            {
                _keys[pair.Key] = pair.Value with { IsActive = false };
            }

            return Task.FromResult(true);
        }
    }

    public void Reset()
    {
        lock (_sync)
        {
            _keys.Clear();
            _clients.Clear();
        }
    }

    private ClientRecord UpsertClientInternal(ApiClientUpsertRequest request)
    {
        var key = (request.TenantId, request.ClientId);
        var newScopes = CopyScopes(request.Scopes);
        var hasNewScopes = newScopes.Count > 0;

        if (_clients.TryGetValue(key, out var existing))
        {
            var scopes = hasNewScopes ? newScopes : existing.Scopes;
            var name = request.Name ?? existing.Name;
            var environment = request.EnvironmentTag ?? existing.EnvironmentTag;
            var updated = existing with
            {
                Name = name,
                EnvironmentTag = environment,
                Scopes = scopes,
                IsActive = request.IsActive,
                IsDeleted = false
            };
            _clients[key] = updated;
            return updated;
        }

        var record = new ClientRecord(
            request.TenantId,
            request.ClientId,
            request.Name ?? request.ClientId,
            request.EnvironmentTag,
            hasNewScopes ? newScopes : Array.Empty<string>(),
            request.IsActive,
            IsDeleted: false);
        _clients[key] = record;
        return record;
    }

    private static IReadOnlyCollection<string> CopyScopes(IReadOnlyCollection<string>? scopes)
    {
        if (scopes is null || scopes.Count == 0)
        {
            return Array.Empty<string>();
        }

        return scopes.ToArray();
    }

    private ApiClientDescriptor ToDescriptor(ClientRecord record)
    {
        return new ApiClientDescriptor(
            record.ClientId,
            record.TenantId,
            record.Name,
            record.EnvironmentTag,
            record.Scopes,
            record.IsActive && !record.IsDeleted,
            ResolveClientExpiration(record.TenantId, record.ClientId));
    }

    private DateTimeOffset? ResolveClientExpiration(string tenantId, string clientId)
    {
        var match = _keys.Values
            .Where(k => k.IsActive
                && string.Equals(k.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                && string.Equals(k.ClientId, clientId, StringComparison.OrdinalIgnoreCase)
                && k.ExpiresAtUtc.HasValue)
            .OrderBy(k => k.ExpiresAtUtc)
            .FirstOrDefault();

        return match?.ExpiresAtUtc;
    }

    private static (string? KeyId, string? Secret) Split(string presented)
    {
        var idx = presented.IndexOf('.');
        if (idx <= 0 || idx == presented.Length - 1)
        {
            return (null, null);
        }

        return (presented[..idx], presented[(idx + 1)..]);
    }

    private sealed record ApiKeyRecord(
        string KeyId,
        string TenantId,
        string ClientId,
        string? EnvironmentTag,
        IReadOnlyCollection<string> Scopes,
        DateTimeOffset? ExpiresAtUtc,
        string Secret,
        bool IsActive);

    private sealed record ClientRecord(
        string TenantId,
        string ClientId,
        string? Name,
        string? EnvironmentTag,
        IReadOnlyCollection<string> Scopes,
        bool IsActive,
        bool IsDeleted);
}

public sealed class TestTenantStore : ITenantStore
{
    private readonly object _sync = new();
    private readonly Dictionary<string, TenantDescriptor> _tenants = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, string> _references = new(StringComparer.OrdinalIgnoreCase);

    public TestTenantStore()
    {
        Reset();
    }

    public Task<TenantDescriptor> CreateAsync(string reference, string name, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(reference)) throw new ArgumentException("Reference is required", nameof(reference));
        if (string.IsNullOrWhiteSpace(name)) throw new ArgumentException("Name is required", nameof(name));

        var normalizedReference = Normalize(reference);
        var trimmedReference = reference.Trim();
        var trimmedName = name.Trim();
        TenantDescriptor descriptor;

        lock (_sync)
        {
            if (_references.TryGetValue(normalizedReference, out var tenantId) && _tenants.TryGetValue(tenantId, out var existing))
            {
                descriptor = existing with { Name = trimmedName, IsActive = true };
                _tenants[tenantId] = descriptor;
            }
            else
            {
                descriptor = new TenantDescriptor(GenerateTenantId(), trimmedReference, trimmedName, true, DateTimeOffset.UtcNow);
                _tenants[descriptor.TenantId] = descriptor;
                _references[normalizedReference] = descriptor.TenantId;
            }
        }

        return Task.FromResult(descriptor);
    }

    public Task<TenantDescriptor?> GetByReferenceAsync(string reference, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(reference)) throw new ArgumentException("Reference is required", nameof(reference));

        var normalized = Normalize(reference);
        lock (_sync)
        {
            if (_references.TryGetValue(normalized, out var tenantId) && _tenants.TryGetValue(tenantId, out var descriptor))
            {
                return Task.FromResult<TenantDescriptor?>(descriptor);
            }
        }

        return Task.FromResult<TenantDescriptor?>(null);
    }

    public Task<TenantDescriptor?> GetByIdAsync(string tenantId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        lock (_sync)
        {
            if (_tenants.TryGetValue(tenantId, out var descriptor))
            {
                return Task.FromResult<TenantDescriptor?>(descriptor);
            }
        }

        return Task.FromResult<TenantDescriptor?>(null);
    }

    public Task<IReadOnlyCollection<TenantDescriptor>> ListAsync(CancellationToken cancellationToken = default)
    {
        IReadOnlyCollection<TenantDescriptor> snapshot;
        lock (_sync)
        {
            snapshot = _tenants.Values
                .OrderBy(tenant => tenant.Reference, StringComparer.OrdinalIgnoreCase)
                .ToArray();
        }

        return Task.FromResult(snapshot);
    }

    public Task<bool> DeactivateAsync(string tenantId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        lock (_sync)
        {
            if (!_tenants.TryGetValue(tenantId, out var existing))
            {
                return Task.FromResult(false);
            }

            if (!existing.IsActive)
            {
                return Task.FromResult(true);
            }

            _tenants[tenantId] = existing with { IsActive = false };
            return Task.FromResult(true);
        }
    }

    public void Reset()
    {
        lock (_sync)
        {
            _tenants.Clear();
            _references.Clear();

            var descriptor = new TenantDescriptor(
                TestCallerContextFactory.DefaultTenantId,
                TestCallerContextFactory.DefaultTenantId,
                "Integration Tenant",
                true,
                DateTimeOffset.UtcNow);

            _tenants[descriptor.TenantId] = descriptor;
            _references[Normalize(descriptor.Reference)] = descriptor.TenantId;
        }
    }

    private static string Normalize(string reference) => reference.Trim().ToLowerInvariant();

    private static string GenerateTenantId() => $"tn_{Guid.NewGuid():N}";
}

public sealed class TestExecutionLogReader : IExecutionLogReader
{
    private readonly Dictionary<string, List<string>> _logs = new(StringComparer.OrdinalIgnoreCase);
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public void SetLog(string executionId, string tenantId, string? environmentTag)
    {
        if (string.IsNullOrWhiteSpace(executionId)) throw new ArgumentException("ExecutionId is required", nameof(executionId));
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        var start = new
        {
            type = "start",
            executionId,
            tenantId,
            environmentTag,
            jobKey = $"{tenantId}:{environmentTag}:tests:job"
        };

        _logs[executionId] = new List<string> { JsonSerializer.Serialize(start, _jsonOptions) };
    }

    public void Clear() => _logs.Clear();

    public async IAsyncEnumerable<string> ReadLinesAsync(string executionId, [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        if (_logs.TryGetValue(executionId, out var lines))
        {
            foreach (var line in lines)
            {
                yield return line;
                await Task.Yield();
            }
        }
    }
}

public sealed class TestExecutionHistoryReader : IExecutionHistoryReader
{
    private readonly List<ExecutionSummary> _summaries = new();
    private readonly object _sync = new();

    public void SetExecutions(IEnumerable<ExecutionSummary> summaries)
    {
        if (summaries is null)
        {
            throw new ArgumentNullException(nameof(summaries));
        }

        lock (_sync)
        {
            _summaries.Clear();
            _summaries.AddRange(summaries);
        }
    }

    public void AddExecution(ExecutionSummary summary)
    {
        if (summary is null)
        {
            throw new ArgumentNullException(nameof(summary));
        }

        lock (_sync)
        {
            _summaries.Add(summary);
        }
    }

    public void Clear()
    {
        lock (_sync)
        {
            _summaries.Clear();
        }
    }

    public Task<IReadOnlyList<ExecutionSummary>> ListExecutionsAsync(PartitionScope scope, ExecutionHistoryQuery? query, CancellationToken cancellationToken)
    {
        var normalized = (query ?? new ExecutionHistoryQuery()).Normalize();
        IReadOnlyList<ExecutionSummary> result;
        lock (_sync)
        {
            result = _summaries
                .Where(summary => string.Equals(summary.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(summary.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
                .Where(summary => Matches(summary, normalized))
                .OrderByDescending(summary => summary.StartedAtUtc)
                .Take(normalized.Limit)
                .ToList();
        }

        return Task.FromResult(result);
    }

    public Task<ExecutionSummary?> GetExecutionAsync(string executionId, CancellationToken cancellationToken)
    {
        ExecutionSummary? match;
        lock (_sync)
        {
            match = _summaries.FirstOrDefault(summary => string.Equals(summary.ExecutionId, executionId, StringComparison.OrdinalIgnoreCase));
        }

        return Task.FromResult(match);
    }

    private static bool Matches(ExecutionSummary summary, ExecutionHistoryQuery query)
    {
        if (query.JobKey is { Length: > 0 } && !string.Equals(summary.JobKey, query.JobKey, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (query.Status.HasValue)
        {
            if (!summary.Status.HasValue || summary.Status.Value != query.Status.Value)
            {
                return false;
            }
        }

        if (query.StartedAfterUtc.HasValue && summary.StartedAtUtc < query.StartedAfterUtc.Value)
        {
            return false;
        }

        if (query.StartedBeforeUtc.HasValue && summary.StartedAtUtc > query.StartedBeforeUtc.Value)
        {
            return false;
        }

        return true;
    }
}
