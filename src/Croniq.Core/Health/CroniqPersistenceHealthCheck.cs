using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Diagnostics.HealthChecks;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Health;

public sealed class CroniqPersistenceHealthCheck : IHealthCheck
{
    private readonly IJobPersistenceProvider _store;
    private readonly CroniqOptions _options;
    private readonly IPersistenceHealth? _health;

    public CroniqPersistenceHealthCheck(
        IJobPersistenceProvider store,
        IOptions<CroniqOptions> options,
        IPersistenceHealth? health = null)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _health = health;
    }

    public async Task<HealthCheckResult> CheckHealthAsync(HealthCheckContext context, CancellationToken cancellationToken = default)
    {
        var providerName = _store.GetType().FullName ?? "unknown";
        if (_health is not null)
        {
            try
            {
                var result = await _health.CheckAsync(cancellationToken).ConfigureAwait(false);
                var data = new Dictionary<string, object>
                {
                    ["provider"] = providerName
                };

                if (!string.IsNullOrWhiteSpace(result.Detail))
                {
                    data["detail"] = result.Detail;
                }

                return result.IsHealthy
                    ? HealthCheckResult.Healthy("persistence reachable", data: data)
                    : HealthCheckResult.Unhealthy("persistence unreachable", data: data);
            }
            catch (Exception ex)
            {
                return HealthCheckResult.Unhealthy("persistence unreachable", ex, new Dictionary<string, object>
                {
                    ["provider"] = providerName
                });
            }
        }

        try
        {
            var scope = new PartitionScope(_options.TenantId, _options.EnvironmentTag);
            _ = await _store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
            return HealthCheckResult.Healthy("persistence reachable", data: new Dictionary<string, object>
            {
                ["provider"] = providerName
            });
        }
        catch (Exception ex)
        {
            return HealthCheckResult.Unhealthy("persistence unreachable", ex, new Dictionary<string, object>
            {
                ["provider"] = providerName
            });
        }
    }
}
