using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Diagnostics.HealthChecks;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Health;

public sealed class CroniqTriggerHealthCheck : IHealthCheck
{
    private readonly IJobPersistenceProvider _store;
    private readonly CroniqOptions _options;
    private readonly CroniqHealthCheckOptions _healthOptions;

    public CroniqTriggerHealthCheck(
        IJobPersistenceProvider store,
        IOptions<CroniqOptions> options,
        IOptions<CroniqHealthCheckOptions> healthOptions)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _healthOptions = healthOptions?.Value ?? new CroniqHealthCheckOptions();
    }

    public async Task<HealthCheckResult> CheckHealthAsync(HealthCheckContext context, CancellationToken cancellationToken = default)
    {
        try
        {
            var scope = new PartitionScope(_options.TenantReference, _options.EnvironmentTag);
            var triggers = await _store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
            var data = new Dictionary<string, object>
            {
                ["triggerCount"] = triggers.Count,
                ["tenantId"] = scope.TenantId,
                ["environmentTag"] = scope.EnvironmentTag
            };

            if (_healthOptions.RequireTriggers && triggers.Count == 0)
            {
                return new HealthCheckResult(
                    HealthStatus.Degraded,
                    "no triggers loaded",
                    data: data);
            }

            return HealthCheckResult.Healthy("triggers loaded", data: data);
        }
        catch (Exception ex)
        {
            return HealthCheckResult.Unhealthy("trigger load failed", ex);
        }
    }
}
