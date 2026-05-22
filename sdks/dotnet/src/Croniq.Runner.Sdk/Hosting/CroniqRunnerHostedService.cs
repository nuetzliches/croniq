using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Hosting;

/// <summary>
/// Generic Host adapter for <see cref="CroniqRunner"/>. Surfaces unhandled
/// runner exceptions to the host via <see cref="IHostApplicationLifetime.StopApplication"/>
/// so the process exits non-zero instead of silently hanging.
/// </summary>
internal sealed class CroniqRunnerHostedService(
    CroniqRunner runner,
    IHostApplicationLifetime lifetime,
    ILogger<CroniqRunnerHostedService> logger) : BackgroundService
{
    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        try
        {
            await runner.RunAsync(stoppingToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
        {
            // Expected during graceful shutdown.
        }
        catch (Exception ex)
        {
            logger.LogCritical(ex, "Croniq runner exited with unhandled exception");
            lifetime.StopApplication();
        }
    }
}
