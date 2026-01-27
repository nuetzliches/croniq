using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using Croniq.Auth.Abstractions;
using Croniq.Api.Models;
using Croniq.Api.Telemetry;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapJobEndpoints(WebApplication app)
    {
        app.MapGet("/tenants/{tenantId}/jobs", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var jobs = await store.ListJobsAsync(scope, cancellationToken).ConfigureAwait(false);
            var payload = jobs.Select(ToJobResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Jobs_List", "List jobs", "Returns all job definitions for the tenant/environment scope.")
        .Produces<JobResponse[]>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(CroniqScopes.JobsRead);

        app.MapGet("/tenants/{tenantId}/jobs/{jobId}", async (
            string tenantId,
            string jobId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!JobKey.TryParse(jobId, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var job = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            if (job is null)
            {
                return Results.NotFound(new { error = "job-not-found", jobId });
            }

            return Results.Ok(ToJobResponse(job));
        })
        .WithDocs("Jobs_Get", "Get job", "Returns the job definition for the specified job key.")
        .Produces<JobResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScope(CroniqScopes.JobsRead);

        app.MapPost("/tenants/{tenantId}/jobs", async (
            string tenantId,
            string? environment,
            UpsertJobRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var isActive = request.IsActive ?? true;
            if (!request.IsActive.HasValue)
            {
                var existing = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
                if (existing is not null)
                {
                    isActive = existing.IsActive;
                }
            }

            if (!string.Equals(jobKey.NamespaceSegment, request.Namespace, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "namespace-mismatch", message = "Namespace must match the job key namespace segment." });
            }

            if (!string.Equals(jobKey.JobName, request.Name, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "name-mismatch", message = "Name must match the job key." });
            }

            var keyVariant = jobKey.Variant ?? string.Empty;
            var requestVariant = request.Variant ?? string.Empty;
            if (!string.Equals(keyVariant, requestVariant, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "variant-mismatch", message = "Variant must match the job key variant." });
            }

            var job = new JobDefinition(
                jobKey.Value,
                request.Namespace,
                request.Name,
                request.Variant,
                request.Description,
                ToReadOnly(request.Metadata),
                isActive);

            await store.UpsertJobAsync(job, scope, cancellationToken).ConfigureAwait(false);
            return Results.Created($"/tenants/{tenantId}/jobs/{Uri.EscapeDataString(job.JobKey)}", ToJobResponse(job));
        })
        .WithDocs("Jobs_Upsert", "Create or update job", "Creates or updates the job definition for the specified job key.")
        .Produces<JobResponse>(StatusCodes.Status201Created)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.JobsWrite);

        app.MapDelete("/tenants/{tenantId}/jobs/{jobId}", async (
            string tenantId,
            string jobId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!JobKey.TryParse(jobId, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            await store.DeleteJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Jobs_Delete", "Delete job", "Deletes the job definition and associated triggers within the tenant/environment scope.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.JobsWrite);

        app.MapPost("/tenants/{tenantId}/jobs/{jobId}/activate", async (
            string tenantId,
            string jobId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!JobKey.TryParse(jobId, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var job = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            if (job is null)
            {
                return Results.NotFound(new { error = "job-not-found", jobId });
            }

            if (job.IsActive)
            {
                return Results.Ok(ToJobResponse(job));
            }

            var updated = job with { IsActive = true };
            await store.UpsertJobAsync(updated, scope, cancellationToken).ConfigureAwait(false);
            return Results.Ok(ToJobResponse(updated));
        })
        .WithDocs("Jobs_Activate", "Activate job", "Activates a pending job so it can be dispatched.")
        .Produces<JobResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScope(CroniqScopes.JobsWrite);
    }

    private static void MapScheduleEndpoints(WebApplication app)
    {
        app.MapPost("/tenants/{tenantId}/schedules", async (
            string tenantId,
            string? environment,
            CroniqTriggerSeedDefinition request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] ICalendarStore calendarStore,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (!TriggerDefinitionValidator.TryValidate(request, scope: null, out var validation, out var error))
            {
                return Results.BadRequest(new { error = "invalid-request", message = error });
            }

            if (ContainsManagedBy(request))
            {
                return Results.BadRequest(new
                {
                    error = "managed-by-reserved",
                    message = "ManagedBy is reserved for config/fluent seeding. Omit managedBy from schedule API requests."
                });
            }

            var jobKey = validation.JobKey;

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, resolvedEnvironment, CroniqScopes.SchedulesWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);

            if (!string.IsNullOrWhiteSpace(validation.CalendarId))
            {
                var calendar = await calendarStore.FindAsync(validation.CalendarId, scope, cancellationToken).ConfigureAwait(false);
                if (calendar is null)
                {
                    return Results.BadRequest(new { error = "calendar-not-found", message = $"Calendar '{validation.CalendarId}' does not exist." });
                }
            }

            var metadata = ToReadOnly(request.Metadata);
            var job = new JobDefinition(
                jobKey.Value,
                jobKey.NamespaceSegment,
                jobKey.JobName,
                jobKey.Variant,
                request.Description,
                metadata);

            var trigger = new TriggerDefinition(
                validation.TriggerId,
                jobKey.Value,
                validation.ScheduleExpression,
                scope,
                validation.StartAtUtc,
                validation.EndAtUtc,
                request.Enabled,
                metadata,
                validation.TimeZoneId,
                validation.CalendarId);

            await store.UpsertJobAsync(job, scope, cancellationToken).ConfigureAwait(false);
            await store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);
            ApiMetrics.RecordScheduleUpsert(scope.TenantId, scope.EnvironmentTag, jobKey.Value);

            return Results.Created(
                $"/tenants/{tenantId}/schedules/{Uri.EscapeDataString(trigger.TriggerId)}",
                new ScheduleUpsertResult(trigger.TriggerId, trigger.JobKey, trigger.ScheduleExpression, trigger.CalendarId));
        })
        .WithDocs("Schedules_Upsert", "Create or update a schedule", "Registers a Cron-based trigger for the specified tenant-scoped job key.")
        .Produces<ScheduleUpsertResult>(StatusCodes.Status201Created)
        .Produces(StatusCodes.Status400BadRequest)
        .WithMetadata(new EndpointAuthExtensions.CroniqAuthEndpointGuardMetadata(
            EndpointAuthExtensions.CroniqAuthGuardKind.JobScopeDerived,
            new[] { CroniqScopes.SchedulesWrite },
            false));

        app.MapGet("/tenants/{tenantId}/schedules", async (
            string tenantId,
            string? environment,
            string? jobKey,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!string.IsNullOrWhiteSpace(jobKey))
            {
                if (!JobKey.TryParse(jobKey, out var parsed))
                {
                    return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
                }
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var triggers = await store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);

            if (!string.IsNullOrWhiteSpace(jobKey))
            {
                triggers = triggers
                    .Where(t => string.Equals(t.JobKey, jobKey, StringComparison.OrdinalIgnoreCase))
                    .ToArray();
            }

            var payload = triggers.Select(ToScheduleResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Schedules_List", "List schedules", "Returns all persisted schedules for the tenant/environment scope, optionally filtered by job key.")
        .Produces<ScheduleResponse[]>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(CroniqScopes.SchedulesWrite);

        app.MapGet("/tenants/{tenantId}/schedules/{triggerId}", async (
            string tenantId,
            string triggerId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var triggers = await store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
            var match = triggers.FirstOrDefault(t => string.Equals(t.TriggerId, triggerId, StringComparison.OrdinalIgnoreCase));
            if (match is null)
            {
                return Results.NotFound(new { error = "schedule-not-found", triggerId });
            }

            return Results.Ok(ToScheduleResponse(match));
        })
        .WithDocs("Schedules_Get", "Get schedule", "Returns the persisted schedule metadata for the requested trigger identifier.")
        .Produces<ScheduleResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScope(CroniqScopes.SchedulesWrite);

        app.MapDelete("/tenants/{tenantId}/schedules/{triggerId}", async (
            string tenantId,
            string triggerId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            await store.DeleteTriggerAsync(triggerId, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Schedules_Delete", "Delete schedule", "Deletes the persisted trigger for the tenant/environment scope.")
        .Produces(StatusCodes.Status204NoContent)
        .RequireCroniqTenantScope(CroniqScopes.SchedulesWrite);
    }

    private static bool ContainsManagedBy(CroniqTriggerSeedDefinition request)
    {
        if (!string.IsNullOrWhiteSpace(request.ManagedBy))
        {
            return true;
        }

        return ContainsMetadataKey(request.Metadata, "managedBy");
    }

    private static bool ContainsMetadataKey(IDictionary<string, string>? metadata, string key)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return false;
        }

        foreach (var pair in metadata)
        {
            if (string.Equals(pair.Key, key, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }
}
