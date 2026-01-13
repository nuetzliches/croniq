using System;
using System.Diagnostics;
using Croniq.Api.Models;
using Croniq.Api.Telemetry;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Observability;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapJobTriggerEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/jobs/trigger", async (
            TriggerJobRequest request,
            HttpContext httpContext,
            [FromServices] IJobRegistry registry,
            [FromServices] IJobTrigger jobTrigger,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IOptions<CroniqOptions> options,
            CancellationToken cancellationToken) =>
        {
            if (!httpContext.Items.TryGetValue(typeof(JobKey), out var cached)
                || cached is not JobKey jobKey)
            {
                if (!JobKey.TryParse(request.JobKey, out jobKey))
                {
                    return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
                }
            }

            if (!registry.TryGet(jobKey, out _))
            {
                return Results.NotFound(new { error = "job-not-registered", request.JobKey });
            }

            if (request.DelaySeconds.HasValue && request.DelaySeconds.Value < 0)
            {
                return Results.BadRequest(new { error = "invalid-delay", message = "DelaySeconds must be zero or greater." });
            }

            var delay = request.DelaySeconds.HasValue
                ? TimeSpan.FromSeconds(request.DelaySeconds.Value)
                : (TimeSpan?)null;
            var metadata = ToReadOnly(request.Metadata);

            var currentOptions = options.Value ?? new CroniqOptions();
            var scope = new PartitionScope(currentOptions.TenantId.Trim(), currentOptions.EnvironmentTag);

            using var triggerActivity = TriggerActivitySource.StartActivity("Croniq.Api.TriggerJob", ActivityKind.Server);
            triggerActivity?.SetTag("croniq.job.key", jobKey.Value);
            triggerActivity?.SetTag("croniq.tenant_id", IdentifierHashing.HashTenantId(scope.TenantId));
            triggerActivity?.SetTag("croniq.environment", scope.EnvironmentTag);
            triggerActivity?.SetTag("croniq.job.namespace", jobKey.NamespaceSegment);
            triggerActivity?.SetTag("croniq.job.name", jobKey.JobName);
            if (delay.HasValue && delay.Value > TimeSpan.Zero)
            {
                triggerActivity?.SetTag("croniq.job.trigger_delay_seconds", delay.Value.TotalSeconds);
            }
            if (!string.IsNullOrWhiteSpace(jobKey.Variant))
            {
                triggerActivity?.SetTag("croniq.job.variant", jobKey.Variant);
            }

            try
            {
                await jobTrigger.TriggerOnceAsync(jobKey.Value, metadata, delay, cancellationToken).ConfigureAwait(false);
                ApiMetrics.RecordManualTrigger(jobKey, scope);
                triggerActivity?.SetStatus(ActivityStatusCode.Ok);
                var status = delay.HasValue && delay.Value > TimeSpan.Zero ? "scheduled" : "triggered";
                return Results.Accepted(value: new TriggerJobResponse(status, request.JobKey));
            }
            catch
            {
                triggerActivity?.SetStatus(ActivityStatusCode.Error);
                throw;
            }
        })
        .WithDocs("Jobs_Trigger", "Trigger a job manually", "Executes a job immediately or schedules a one-off run when DelaySeconds is provided.")
        .Produces<TriggerJobResponse>(StatusCodes.Status202Accepted)
        .RequireCroniqJobScopeFromBody<TriggerJobRequest>(request => request.JobKey, CroniqScopes.JobsTrigger);
    }
}
