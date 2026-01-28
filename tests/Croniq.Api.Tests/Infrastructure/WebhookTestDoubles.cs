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
    public ExecutionPolicyOptions ResolveExecution(JobKey jobKey, PartitionScope? scope = null) => new();

    public MisfirePolicyOptions ResolveMisfire(JobKey jobKey, PartitionScope? scope = null) => new();

    public QuotaOptions ResolveQuota(JobKey jobKey, PartitionScope? scope = null) => new();

    public void Reset()
    {
        // nothing to reset for now
    }
}

public sealed class NoopJobPersistenceProvider : IJobPersistenceProvider, ICalendarStore, IPersistenceHealth
{
    private readonly object _sync = new();
    private readonly Dictionary<string, JobDefinition> _jobs = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, TriggerDefinition> _triggers = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, CalendarDefinition> _calendars = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, LeaseState> _leases = new(StringComparer.OrdinalIgnoreCase);
    private long _leaseSequence;

    private static readonly TimeSpan DefaultLeaseDuration = TimeSpan.FromSeconds(60);

    private sealed record LeaseState(
        string LeaseId,
        string OwnerInstanceId,
        DateTimeOffset ExpiresAtUtc);

    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        cancellationToken.ThrowIfCancellationRequested();

        lock (_sync)
        {
            var now = request.NowUtc;
            var scope = request.Scope;
            var matches = _triggers.Values
                .Where(t => string.Equals(t.Scope.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
                            && string.Equals(t.Scope.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase)
                            && t.Enabled
                            && (t.StartAtUtc is null || t.StartAtUtc <= now)
                            && (request.AllowTestExecutions
                                || !string.Equals(t.ExecutionMode, ExecutionIntent.ExecutionModes.Test, StringComparison.OrdinalIgnoreCase)))
                .Where(t => IsJobActive(scope, t.JobKey))
                .OrderBy(t => t.StartAtUtc ?? DateTimeOffset.MinValue)
                .ThenBy(t => t.TriggerId, StringComparer.OrdinalIgnoreCase)
                .ToList();

            var leases = new List<TriggerLease>(Math.Min(request.BatchSize, matches.Count));

            foreach (var trigger in matches)
            {
                if (leases.Count >= request.BatchSize)
                {
                    break;
                }

                if (_leases.TryGetValue(trigger.TriggerId, out var existing)
                    && existing.ExpiresAtUtc > now)
                {
                    continue;
                }

                var leaseId = $"l_{Interlocked.Increment(ref _leaseSequence)}";
                var executionId = Guid.NewGuid().ToString("N");
                var expiresAt = now.Add(DefaultLeaseDuration);
                _leases[trigger.TriggerId] = new LeaseState(leaseId, request.InstanceId, expiresAt);

                leases.Add(new TriggerLease(
                    leaseId,
                    trigger.TriggerId,
                    trigger.JobKey,
                    trigger.Scope,
                    FireAtUtc: trigger.StartAtUtc ?? now,
                    LeaseExpiresAtUtc: expiresAt,
                    Payload: null,
                    ExecutionId: executionId,
                    ExecutionMode: trigger.ExecutionMode,
                    InvocationSource: trigger.InvocationSource));
            }

            return Task.FromResult<IReadOnlyCollection<TriggerLease>>(leases);
        }
    }

    private bool IsJobActive(PartitionScope scope, string jobKey)
    {
        if (!_jobs.TryGetValue(BuildScopedJobKey(scope, jobKey), out var job))
        {
            return true;
        }

        return job.IsActive;
    }

    public Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        cancellationToken.ThrowIfCancellationRequested();

