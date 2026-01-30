using System;
using System.Diagnostics;
using Croniq.Api.Models;
using Croniq.Api.Telemetry;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Observability;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
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
            [FromServices] IJobPersistenceProvider store,
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

            if (request.DelaySeconds.HasValue && request.DelaySeconds.Value < 0)
            {
                return Results.BadRequest(new { error = "invalid-delay", message = "DelaySeconds must be zero or greater." });
            }

            var executionMode = string.IsNullOrWhiteSpace(request.ExecutionMode)
                ? ExecutionIntent.ExecutionModes.Normal
                : request.ExecutionMode.Trim().ToLowerInvariant();
            if (!string.Equals(executionMode, ExecutionIntent.ExecutionModes.Normal, StringComparison.OrdinalIgnoreCase)
                && !string.Equals(executionMode, ExecutionIntent.ExecutionModes.Test, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "invalid-execution-mode", message = "ExecutionMode must be 'normal' or 'test'." });
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

            if (!registry.TryGet(jobKey, out _))
            {
                var existing = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
                if (existing is null)
                {
                    return Results.NotFound(new { error = "job-not-registered", request.JobKey });
                }

                if (!existing.IsActive)
                {
                    return Results.Problem(
                        statusCode: StatusCodes.Status409Conflict,
                        title: "job-inactive",
                        detail: "Job is inactive and cannot be triggered.");
                }

                if (string.IsNullOrWhiteSpace(existing.AssignedRunnerId))
                {
                    return Results.BadRequest(new { error = "assignment-required", message = "AssignedRunnerId is required for active jobs." });
                }

                var fireAtUtc = delay.HasValue && delay.Value > TimeSpan.Zero
                    ? DateTimeOffset.UtcNow.Add(delay.Value)
                    : DateTimeOffset.UtcNow;
                var triggerId = $"{jobKey.Value}:once-{Guid.NewGuid():N}";
                var trigger = new TriggerDefinition(
                    triggerId,
                    jobKey.Value,
                    TriggerSchedule.OnceExpression,
                    scope,
                    fireAtUtc,
                    EndAtUtc: null,
                    Enabled: true,
                    metadata,
                    TimeZoneInfo.Utc.Id,
                    CalendarId: null,
                    executionMode,
                    ExecutionIntent.InvocationSources.Manual);

                await store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);
                ApiMetrics.RecordManualTrigger(jobKey, scope);
                triggerActivity?.SetStatus(ActivityStatusCode.Ok);
                var scheduledStatus = delay.HasValue && delay.Value > TimeSpan.Zero ? "scheduled" : "triggered";
                return Results.Accepted(value: new TriggerJobResponse(scheduledStatus, request.JobKey));
            }

            try
            {
                await jobTrigger.TriggerOnceAsync(
                    jobKey.Value,
                    metadata,
                    delay,
                    executionMode,
                    ExecutionIntent.InvocationSources.Manual,
                    cancellationToken).ConfigureAwait(false);
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
        .WithDocs("Jobs_Trigger", "Trigger a job manually", "Executes a job immediately when registered locally; otherwise enqueues a one-off trigger (DelaySeconds supported).")
        .Produces<TriggerJobResponse>(StatusCodes.Status202Accepted)
        .RequireCroniqJobScopeFromBody<TriggerJobRequest>(request => request.JobKey, CroniqScopes.JobsTrigger);
    }
}
