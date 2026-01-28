using System;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace Croniq.Runner;

public sealed class CroniqRunnerHostedService : BackgroundService
{
    private readonly CroniqRunner _runner;
    private readonly CroniqRunnerOptions _options;
    private readonly ILogger<CroniqRunnerHostedService> _logger;

    public CroniqRunnerHostedService(
        CroniqRunner runner,
        CroniqRunnerOptions options,
        ILogger<CroniqRunnerHostedService> logger)
    {
        _runner = runner ?? throw new ArgumentNullException(nameof(runner));
        _options = options ?? throw new ArgumentNullException(nameof(options));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        try
        {
            await _runner.StartAsync(stoppingToken).ConfigureAwait(false);
        }
        catch (RunnerIdInUseException ex)
        {
            _logger.LogError(ex, "RunnerId already in use; shutting down.");
            throw;
        }
        catch (RunnerMismatchException ex)
        {
            _logger.LogError(ex, "RunnerId mismatch; shutting down.");
            throw;
        }
        catch (RunnerJobRegistrationDeniedException ex)
        {
            _logger.LogError(ex, "Runner self-registration denied; shutting down.");
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Runner host terminated unexpectedly.");
            throw;
        }
    }

    public override async Task StopAsync(CancellationToken cancellationToken)
    {
        try
        {
            await _runner.DrainAsync(_options.DrainTimeout).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Runner drain failed.");
        }

        await base.StopAsync(cancellationToken).ConfigureAwait(false);
    }
}
