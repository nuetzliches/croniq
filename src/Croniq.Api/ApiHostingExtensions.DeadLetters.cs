using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapScheduleDeadLetterEndpoints(WebApplication app)
    {
        app.MapGet("/tenants/{tenantId}/schedules/deadletters", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobDeadLetterStore? deadLetterStore,
            CancellationToken cancellationToken) =>
        {
            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "deadletter-unavailable", detail: "Dead-letter store not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var entries = await deadLetterStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);
            var response = entries.Select(ToScheduleDeadLetterResponse).ToList();
            return Results.Ok(response);
        })
        .WithDocs("ScheduleDeadLetters_List", "List schedule dead letters", "Enumerates failed trigger executions for investigation or replay.")
        .Produces<List<ScheduleDeadLetterResponse>>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(CroniqScopes.SchedulesDeadLetter);

        app.MapPost("/tenants/{tenantId}/schedules/deadletters/{deadLetterId}/replay", async (
            string tenantId,
            long deadLetterId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobDeadLetterStore? deadLetterStore,
            [FromServices] IJobRegistry registry,
            [FromServices] IJobExecutionPipeline pipeline,
            [FromServices] IPolicyResolver policyResolver,
            [FromServices] ILogger<ScheduleDeadLetterApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "deadletter-unavailable", detail: "Dead-letter store not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var entry = await deadLetterStore.FindAsync(deadLetterId, scope, cancellationToken).ConfigureAwait(false);
            if (entry is null)
            {
                return Results.NotFound(new { error = "deadletter-not-found", id = deadLetterId });
            }

            if (!JobKey.TryParse(entry.JobKey, out var jobKey))
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "invalid-job-key", detail: "Stored job key is invalid.");
            }

            if (!registry.TryGet(jobKey, out var descriptor))
            {
                return Results.Problem(statusCode: StatusCodes.Status404NotFound, title: "job-not-registered", detail: "Job not registered for this trigger.");
            }

            var metadata = entry.Metadata is null
                ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                : new Dictionary<string, string>(entry.Metadata, StringComparer.OrdinalIgnoreCase);

            if (!metadata.ContainsKey("trigger_id"))
            {
                metadata["trigger_id"] = entry.TriggerId;
            }

            metadata["deadletter:id"] = entry.Id.ToString(CultureInfo.InvariantCulture);
            metadata["deadletter:replay_at"] = DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture);

            var executionOptions = policyResolver.ResolveExecution(jobKey, scope);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, TriggerActivitySource);

            using var replayActivity = TriggerActivitySource.StartActivity("Croniq.Api.ScheduleDeadLetterReplay", ActivityKind.Server);
            replayActivity?.SetTag("croniq.deadletter.id", entry.Id);
            replayActivity?.SetTag("croniq.trigger.id", entry.TriggerId);
            replayActivity?.SetTag("croniq.job.key", jobKey.Value);

            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                await deadLetterStore.ResolveAsync(deadLetterId, scope, cancellationToken).ConfigureAwait(false);
                replayActivity?.SetStatus(ActivityStatusCode.Ok);
                return Results.Ok(new ScheduleReplayResult("replayed", entry.Id, entry.JobKey, entry.TriggerId));
            }
            catch (Exception ex)
            {
                replayActivity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                logger.LogError(ex, "failed to replay schedule deadletter {DeadLetterId}", deadLetterId);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "replay-failed", detail: ex.Message);
            }
        })
        .WithDocs("ScheduleDeadLetters_Replay", "Replay schedule dead letter", "Re-dispatches a failed trigger execution via the job execution pipeline.")
        .Produces<ScheduleReplayResult>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError)
        .RequireCroniqTenantScope(CroniqScopes.SchedulesDeadLetter);
    }

    private sealed class ScheduleDeadLetterApiMarker
    {
    }
}
