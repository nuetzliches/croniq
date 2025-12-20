using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Options;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Hosting;

public sealed class CroniqTriggerSummaryHostedService : IHostedService
{
    private readonly IJobPersistenceProvider _store;
    private readonly CroniqOptions _options;
    private readonly CroniqStartupOptions _startupOptions;
    private readonly ILogger<CroniqTriggerSummaryHostedService> _logger;

    public CroniqTriggerSummaryHostedService(
        IJobPersistenceProvider store,
        IOptions<CroniqOptions> options,
        IOptions<CroniqStartupOptions> startupOptions,
        ILogger<CroniqTriggerSummaryHostedService> logger)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _startupOptions = startupOptions?.Value ?? new CroniqStartupOptions();
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        var startupMode = CroniqStartupModeParser.Parse(_startupOptions.Mode);
        if (startupMode == CroniqStartupMode.Validate)
        {
            _logger.LogInformation("Croniq startup mode is Validate; trigger summary is disabled.");
            return;
        }

        var scope = new PartitionScope(_options.TenantId, _options.EnvironmentTag);
        using var logScope = _logger.BeginScope(new Dictionary<string, object?>
        {
            ["tenantId"] = scope.TenantId,
            ["environmentTag"] = scope.EnvironmentTag
        });

        var triggers = await _store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
        if (triggers.Count == 0)
        {
            _logger.LogInformation("Croniq trigger summary: no triggers found.");
            return;
        }

        var total = triggers.Count;
        var disabled = triggers.Count(t => !t.Enabled);
        var enabled = total - disabled;

        _logger.LogInformation(
            "Croniq trigger summary: {Total} total, {Enabled} enabled, {Disabled} disabled.",
            total,
            enabled,
            disabled);

        var now = DateTimeOffset.UtcNow;

        foreach (var trigger in triggers.OrderBy(t => t.JobKey).ThenBy(t => t.TriggerId))
        {
            DateTimeOffset? nextFireAtUtc = null;

            if (trigger.Enabled)
            {
                try
                {
                    nextFireAtUtc = ComputeNextFire(trigger, now);
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(
                        ex,
                        "Failed to compute next fire for trigger {TriggerId} ({JobKey}).",
                        trigger.TriggerId,
                        trigger.JobKey);
                }
            }

            _logger.LogInformation(
                "Trigger {TriggerId} for {JobKey} (enabled={Enabled}) next fire at {NextFireAtUtc}.",
                trigger.TriggerId,
                trigger.JobKey,
                trigger.Enabled,
                nextFireAtUtc);
        }
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private static DateTimeOffset? ComputeNextFire(TriggerDefinition trigger, DateTimeOffset referenceUtc)
    {
        return TriggerSchedule.GetNextOccurrence(
            trigger.ScheduleExpression,
            referenceUtc,
            trigger.StartAtUtc,
            trigger.EndAtUtc);
    }
}
