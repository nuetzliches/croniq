using Croniq.Core.Execution;
using Croniq.Core.Options;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Hosting;

/// <summary>
/// Background service that continuously processes trigger batches.
/// </summary>
public sealed class CroniqWorkerHostedService : BackgroundService
{
    private readonly TriggerWorker _worker;
    private readonly ILogger<CroniqWorkerHostedService> _logger;
    private readonly CroniqOptions _options;
    private readonly WorkerHostOptions _hostOptions;

    public CroniqWorkerHostedService(
        TriggerWorker worker,
        IOptions<CroniqOptions> options,
        IOptions<WorkerHostOptions> hostOptions,
        ILogger<CroniqWorkerHostedService> logger)
    {
        _worker = worker ?? throw new ArgumentNullException(nameof(worker));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _hostOptions = hostOptions?.Value ?? throw new ArgumentNullException(nameof(hostOptions));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        _logger.LogInformation("Croniq worker starting for tenant {Tenant} / env {Environment} (instance {Instance})", _options.TenantId, _options.EnvironmentTag, _options.InstanceId);

        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                var processed = await _worker.ProcessBatchAsync(DateTimeOffset.UtcNow, _hostOptions.BatchSize, stoppingToken).ConfigureAwait(false);
                var delay = processed == 0 ? _hostOptions.IdleDelay : _hostOptions.BusyDelay;
                await Task.Delay(delay, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Croniq worker batch failed; retrying shortly");
                await Task.Delay(_hostOptions.ErrorDelay, stoppingToken).ConfigureAwait(false);
            }
        }

        _logger.LogInformation("Croniq worker stopping");
    }
}