        lock (_sync)
        {
            var lease = request.Lease;
            if (!_leases.TryGetValue(lease.TriggerId, out var state))
            {
                return Task.FromResult<TriggerLease?>(null);
            }

            if (!string.Equals(state.LeaseId, lease.LeaseId, StringComparison.OrdinalIgnoreCase))
            {
                return Task.FromResult<TriggerLease?>(null);
            }

            if (!string.Equals(state.OwnerInstanceId, request.InstanceId, StringComparison.OrdinalIgnoreCase))
            {
                return Task.FromResult<TriggerLease?>(null);
            }

            if (state.ExpiresAtUtc <= request.NowUtc)
            {
                _leases.Remove(lease.TriggerId);
                return Task.FromResult<TriggerLease?>(null);
            }

            var extended = request.NowUtc.Add(DefaultLeaseDuration);
            _leases[lease.TriggerId] = state with { ExpiresAtUtc = extended };
            return Task.FromResult<TriggerLease?>(lease with { LeaseExpiresAtUtc = extended });
        }
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        cancellationToken.ThrowIfCancellationRequested();

        lock (_sync)
        {
            var lease = request.Lease;
            if (_leases.TryGetValue(lease.TriggerId, out var state)
                && string.Equals(state.LeaseId, lease.LeaseId, StringComparison.OrdinalIgnoreCase))
            {
                if (!string.Equals(state.OwnerInstanceId, request.InstanceId, StringComparison.OrdinalIgnoreCase))
                {
                    throw new InvalidOperationException($"Lease '{lease.LeaseId}' is owned by another instance.");
                }

                _leases.Remove(lease.TriggerId);
            }

            if (_triggers.TryGetValue(lease.TriggerId, out var trigger)
                && string.Equals(trigger.Scope.TenantId, lease.Scope.TenantId, StringComparison.OrdinalIgnoreCase)
                && string.Equals(trigger.Scope.EnvironmentTag, lease.Scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            {
                if (request.NextFireTimeUtc is null)
                {
                    _triggers.Remove(lease.TriggerId);
                }
                else
                {
                    _triggers[lease.TriggerId] = trigger with { StartAtUtc = request.NextFireTimeUtc };
                }
            }
        }

        return Task.CompletedTask;
    }

    public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task UpsertJobAsync(JobDefinition job, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (job is null)
        {
            throw new ArgumentNullException(nameof(job));
        }

        lock (_sync)
        {
            _jobs[BuildScopedJobKey(scope, job.JobKey)] = CloneJob(job);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            var prefix = BuildScopedJobKeyPrefix(scope);
            var matches = _jobs
                .Where(pair => pair.Key.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                .Select(pair => pair.Value)
                .Select(CloneJob)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<JobDefinition>>(matches);
        }
    }

    public Task<JobDefinition?> GetJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        lock (_sync)
        {
            if (!_jobs.TryGetValue(BuildScopedJobKey(scope, jobKey), out var job))
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
            var key = BuildScopedJobKey(scope, jobKey);
            if (!_jobs.ContainsKey(key))
            {
                return Task.CompletedTask;
            }

            _jobs.Remove(key);

            var triggerIds = _triggers
                .Where(pair => string.Equals(pair.Value.JobKey, jobKey, StringComparison.OrdinalIgnoreCase)
                               && string.Equals(pair.Value.Scope.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
                               && string.Equals(pair.Value.Scope.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
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

    public Task<CalendarDefinition?> FindAsync(string calendarId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(calendarId)) throw new ArgumentNullException(nameof(calendarId));

        lock (_sync)
        {
            if (_calendars.TryGetValue(BuildScopedCalendarKey(scope, calendarId), out var calendar))
            {
                return Task.FromResult<CalendarDefinition?>(calendar);
            }
        }

        return Task.FromResult<CalendarDefinition?>(null);
    }

    public Task<IReadOnlyCollection<CalendarDefinition>> ListCalendarsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        lock (_sync)
        {
            var prefix = BuildScopedCalendarKeyPrefix(scope);
            var matches = _calendars
                .Where(pair => pair.Key.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                .Select(pair => pair.Value)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<CalendarDefinition>>(matches);
        }
    }

    public Task UpsertAsync(CalendarUpsert request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        var key = BuildScopedCalendarKey(scope, request.CalendarId);
        var now = DateTimeOffset.UtcNow;

        lock (_sync)
        {
            if (_calendars.TryGetValue(key, out var existing))
            {
                _calendars[key] = existing with
                {
                    Name = request.Name,
                    Description = request.Description,
                    TimeZoneId = request.TimeZoneId,
                    Mode = request.Mode,
                    Rules = request.Rules,
                    Enabled = request.Enabled,
                    UpdatedAtUtc = now
                };
            }
            else
            {
                _calendars[key] = new CalendarDefinition(
                    request.CalendarId,
                    request.TenantId,
                    request.EnvironmentTag,
                    request.Name,
                    request.Description,
                    request.TimeZoneId,
                    request.Mode,
                    request.Rules,
                    request.Enabled,
                    now,
                    now);
            }
        }

        return Task.CompletedTask;
    }

    public Task DeleteAsync(string calendarId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(calendarId)) throw new ArgumentNullException(nameof(calendarId));

        lock (_sync)
        {
            _calendars.Remove(BuildScopedCalendarKey(scope, calendarId));
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
            _calendars.Clear();
            _leases.Clear();
            _leaseSequence = 0;
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
            metadata,
            source.TimeZoneId,
            source.CalendarId,
            source.ExecutionMode,
            source.InvocationSource);
    }

    private static JobDefinition CloneJob(JobDefinition job)
    {
        IReadOnlyDictionary<string, string>? metadata = job.Metadata is null
            ? null
            : new Dictionary<string, string>(job.Metadata, StringComparer.OrdinalIgnoreCase);

        return new JobDefinition(job.JobKey, job.Namespace, job.Name, job.Variant, job.Description, metadata, job.IsActive);
    }

    private static string BuildScopedJobKey(PartitionScope scope, string jobKey)
        => $"{scope.TenantId}|{scope.EnvironmentTag}|{jobKey}";

    private static string BuildScopedJobKeyPrefix(PartitionScope scope)
        => $"{scope.TenantId}|{scope.EnvironmentTag}|";

    private static string BuildScopedCalendarKey(PartitionScope scope, string calendarId)
        => $"{scope.TenantId}|{scope.EnvironmentTag}|{calendarId}";

    private static string BuildScopedCalendarKeyPrefix(PartitionScope scope)
        => $"{scope.TenantId}|{scope.EnvironmentTag}|";
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
                CroniqScopes.CalendarsRead,
                CroniqScopes.CalendarsWrite,
                CroniqScopes.JobsRead,
                CroniqScopes.ExecutionsRead,
                CroniqScopes.JobsWrite,
                CroniqScopes.JobsTrigger,
                CroniqScopes.WorkPoll,
                CroniqScopes.WorkRenew,
                CroniqScopes.WorkAck,
                CroniqScopes.WorkEvents,
                CroniqScopes.WorkersHeartbeat,
                CroniqScopes.WorkersRead,
                CroniqScopes.RunnersHeartbeat,
                CroniqScopes.RunnersRead,
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

    public TestTenantStore()
    {
        Reset();
    }

    public Task<TenantDescriptor> CreateAsync(TenantCreateRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.Name)) throw new ArgumentException("Name is required", nameof(request));

        var tenantId = string.IsNullOrWhiteSpace(request.TenantId)
            ? GenerateTenantId()
            : request.TenantId.Trim();
        var reference = string.IsNullOrWhiteSpace(request.Reference)
            ? tenantId
            : request.Reference.Trim();

        var trimmedName = request.Name.Trim();
        TenantDescriptor descriptor;

        lock (_sync)
        {
            if (_tenants.TryGetValue(tenantId, out var existing))
            {
                descriptor = existing with { Name = trimmedName, Reference = reference, IsActive = true };
                _tenants[tenantId] = descriptor;
            }
            else
            {
                descriptor = new TenantDescriptor(tenantId, trimmedName, true, DateTimeOffset.UtcNow, reference);
                _tenants[descriptor.TenantId] = descriptor;
            }
        }

        return Task.FromResult(descriptor);
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
                .OrderBy(tenant => tenant.TenantId, StringComparer.OrdinalIgnoreCase)
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

            var descriptor = new TenantDescriptor(
                TestCallerContextFactory.DefaultTenantId,
                "Integration Tenant",
                true,
                DateTimeOffset.UtcNow,
                TestCallerContextFactory.DefaultTenantId);

            _tenants[descriptor.TenantId] = descriptor;
        }
    }

    private static string GenerateTenantId() => $"tn_{Guid.NewGuid():N}";
}

public sealed class TestExecutionLogReader : IExecutionLogReader, IExecutionLogStore
{
    private readonly System.Collections.Concurrent.ConcurrentDictionary<string, List<string>> _logs = new(StringComparer.OrdinalIgnoreCase);
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web)
    {
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull
    };

