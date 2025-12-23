using Croniq.Sdk;
using Microsoft.Extensions.Logging;

namespace Croniq.Sample.Jobs;

[CroniqJob("samples", "logging-job")]
public sealed class LoggingSampleJob : IJob
{
    public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
    {
        var metadataCount = context.Metadata?.Count ?? 0;
        context.Logger.LogInformation("Executing Croniq smoke job for {JobKey} with {MetadataCount} metadata entries", context.JobKey, metadataCount);
        return Task.CompletedTask;
    }
}
