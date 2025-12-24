using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Hosting;

public sealed class CroniqTriggerSeedingHostedService : IHostedService
{
    private const string TriggerSectionPath = "Croniq:Triggers";
    private const string ManagedByMetadataKey = "managedBy";

    private readonly IConfiguration _configuration;
    private readonly IJobPersistenceProvider _store;
    private readonly IJobRegistry _jobs;
    private readonly CroniqOptions _coreOptions;
    private readonly CroniqSeedingOptions _seedingOptions;
    private readonly CroniqStartupOptions _startupOptions;
    private readonly ILogger<CroniqTriggerSeedingHostedService> _logger;
    private readonly IReadOnlyCollection<CroniqTriggerSeedRegistration> _fluentRegistrations;

    public CroniqTriggerSeedingHostedService(
        IConfiguration configuration,
        IJobPersistenceProvider store,
        IJobRegistry jobs,
        IEnumerable<CroniqTriggerSeedRegistration> fluentRegistrations,
        IOptions<CroniqOptions> coreOptions,
        IOptions<CroniqSeedingOptions> seedingOptions,
        IOptions<CroniqStartupOptions> startupOptions,
        ILogger<CroniqTriggerSeedingHostedService> logger)
    {
        _configuration = configuration ?? throw new ArgumentNullException(nameof(configuration));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _jobs = jobs ?? throw new ArgumentNullException(nameof(jobs));
        _fluentRegistrations = fluentRegistrations?.ToArray() ?? Array.Empty<CroniqTriggerSeedRegistration>();
        _coreOptions = coreOptions?.Value ?? throw new ArgumentNullException(nameof(coreOptions));
        _seedingOptions = seedingOptions?.Value ?? new CroniqSeedingOptions();
        _startupOptions = startupOptions?.Value ?? new CroniqStartupOptions();
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        var startupMode = CroniqStartupModeParser.Parse(_startupOptions.Mode);
        var mode = ParseMode(_seedingOptions.Mode);
        if (startupMode != CroniqStartupMode.Validate && mode == CroniqSeedingMode.Off)
        {
            _logger.LogInformation("Croniq trigger seeding is disabled.");
            return;
        }

        var definitions = LoadDefinitions();
        if (definitions.Count == 0)
        {
            var message = startupMode == CroniqStartupMode.Validate
                ? "Croniq trigger validation skipped; no triggers configured."
                : "Croniq trigger seeding skipped; no triggers configured.";
            _logger.LogInformation(message);
            return;
        }

        var scope = new PartitionScope(_coreOptions.TenantId.Trim(), _coreOptions.EnvironmentTag);
        var plans = BuildPlans(definitions, scope, mode);
        if (plans.Count == 0)
        {
            var message = startupMode == CroniqStartupMode.Validate
                ? "Croniq trigger validation skipped; no valid triggers resolved."
                : "Croniq trigger seeding skipped; no valid triggers resolved.";
            _logger.LogInformation(message);
            return;
        }

        if (startupMode == CroniqStartupMode.Validate)
        {
            _logger.LogInformation(
                "Croniq trigger validation completed with {Count} trigger definitions for {TenantId}/{EnvironmentTag}.",
                plans.Count,
                _coreOptions.TenantId.Trim(),
                _coreOptions.EnvironmentTag);
            return;
        }

        _logger.LogInformation(
            "Croniq trigger seeding starting (Mode={Mode}) with {Count} trigger definitions for {TenantId}/{EnvironmentTag}.",
            mode,
            plans.Count,
            _coreOptions.TenantId.Trim(),
            _coreOptions.EnvironmentTag);

        await EnsureJobsAsync(plans, scope, cancellationToken).ConfigureAwait(false);

        var existing = await _store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
        var existingById = existing.ToDictionary(t => t.TriggerId, StringComparer.OrdinalIgnoreCase);

        var created = 0;
        var updated = 0;
        var skipped = 0;

        foreach (var plan in plans)
        {
            if (existingById.TryGetValue(plan.TriggerId, out var existingTrigger))
            {
                if (mode == CroniqSeedingMode.CreateIfMissing)
                {
                    skipped++;
                    LogSkip(plan, "exists");
                    continue;
                }

                var existingManagedBy = ResolveManagedBy(existingTrigger.Metadata);
                if (string.IsNullOrWhiteSpace(existingManagedBy))
                {
                    skipped++;
                    LogSkip(plan, "missing-managedBy");
                    continue;
                }

                if (!string.Equals(existingManagedBy, plan.ManagedBy, StringComparison.OrdinalIgnoreCase))
                {
                    skipped++;
                    LogSkip(plan, "managedBy-mismatch");
                    continue;
                }

                await _store.UpsertTriggerAsync(plan.TriggerDefinition, cancellationToken).ConfigureAwait(false);
                updated++;
                _logger.LogInformation(
                    "Updated trigger {TriggerId} for {JobKey}. {Summary}",
                    plan.TriggerId,
                    plan.JobKey.Value,
                    plan.CronSummary);
                continue;
            }

            await _store.UpsertTriggerAsync(plan.TriggerDefinition, cancellationToken).ConfigureAwait(false);
            existingById[plan.TriggerId] = plan.TriggerDefinition;
            created++;
            _logger.LogInformation(
                "Created trigger {TriggerId} for {JobKey}. {Summary}",
                plan.TriggerId,
                plan.JobKey.Value,
                plan.CronSummary);
        }

        _logger.LogInformation(
            "Croniq trigger seeding completed. Created {Created}, Updated {Updated}, Skipped {Skipped}.",
            created,
            updated,
            skipped);
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private IReadOnlyList<CroniqTriggerSeedDefinition> LoadDefinitions()
    {
        var definitions = new List<CroniqTriggerSeedDefinition>();
        definitions.AddRange(LoadConfigDefinitions());
        definitions.AddRange(LoadFluentDefinitions());
        return definitions;
    }

    private IReadOnlyList<CroniqTriggerSeedDefinition> LoadConfigDefinitions()
    {
        var section = _configuration.GetSection(TriggerSectionPath);
        if (!section.Exists())
        {
            return Array.Empty<CroniqTriggerSeedDefinition>();
        }

        var definitions = new List<CroniqTriggerSeedDefinition>();
        foreach (var child in section.GetChildren())
        {
            var definition = child.Get<CroniqTriggerSeedDefinition>() ?? new CroniqTriggerSeedDefinition();
            if (string.IsNullOrWhiteSpace(definition.TriggerId) && IsMapKey(child.Key))
            {
                definition.TriggerId = child.Key;
            }

            definitions.Add(definition);
        }

        return definitions;
    }

    private IReadOnlyList<CroniqTriggerSeedDefinition> LoadFluentDefinitions()
    {
        if (_fluentRegistrations.Count == 0)
        {
            return Array.Empty<CroniqTriggerSeedDefinition>();
        }

        var definitions = new List<CroniqTriggerSeedDefinition>(_fluentRegistrations.Count);
        foreach (var registration in _fluentRegistrations)
        {
            var jobKey = JobKey.Create(
                registration.JobAttribute.NamespaceSegment,
                registration.JobAttribute.JobName,
                registration.JobAttribute.Variant);

            var metadata = registration.Metadata is null
                ? null
                : new Dictionary<string, string>(registration.Metadata, StringComparer.OrdinalIgnoreCase);

            definitions.Add(new CroniqTriggerSeedDefinition
            {
                TriggerId = registration.TriggerId,
                JobKey = jobKey.Value,
                CronExpression = registration.CronExpression,
                StartAtUtc = registration.StartAtUtc,
                EndAtUtc = registration.EndAtUtc,
                Enabled = registration.Enabled,
                Metadata = metadata,
                ManagedBy = registration.ManagedBy,
                TimeZoneId = registration.TimeZoneId
            });
        }

        return definitions;
    }

    private List<SeedPlan> BuildPlans(
        IReadOnlyList<CroniqTriggerSeedDefinition> definitions,
        PartitionScope scope,
        CroniqSeedingMode mode)
    {
        var errors = new List<string>();
        var plans = new List<SeedPlan>();
        var seenTriggerIds = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        for (var i = 0; i < definitions.Count; i++)
        {
            var definition = definitions[i];
            var label = BuildLabel(definition, i);

            if (!TriggerDefinitionValidator.TryValidate(definition, scope, out var validation, out var error))
            {
                errors.Add($"{label}: {error}");
                continue;
            }

            if (!_jobs.TryGet(validation.JobKey, out var descriptor))
            {
                errors.Add($"{label}: JobKey '{validation.JobKey.Value}' is not registered. Call AddCroniqJob<T>() or AddCroniqJob(...) for the job.");
                continue;
            }

            if (!seenTriggerIds.Add(validation.TriggerId))
            {
                errors.Add($"{label}: TriggerId '{validation.TriggerId}' is duplicated.");
                continue;
            }

            var managedBy = ResolveManagedBy(definition);
            if (mode == CroniqSeedingMode.ForceUpdate && string.IsNullOrWhiteSpace(managedBy))
            {
                errors.Add($"{label}: ManagedBy (or metadata.{ManagedByMetadataKey}) is required when seeding mode is ForceUpdate.");
                continue;
            }

            var metadata = BuildMetadata(definition, managedBy);
            var triggerDefinition = new TriggerDefinition(
                validation.TriggerId,
                validation.JobKey.Value,
                validation.ScheduleExpression,
                scope,
                validation.StartAtUtc,
                validation.EndAtUtc,
                definition.Enabled,
                metadata,
                validation.TimeZoneId);

            var jobDefinition = new JobDefinition(
                validation.JobKey.Value,
                descriptor.Attribute.NamespaceSegment,
                descriptor.Attribute.JobName,
                descriptor.Attribute.Variant,
                descriptor.JobType.Name,
                Metadata: null);

            plans.Add(new SeedPlan(validation.JobKey, triggerDefinition, jobDefinition, validation.Summary, managedBy));
        }

        if (errors.Count > 0)
        {
            throw new InvalidOperationException("Croniq trigger seeding validation failed:\n" + string.Join("\n", errors));
        }

        return plans;
    }

    private async Task EnsureJobsAsync(IReadOnlyList<SeedPlan> plans, PartitionScope scope, CancellationToken cancellationToken)
    {
        var jobDefinitions = new Dictionary<string, JobDefinition>(StringComparer.OrdinalIgnoreCase);
        foreach (var plan in plans)
        {
            if (!jobDefinitions.ContainsKey(plan.JobDefinition.JobKey))
            {
                jobDefinitions[plan.JobDefinition.JobKey] = plan.JobDefinition;
            }
        }

        foreach (var job in jobDefinitions.Values)
        {
            var existing = await _store.GetJobAsync(job.JobKey, scope, cancellationToken).ConfigureAwait(false);
            if (existing is null)
            {
                await _store.UpsertJobAsync(job, scope, cancellationToken).ConfigureAwait(false);
            }
        }
    }

    private void LogSkip(SeedPlan plan, string reason)
    {
        _logger.LogInformation(
            "Skipped trigger {TriggerId} for {JobKey} ({Reason}).",
            plan.TriggerId,
            plan.JobKey.Value,
            reason);
    }

    private static CroniqSeedingMode ParseMode(string? mode)
    {
        if (string.IsNullOrWhiteSpace(mode))
        {
            return CroniqSeedingMode.CreateIfMissing;
        }

        if (Enum.TryParse<CroniqSeedingMode>(mode, ignoreCase: true, out var parsed))
        {
            return parsed;
        }

        throw new InvalidOperationException($"Croniq seeding mode '{mode}' is invalid. Valid values: Off, CreateIfMissing, ForceUpdate.");
    }

    private static string BuildLabel(CroniqTriggerSeedDefinition definition, int index)
    {
        if (!string.IsNullOrWhiteSpace(definition.TriggerId))
        {
            return $"Croniq:Triggers[{definition.TriggerId}]";
        }

        if (!string.IsNullOrWhiteSpace(definition.JobKey))
        {
            return $"Croniq:Triggers[{definition.JobKey}]";
        }

        return $"Croniq:Triggers[{index}]";
    }

    private static bool IsMapKey(string key)
    {
        return !int.TryParse(key, NumberStyles.Integer, CultureInfo.InvariantCulture, out _);
    }

    private static string? ResolveManagedBy(CroniqTriggerSeedDefinition definition)
    {
        if (!string.IsNullOrWhiteSpace(definition.ManagedBy))
        {
            return definition.ManagedBy.Trim();
        }

        return ResolveManagedBy(definition.Metadata);
    }

    private static string? ResolveManagedBy(IReadOnlyDictionary<string, string>? metadata)
    {
        return TryGetMetadataValue(metadata, ManagedByMetadataKey);
    }

    private static IReadOnlyDictionary<string, string>? BuildMetadata(CroniqTriggerSeedDefinition definition, string? managedBy)
    {
        Dictionary<string, string>? result = null;

        if (definition.Metadata is { Count: > 0 })
        {
            result = new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);
        }

        if (!string.IsNullOrWhiteSpace(managedBy))
        {
            result ??= new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            result[ManagedByMetadataKey] = managedBy.Trim();
        }

        return result is { Count: > 0 } ? result : null;
    }

    private static string? TryGetMetadataValue(IReadOnlyDictionary<string, string>? metadata, string key)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        if (metadata.TryGetValue(key, out var value))
        {
            return value;
        }

        foreach (var pair in metadata)
        {
            if (string.Equals(pair.Key, key, StringComparison.OrdinalIgnoreCase))
            {
                return pair.Value;
            }
        }

        return null;
    }

    private sealed record SeedPlan(
        JobKey JobKey,
        TriggerDefinition TriggerDefinition,
        JobDefinition JobDefinition,
        string CronSummary,
        string? ManagedBy)
    {
        public string TriggerId => TriggerDefinition.TriggerId;
    }
}