    public void SetLog(string executionId, string tenantId, string? environmentTag)
    {
        if (string.IsNullOrWhiteSpace(executionId)) throw new ArgumentException("ExecutionId is required", nameof(executionId));
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        var start = new ExecutionRecord(
            executionId,
            ExecutionKind.Job,
            null,
            $"{tenantId}:{environmentTag}:tests:job",
            tenantId,
            environmentTag ?? string.Empty,
            TriggerId: null,
            FireAtUtc: DateTimeOffset.UtcNow,
            StartedAtUtc: DateTimeOffset.UtcNow,
            InstanceId: "itest",
            TraceId: null,
            SpanId: null,
            CorrelationId: null);

        WriteStartLine(start);
    }

    public void Clear() => _logs.Clear();

    public async IAsyncEnumerable<string> ReadLinesAsync(string executionId, [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        if (!_logs.TryGetValue(executionId, out var lines))
        {
            yield break;
        }

        List<string> snapshot;
        lock (lines)
        {
            snapshot = new List<string>(lines);
        }

        foreach (var line in snapshot)
        {
            yield return line;
            await Task.Yield();
        }
    }

    public Task OnExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken)
    {
        if (record is null) throw new ArgumentNullException(nameof(record));
        WriteStartLine(record);
        return Task.CompletedTask;
    }

