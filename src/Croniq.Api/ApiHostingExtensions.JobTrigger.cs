using System;
using System.Collections.Generic;
using System.Diagnostics;
using Croniq.Api.Models;
using Croniq.Api.Telemetry;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapJobTriggerEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/jobs/trigger", async (
            TriggerJobRequest request,
            [FromServices] IJobRegistry registry,
            [FromServices] IJobExecutionPipeline pipeline,
            [FromServices] IPolicyResolver policyResolver,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var authFailure = TenantGuard.EnsureJobScope(callerContextAccessor, jobKey, CroniqScopes.JobsTrigger);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (!registry.TryGet(jobKey, out var descriptor))
            {
                return Results.NotFound(new { error = "job-not-registered", request.JobKey });
            }

            var metadata = ToReadOnly(request.Metadata) ?? new Dictionary<string, string>();
            var executionOptions = policyResolver.ResolveExecution(jobKey);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, descriptor, executionOptions, metadata, TriggerActivitySource);

            using var triggerActivity = TriggerActivitySource.StartActivity("Croniq.Api.TriggerJob", ActivityKind.Server);
            triggerActivity?.SetTag("croniq.job.key", jobKey.Value);
            triggerActivity?.SetTag("croniq.tenant_id", jobKey.TenantId);
            triggerActivity?.SetTag("croniq.environment", jobKey.EnvironmentTag);
            triggerActivity?.SetTag("croniq.job.namespace", jobKey.NamespaceSegment);
            triggerActivity?.SetTag("croniq.job.name", jobKey.JobName);
            if (!string.IsNullOrWhiteSpace(jobKey.Variant))
            {
                triggerActivity?.SetTag("croniq.job.variant", jobKey.Variant);
            }

            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                ApiMetrics.RecordManualTrigger(jobKey);
                triggerActivity?.SetStatus(ActivityStatusCode.Ok);
                return Results.Accepted(value: new { status = "triggered", request.JobKey });
            }
            catch
            {
                triggerActivity?.SetStatus(ActivityStatusCode.Error);
                throw;
            }
        })
        .WithDocs("Jobs_Trigger", "Trigger a job manually", "Executes a job immediately using the provided metadata and job key.");
    }
}
