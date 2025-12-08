using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Execution;

public sealed class DefaultJobExecutionPipeline : IJobExecutionPipeline
{
    private const string TriggerIdMetadataKey = "trigger_id";
    private const string InitiatorMetadataKey = "initiator";

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
        using var logScope = jobLogger.BeginScope(BuildLogScope(request.JobKey, metadata));

        using var activity = activitySource.StartActivity("Croniq.Job.Execute");
        activity?.SetTag("croniq.job.key", request.JobKey.Value);
        activity?.SetTag("croniq.job.namespace", request.JobKey.NamespaceSegment);
        activity?.SetTag("croniq.job.name", request.JobKey.JobName);
        if (!string.IsNullOrWhiteSpace(request.JobKey.Variant))
        {
            activity?.SetTag("croniq.job.variant", request.JobKey.Variant);
        }
        activity?.SetTag("croniq.tenant_id", request.JobKey.TenantId);
        activity?.SetTag("croniq.environment", request.JobKey.EnvironmentTag);
        var stopwatch = Stopwatch.StartNew();
        jobLogger.LogInformation("Trigger {JobKey} started", request.JobKey.Value);

        var executionOptions = request.ExecutionOptions ?? _policyResolver.ResolveExecution(request.JobKey);
        var pipeline = _pipelineProvider.Get(request.JobKey, executionOptions);

        var context = new JobExecutionContext(request.JobKey.ToString(), metadata, jobLogger, activitySource);

        try
        {
            await pipeline.ExecuteAsync(async token =>
            {
                var effectiveToken = executionOptions.Timeout.CancelExecutionOnTimeout ? token : cancellationToken;
                await job.ExecuteAsync(context, effectiveToken).ConfigureAwait(false);
            }, cancellationToken).ConfigureAwait(false);
            stopwatch.Stop();
            activity?.SetStatus(ActivityStatusCode.Ok);
            jobLogger.LogInformation("Trigger {JobKey} completed in {ElapsedMilliseconds} ms", request.JobKey.Value, stopwatch.ElapsedMilliseconds);
        }
        catch (Exception ex)
        {
            stopwatch.Stop();
            activity?.SetStatus(ActivityStatusCode.Error);
            jobLogger.LogError(ex, "Trigger {JobKey} failed after {ElapsedMilliseconds} ms", request.JobKey.Value, stopwatch.ElapsedMilliseconds);
            throw;
        }
    }

    private static IReadOnlyCollection<KeyValuePair<string, object?>> BuildLogScope(JobKey jobKey, IReadOnlyDictionary<string, string> metadata)
    {
        var scope = new List<KeyValuePair<string, object?>>
        {
            new("croniq.job.key", jobKey.Value),
            new("croniq.job.namespace", jobKey.NamespaceSegment),
            new("croniq.job.name", jobKey.JobName),
            new("croniq.tenant_id", jobKey.TenantId),
            new("croniq.environment", jobKey.EnvironmentTag)
        };

        if (!string.IsNullOrWhiteSpace(jobKey.Variant))
        {
            scope.Add(new KeyValuePair<string, object?>("croniq.job.variant", jobKey.Variant));
        }

        if (metadata.TryGetValue(TriggerIdMetadataKey, out var triggerId) && !string.IsNullOrWhiteSpace(triggerId))
        {
            scope.Add(new KeyValuePair<string, object?>("croniq.trigger.id", triggerId));
        }

        if (metadata.TryGetValue(InitiatorMetadataKey, out var initiator) && !string.IsNullOrWhiteSpace(initiator))
        {
            scope.Add(new KeyValuePair<string, object?>("croniq.trigger.initiator", initiator));
        }

        return scope;
    }
}
