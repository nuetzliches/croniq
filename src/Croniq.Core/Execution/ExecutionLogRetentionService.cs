using System;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Execution;

/// <summary>
/// Background service that deletes execution log files older than the retention window.
/// </summary>
public sealed class ExecutionLogRetentionService : BackgroundService
{
    private readonly ILogger<ExecutionLogRetentionService> _logger;
    private readonly FileExecutionLogStoreOptions _storeOptions;
    private readonly ExecutionLogRetentionOptions _retentionOptions;

    public ExecutionLogRetentionService(
        IOptions<FileExecutionLogStoreOptions> storeOptions,
        IOptions<ExecutionLogRetentionOptions> retentionOptions,
        ILogger<ExecutionLogRetentionService> logger)
    {
        _storeOptions = storeOptions?.Value ?? throw new ArgumentNullException(nameof(storeOptions));
        _retentionOptions = retentionOptions?.Value ?? throw new ArgumentNullException(nameof(retentionOptions));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                Sweep();
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Execution log retention sweep failed");
            }

            try
            {
                await Task.Delay(_retentionOptions.SweepInterval, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private void Sweep()
    {
        var basePath = string.IsNullOrWhiteSpace(_storeOptions.BasePath) ? "logs" : _storeOptions.BasePath;
        if (!Directory.Exists(basePath))
        {
            return;
        }

        var cutoff = DateTimeOffset.UtcNow.AddDays(-_retentionOptions.RetentionDays);
        var files = Directory.EnumerateFiles(basePath, "*.ndjson", SearchOption.AllDirectories);

        foreach (var file in files)
        {
            try
            {
                var info = new FileInfo(file);
                if (info.LastWriteTimeUtc <= cutoff.UtcDateTime)
                {
                    info.Delete();
                }
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Failed to delete log file {File}", file);
            }
        }

        // Clean up empty directories
        foreach (var dir in Directory.EnumerateDirectories(basePath, "*", SearchOption.AllDirectories).OrderByDescending(d => d.Length))
        {
            try
            {
                if (!Directory.EnumerateFileSystemEntries(dir).Any())
                {
                    Directory.Delete(dir, recursive: false);
                }
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Failed to delete directory {Directory}", dir);
            }
        }
    }
}
