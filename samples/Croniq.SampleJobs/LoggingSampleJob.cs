using Croniq.Sdk;
using Microsoft.Extensions.Logging;

namespace Croniq.SampleJobs;

[CroniqJob("samples", "smoke")]
public sealed class LoggingSampleJob : IJob
{
    private readonly ILogger<LoggingSampleJob> _logger;

    public LoggingSampleJob(ILogger<LoggingSampleJob> logger)
    {
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
    {
        var metadataCount = context.Metadata?.Count ?? 0;
        _logger.LogInformation("Executing Croniq smoke job for {JobKey} with {MetadataCount} metadata entries", context.JobKey, metadataCount);
        return Task.CompletedTask;
    }
}
