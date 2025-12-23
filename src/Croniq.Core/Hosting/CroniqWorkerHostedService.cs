using Croniq.Core.Execution;
using Croniq.Options;
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
    private readonly CroniqStartupOptions _startupOptions;

    public CroniqWorkerHostedService(
        TriggerWorker worker,
        IOptions<CroniqOptions> options,
        IOptions<WorkerHostOptions> hostOptions,
        IOptions<CroniqStartupOptions> startupOptions,
        ILogger<CroniqWorkerHostedService> logger)
    {
        _worker = worker ?? throw new ArgumentNullException(nameof(worker));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _hostOptions = hostOptions?.Value ?? throw new ArgumentNullException(nameof(hostOptions));
        _startupOptions = startupOptions?.Value ?? new CroniqStartupOptions();
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        var startupMode = CroniqStartupModeParser.Parse(_startupOptions.Mode);
        if (startupMode == CroniqStartupMode.Validate)
        {
            _logger.LogInformation("Croniq startup mode is Validate; worker loops are disabled.");
            return;
        }

        _logger.LogInformation("Croniq worker starting for tenant {Tenant} / env {Environment} (instance {Instance})", _options.GetEffectiveTenantId(), _options.EnvironmentTag, _options.InstanceId);

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