    public Task AppendAsync(string executionId, IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(executionId)) throw new ArgumentException("ExecutionId is required", nameof(executionId));
        if (entries is null) throw new ArgumentNullException(nameof(entries));

        var lines = _logs.GetOrAdd(executionId, _ => new List<string>());
        lock (lines)
        {
            foreach (var entry in entries)
            {
                lines.Add(JsonSerializer.Serialize(new
                {
                    type = "log",
                    entry.ExecutionId,
                    entry.TimestampUtc,
                    entry.Level,
                    entry.MessageTemplate,
                    entry.RenderedMessage,
                    entry.Exception,
                    entry.Properties,
                    entry.TraceId,
                    entry.SpanId,
                    entry.CorrelationId,
                    entry.Sequence
                }, _jsonOptions));
            }
        }

        return Task.CompletedTask;
    }

    public Task OnExecutionCompletedAsync(ExecutionCompletion completion, CancellationToken cancellationToken)
    {
        if (completion is null) throw new ArgumentNullException(nameof(completion));

        var lines = _logs.GetOrAdd(completion.ExecutionId, _ => new List<string>());
        lock (lines)
        {
            lines.Add(JsonSerializer.Serialize(new
            {
                type = "completion",
                completion.ExecutionId,
                completion.CompletedAtUtc,
                completion.Status,
                completion.DurationMs,
                completion.ErrorType,
                completion.ErrorMessage
            }, _jsonOptions));
        }

        return Task.CompletedTask;
    }

    private void WriteStartLine(ExecutionRecord record)
    {
        var line = JsonSerializer.Serialize(new
        {
            type = "start",
            record.ExecutionId,
            record.Kind,
            record.WorkflowId,
            record.JobKey,
            record.TenantId,
            record.EnvironmentTag,
            record.TriggerId,
            record.FireAtUtc,
            record.StartedAtUtc,
            record.InstanceId,
            record.TraceId,
            record.SpanId,
            record.CorrelationId
        }, _jsonOptions);

        var lines = _logs.GetOrAdd(record.ExecutionId, _ => new List<string>());
        lock (lines)
        {
            lines.Clear();
            lines.Add(line);
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
