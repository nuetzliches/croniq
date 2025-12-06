using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Policies;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Execution;

public sealed class DefaultJobExecutionPipeline : IJobExecutionPipeline
{
    private readonly IServiceScopeFactory _scopeFactory;
    private readonly ActivitySource _activitySource;
    private readonly ILogger<DefaultJobExecutionPipeline> _logger;
    private readonly IPolicyResolver _policyResolver;
    private readonly IExecutionPolicyPipelineProvider _pipelineProvider;

    public DefaultJobExecutionPipeline(
        IServiceScopeFactory scopeFactory,
        ActivitySource activitySource,
        IPolicyResolver policyResolver,
        IExecutionPolicyPipelineProvider pipelineProvider,
        ILogger<DefaultJobExecutionPipeline> logger)
    {
        _scopeFactory = scopeFactory ?? throw new ArgumentNullException(nameof(scopeFactory));
        _activitySource = activitySource ?? throw new ArgumentNullException(nameof(activitySource));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _pipelineProvider = pipelineProvider ?? throw new ArgumentNullException(nameof(pipelineProvider));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public async Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        using var scope = _scopeFactory.CreateScope();

        var job = (IJob)scope.ServiceProvider.GetRequiredService(request.Descriptor.JobType);
        var loggerFactory = scope.ServiceProvider.GetService<ILoggerFactory>();
        var jobLogger = loggerFactory?.CreateLogger(request.Descriptor.JobType) ?? _logger;
        var metadata = request.Metadata ?? new Dictionary<string, string>();
        var activitySource = request.ActivitySource ?? _activitySource;

        using var activity = activitySource.StartActivity("Croniq.Job.Execute");
        jobLogger.LogDebug("Starting job {JobKey}", request.JobKey.Value);

        var executionOptions = _policyResolver.ResolveExecution(request.JobKey);
        var pipeline = _pipelineProvider.Get(request.JobKey, executionOptions);

        var context = new JobExecutionContext(request.JobKey.ToString(), metadata, jobLogger, activitySource);

        await pipeline.ExecuteAsync(async token =>
        {
            var effectiveToken = executionOptions.Timeout.CancelExecutionOnTimeout ? token : cancellationToken;
            await job.ExecuteAsync(context, effectiveToken).ConfigureAwait(false);
        }, cancellationToken).ConfigureAwait(false);

        jobLogger.LogDebug("Completed job {JobKey}", request.JobKey.Value);
    }
}
