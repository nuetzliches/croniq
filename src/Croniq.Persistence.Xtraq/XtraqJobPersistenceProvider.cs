using System;
using System.Collections.Generic;
using System.Data;
using System.Text.Json;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Xtraq.Core;
using Croniq.Persistence.Xtraq.Croniq;
using Microsoft.Data.SqlClient;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Persistence.Xtraq;

/// <summary>
/// Xtraq-backed implementation of the job persistence provider.
/// Generated Xtraq artefacts should be placed under <c>Xtraq/Generated</c> in this project
/// and wired up here once available.
/// </summary>
public sealed class XtraqJobPersistenceProvider : IJobPersistenceProvider
{
    private readonly ILogger<XtraqJobPersistenceProvider> _logger;
    private readonly XtraqOptions _options;
    private readonly XtraqDbContext _db;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public XtraqJobPersistenceProvider(
        XtraqDbContext db,
        IOptions<XtraqOptions> options,
        ILogger<XtraqJobPersistenceProvider> logger)
    {
        _db = db ?? throw new ArgumentNullException(nameof(db));
        _options = options.Value ?? throw new ArgumentNullException(nameof(options));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken)
    {
        if (job is null) throw new ArgumentNullException(nameof(job));

        var keyParts = JobKeyParts.Parse(job.JobKey);
        var jobRef = new JobRefRequest
        {
            JobKey = job.JobKey,
            TenantId = keyParts.TenantId,
            Environment = keyParts.Environment,
            Namespace = keyParts.Namespace,
            Name = keyParts.Name,
            Variant = keyParts.Variant,
            Description = job.Description,
            Metadata = SerializeMetadata(job.Metadata)
        };

        var request = new JobUpsertRequest
        {
            Job = new[] { jobRef },
            AllowDeletedReuse = false
        };

        return _db.JobUpsertAsync(request, cancellationToken);
    }

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
    {
        if (trigger is null) throw new ArgumentNullException(nameof(trigger));

        var triggerKey = trigger.TriggerId;
        var jobKeyParts = JobKeyParts.Parse(trigger.JobKey);

        return Task.Run(async () =>
        {
            var jobId = await ResolveJobIdAsync(jobKeyParts.JobKey, cancellationToken).ConfigureAwait(false);
            var triggerRef = new TriggerRefRequest
            {
                TriggerKey = triggerKey,
                JobKey = jobKeyParts.JobKey,
                TenantId = jobKeyParts.TenantId,
                JobId = jobId,
                Environment = jobKeyParts.Environment,
                Namespace = jobKeyParts.Namespace,
                Name = jobKeyParts.Name,
                Variant = jobKeyParts.Variant,
                CronExpression = trigger.ScheduleExpression,
                TimeZoneId = "UTC",
                StartAtUtc = trigger.StartAtUtc?.UtcDateTime,
                EndAtUtc = trigger.EndAtUtc?.UtcDateTime,
                Enabled = trigger.Enabled,
                Metadata = SerializeMetadata(trigger.Metadata)
            };

            var request = new TriggerUpsertRequest
            {
                Trigger = new[] { triggerRef },
                AllowDeletedReuse = false
            };

            await _db.TriggerUpsertAsync(request, cancellationToken).ConfigureAwait(false);
        }, cancellationToken);
    }

    public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        throw new NotSupportedException("Listing triggers is not supported by the current Xtraq contract.");
    }

    public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(triggerId)) throw new ArgumentNullException(nameof(triggerId));

        return Task.Run(async () =>
        {
            var triggerKeyParts = TriggerKeyParts.Parse(triggerId);
            var jobId = await ResolveJobIdAsync(triggerKeyParts.JobKey, cancellationToken).ConfigureAwait(false);

            var triggerRef = new TriggerRefRequest
            {
                TriggerKey = triggerKeyParts.TriggerKey,
                JobKey = triggerKeyParts.JobKey,
                TenantId = triggerKeyParts.TenantId,
                JobId = jobId,
                Environment = triggerKeyParts.Environment,
                Namespace = triggerKeyParts.Namespace,
                Name = triggerKeyParts.Name,
                Variant = triggerKeyParts.Variant,
                CronExpression = triggerKeyParts.CronExpression ?? string.Empty,
                TimeZoneId = triggerKeyParts.TimeZoneId ?? "UTC",
                StartAtUtc = null,
                EndAtUtc = null,
                Enabled = true,
                Metadata = null
            };

            var request = new TriggerDeleteRequest
            {
                Trigger = new[] { triggerRef }
            };

            await _db.TriggerDeleteAsync(request, cancellationToken).ConfigureAwait(false);
        }, cancellationToken);
    }

    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        throw new NotSupportedException("Trigger lease acquire is not yet implemented against Xtraq.");
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
    {
        throw new NotSupportedException("Trigger lease release is not yet implemented against Xtraq.");
    }

    private string? SerializeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0) return null;
        return JsonSerializer.Serialize(metadata, _jsonOptions);
    }

    private async Task<long> ResolveJobIdAsync(string jobKey, CancellationToken cancellationToken)
    {
        var result = await _db.JobFindByKeyAsync(new JobFindByKeyRequest { JobKey = jobKey }, cancellationToken).ConfigureAwait(false);
        if (result.Result?.JobId is not long id)
        {
            throw new InvalidOperationException($"Job '{jobKey}' not found.");
        }
        return id;
    }

    private sealed record JobKeyParts(string JobKey, int TenantId, string Environment, string Namespace, string Name, string? Variant)
    {
        public static JobKeyParts Parse(string key)
        {
            if (string.IsNullOrWhiteSpace(key)) throw new ArgumentNullException(nameof(key));
            var parts = key.Split(':');
            if (parts.Length < 4) throw new InvalidOperationException($"JobKey '{key}' must be formatted as 'tenantId:env:namespace:name[:variant]'.");
            if (!int.TryParse(parts[0], out var tenantId))
            {
                throw new InvalidOperationException($"JobKey '{key}' must start with numeric tenantId.");
            }
            var variant = parts.Length > 4 ? parts[4] : null;
            return new JobKeyParts(key, tenantId, parts[1], parts[2], parts[3], variant);
        }
    }

    private sealed record TriggerKeyParts(string TriggerKey, string JobKey, int TenantId, string Environment, string Namespace, string Name, string? Variant, string? CronExpression, string? TimeZoneId)
    {
        public static TriggerKeyParts Parse(string triggerKey)
        {
            if (string.IsNullOrWhiteSpace(triggerKey)) throw new ArgumentNullException(nameof(triggerKey));
            // Expected format: tenantId:env:namespace:name[:variant][:cron][:tz]
            var parts = triggerKey.Split(':');
            if (parts.Length < 4) throw new InvalidOperationException($"TriggerKey '{triggerKey}' must be formatted as 'tenantId:env:namespace:name[:variant]' at minimum.");
            if (!int.TryParse(parts[0], out var tenantId))
            {
                throw new InvalidOperationException($"TriggerKey '{triggerKey}' must start with numeric tenantId.");
            }

            var variant = parts.Length > 4 ? parts[4] : null;
            var cron = parts.Length > 5 ? parts[5] : null;
            var tz = parts.Length > 6 ? parts[6] : null;
            var jobKey = $"{parts[0]}:{parts[1]}:{parts[2]}:{parts[3]}{(variant is not null ? ":" + variant : string.Empty)}";
            return new TriggerKeyParts(triggerKey, jobKey, tenantId, parts[1], parts[2], parts[3], variant, cron, tz);
        }
    }
}
