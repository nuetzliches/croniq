using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Hosting;

public sealed class CroniqJobRegistrySyncHostedService : IHostedService
{
    private const string ManagedByMetadataKey = "managedBy";

    private readonly IJobPersistenceProvider _store;
    private readonly IJobRegistry _registry;
    private readonly CroniqOptions _coreOptions;
    private readonly CroniqJobRegistrySyncOptions _syncOptions;
    private readonly CroniqStartupOptions _startupOptions;
    private readonly ILogger<CroniqJobRegistrySyncHostedService> _logger;

    public CroniqJobRegistrySyncHostedService(
        IJobPersistenceProvider store,
        IJobRegistry registry,
        IOptions<CroniqOptions> coreOptions,
        IOptions<CroniqJobRegistrySyncOptions> syncOptions,
        IOptions<CroniqStartupOptions> startupOptions,
        ILogger<CroniqJobRegistrySyncHostedService> logger)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _registry = registry ?? throw new ArgumentNullException(nameof(registry));
        _coreOptions = coreOptions?.Value ?? throw new ArgumentNullException(nameof(coreOptions));
        _syncOptions = syncOptions?.Value ?? new CroniqJobRegistrySyncOptions();
        _startupOptions = startupOptions?.Value ?? new CroniqStartupOptions();
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        var startupMode = CroniqStartupModeParser.Parse(_startupOptions.Mode);
        var mode = ParseMode(_syncOptions.Mode);

        if (startupMode != CroniqStartupMode.Validate && mode == CroniqSeedingMode.Off)
        {
            _logger.LogInformation("Croniq job registry sync is disabled.");
            return;
        }

        var descriptors = _registry.Descriptors;
        if (descriptors.Count == 0)
        {
            var message = startupMode == CroniqStartupMode.Validate
                ? "Croniq job registry sync validation skipped; no jobs registered."
                : "Croniq job registry sync skipped; no jobs registered.";
            _logger.LogInformation(message);
            return;
        }

        var scope = new PartitionScope(_coreOptions.TenantId.Trim(), _coreOptions.EnvironmentTag);

        if (startupMode == CroniqStartupMode.Validate)
        {
            _logger.LogInformation(
                "Croniq job registry sync validation completed with {Count} jobs for {TenantId}/{EnvironmentTag}.",
                descriptors.Count,
                _coreOptions.TenantId.Trim(),
                _coreOptions.EnvironmentTag);
            return;
        }

        var managedBy = string.IsNullOrWhiteSpace(_syncOptions.ManagedBy)
            ? "croniq-job-registry-sync"
            : _syncOptions.ManagedBy.Trim();

        _logger.LogInformation(
            "Croniq job registry sync starting (Mode={Mode}) with {Count} jobs for {TenantId}/{EnvironmentTag}.",
            mode,
            descriptors.Count,
            _coreOptions.TenantId.Trim(),
            _coreOptions.EnvironmentTag);

        var created = 0;
        var updated = 0;
        var skipped = 0;

        Dictionary<string, JobDefinition>? existingByKey = null;
        if (mode == CroniqSeedingMode.ForceUpdate)
        {
            var existing = await _store.ListJobsAsync(scope, cancellationToken).ConfigureAwait(false);
            existingByKey = existing.ToDictionary(j => j.JobKey, StringComparer.OrdinalIgnoreCase);
        }

        foreach (var descriptor in descriptors)
        {
            var jobKey = descriptor.JobKey.Value;

            if (mode == CroniqSeedingMode.CreateIfMissing)
            {
                var existing = await _store.GetJobAsync(jobKey, scope, cancellationToken).ConfigureAwait(false);
                if (existing is not null)
                {
                    skipped++;
                    continue;
                }

                await _store.UpsertJobAsync(
                        BuildSyncedJob(descriptor, description: null, metadata: BuildSyncMetadata(null, managedBy)),
                        scope,
                        cancellationToken)
                    .ConfigureAwait(false);

                created++;
                continue;
            }

            existingByKey!.TryGetValue(jobKey, out var known);
            if (known is not null)
            {
                var existingManagedBy = ResolveManagedBy(known.Metadata);
                if (string.IsNullOrWhiteSpace(existingManagedBy))
                {
                    skipped++;
                    continue;
                }

                if (!string.Equals(existingManagedBy, managedBy, StringComparison.OrdinalIgnoreCase))
                {
                    skipped++;
                    continue;
                }

                var mergedMetadata = BuildSyncMetadata(known.Metadata, managedBy);

                await _store.UpsertJobAsync(
                        BuildSyncedJob(descriptor, known.Description, mergedMetadata),
                        scope,
                        cancellationToken)
                    .ConfigureAwait(false);

                updated++;
                continue;
            }

            await _store.UpsertJobAsync(
                    BuildSyncedJob(descriptor, description: null, metadata: BuildSyncMetadata(null, managedBy)),
                    scope,
                    cancellationToken)
                .ConfigureAwait(false);

            created++;
        }

        _logger.LogInformation(
            "Croniq job registry sync completed. Created {Created}, Updated {Updated}, Skipped {Skipped}.",
            created,
            updated,
            skipped);
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private static JobDefinition BuildSyncedJob(
        JobDescriptor descriptor,
        string? description,
        IReadOnlyDictionary<string, string>? metadata)
    {
        return new JobDefinition(
            descriptor.JobKey.Value,
            descriptor.Attribute.NamespaceSegment,
            descriptor.Attribute.JobName,
            descriptor.Attribute.Variant,
            description,
            metadata);
    }

    private static IReadOnlyDictionary<string, string>? BuildSyncMetadata(
        IReadOnlyDictionary<string, string>? existing,
        string managedBy)
    {
        var normalizedManagedBy = managedBy.Trim();
        if (string.IsNullOrWhiteSpace(normalizedManagedBy))
        {
            return existing;
        }

        if (existing is null || existing.Count == 0)
        {
            return new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            {
                [ManagedByMetadataKey] = normalizedManagedBy
            };
        }

        var clone = new Dictionary<string, string>(existing, StringComparer.OrdinalIgnoreCase)
        {
            [ManagedByMetadataKey] = normalizedManagedBy
        };

        return clone;
    }

    private static string? ResolveManagedBy(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        return metadata.TryGetValue(ManagedByMetadataKey, out var value) ? value : null;
    }

    private static CroniqSeedingMode ParseMode(string? mode)
    {
        if (string.IsNullOrWhiteSpace(mode))
        {
            return CroniqSeedingMode.Off;
        }

        if (Enum.TryParse<CroniqSeedingMode>(mode.Trim(), ignoreCase: true, out var parsed))
        {
            return parsed;
        }

        throw new InvalidOperationException(
            $"Croniq job registry sync mode '{mode}' is invalid. Valid values: Off, CreateIfMissing, ForceUpdate.");
    }
}
