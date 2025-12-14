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
    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        return Task.FromResult<IReadOnlyCollection<TriggerLease>>(Array.Empty<TriggerLease>());
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        return Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(Array.Empty<TriggerDefinition>());
    }

    public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task<PersistenceHealthResult> CheckAsync(CancellationToken cancellationToken = default)
    {
        return Task.FromResult(new PersistenceHealthResult(true, "noop"));
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
                CroniqScopes.JobsTrigger,
                CroniqScopes.WebhooksRead,
                CroniqScopes.WebhooksWrite,
                CroniqScopes.WebhooksRotate,
                CroniqScopes.WebhooksDeadLetter,
                CroniqScopes.ApiKeysManage
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

            _clients[(request.TenantId, request.ClientId)] = new ClientRecord(
                request.TenantId,
                request.ClientId,
                request.EnvironmentTag,
                scopes);
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
            if (!_clients.TryGetValue((tenantId, clientId), out var record))
            {
                return Task.FromResult<ApiClientDescriptor?>(null);
            }

            var activeKey = _keys.Values
                .Where(k => string.Equals(k.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(k.ClientId, clientId, StringComparison.OrdinalIgnoreCase)
                    && k.IsActive)
                .OrderBy(k => k.ExpiresAtUtc ?? DateTimeOffset.MaxValue)
                .FirstOrDefault();

            var isActive = activeKey is not null;
            var expires = activeKey?.ExpiresAtUtc;

            return Task.FromResult<ApiClientDescriptor?>(new ApiClientDescriptor(
                clientId,
                tenantId,
                clientId,
                record.EnvironmentTag,
                record.Scopes,
                isActive,
                expires));
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
        string? EnvironmentTag,
        IReadOnlyCollection<string> Scopes);
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
