using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Sample;

internal sealed class SampleTriggerSeedHostedService : IHostedService
{
    private readonly IJobPersistenceProvider _store;
    private readonly IJobRegistry _jobs;
    private readonly IOptions<CroniqOptions> _coreOptions;
    private readonly ILogger<SampleTriggerSeedHostedService> _logger;

    public SampleTriggerSeedHostedService(
        IJobPersistenceProvider store,
        IJobRegistry jobs,
        IOptions<CroniqOptions> coreOptions,
        ILogger<SampleTriggerSeedHostedService> logger)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _jobs = jobs ?? throw new ArgumentNullException(nameof(jobs));
        _coreOptions = coreOptions ?? throw new ArgumentNullException(nameof(coreOptions));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        var options = _coreOptions.Value;
        var scope = new PartitionScope(options.TenantId, options.EnvironmentTag);

        var descriptor = _jobs.Descriptors.FirstOrDefault(d =>
            string.Equals(d.QualifiedName, "samples:smoke", StringComparison.OrdinalIgnoreCase));

        if (descriptor is null)
        {
            _logger.LogWarning("No job with QualifiedName '{QualifiedName}' registered; skipping sample trigger seeding.", "samples:smoke");
            return;
        }

        await _store.UpsertJobAsync(
            new JobDefinition(
                descriptor.JobKey.Value,
                descriptor.Attribute.NamespaceSegment,
                descriptor.Attribute.JobName,
                descriptor.Attribute.Variant,
                Description: descriptor.JobType.Name,
                Metadata: new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                {
                    ["source"] = "Croniq.Sample"
                }),
            cancellationToken);

        var triggerId = "samples-smoke-every-5s";

        await _store.UpsertTriggerAsync(
            new TriggerDefinition(
                triggerId,
                descriptor.JobKey.Value,
                ScheduleExpression: "0/5 * * * * ?",
                scope,
                StartAtUtc: DateTimeOffset.UtcNow,
                Metadata: new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                {
                    ["seededBy"] = "Croniq.Sample"
                }),
            cancellationToken);

        _logger.LogInformation(
            "Seeded sample trigger {TriggerId} for {JobKey} in {TenantId}/{EnvironmentTag}.",
            triggerId,
            descriptor.JobKey.Value,
            options.TenantId,
            options.EnvironmentTag);
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;
}

