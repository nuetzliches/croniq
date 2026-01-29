using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using Croniq.Auth.Abstractions;
using Croniq.Api.Models;
using Croniq.Api.Telemetry;
using Croniq.Core;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Options;

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

        app.MapPost("/tenants/{tenantId}/jobs:register", async (
            string tenantId,
            string? environment,
            RunnerJobRegistrationRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IOptions<CroniqApiOptions> apiOptions,
            [FromServices] IJobPersistenceProvider store,
            [FromServices] IRunnerStore runnerStore,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(request.EnvironmentTag ?? environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (string.IsNullOrWhiteSpace(request.RunnerId))
            {
                return Results.BadRequest(new { error = "runner-required", message = "RunnerId is required." });
            }

            var runnerId = request.RunnerId.Trim();
            var runnerFailure = EnsureRunnerIdentity(callerContextAccessor, runnerId);
            if (runnerFailure is not null)
            {
                return runnerFailure;
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var existing = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            if (existing is not null)
            {
                var assignedRunnerMatches = string.Equals(existing.AssignedRunnerId, runnerId, StringComparison.OrdinalIgnoreCase);
                if (existing.IsActive && !assignedRunnerMatches)
                {
                    var canReassign = false;
                    if (!string.IsNullOrWhiteSpace(existing.AssignedRunnerId)
                        && string.Equals(existing.AssignmentSource, "runner", StringComparison.OrdinalIgnoreCase)
                        && runnerStore is not NoOpRunnerStore)
                    {
                        var lookup = new RunnerLookup(scope, existing.AssignedRunnerId, DateTimeOffset.UtcNow);
                        var assignedRunner = await runnerStore.TryGetAsync(lookup, cancellationToken).ConfigureAwait(false);
                        canReassign = assignedRunner is null;
                    }

                    if (!canReassign)
                    {
                        return Results.Problem(
                            statusCode: StatusCodes.Status409Conflict,
                            title: "job-assignment-conflict",
                            detail: "Job is already active and assigned to another runner.");
                    }

                    var reassigned = existing with
                    {
                        AssignedRunnerId = runnerId,
                        AssignedBy = runnerId,
                        AssignedAtUtc = DateTimeOffset.UtcNow,
                        AssignmentSource = "runner"
                    };
                    await store.UpsertJobAsync(reassigned, scope, cancellationToken).ConfigureAwait(false);
                    return Results.Ok(ToJobResponse(reassigned));
                }

                if (!assignedRunnerMatches)
                {
                    var updated = existing with
                    {
                        AssignedRunnerId = runnerId,
                        AssignedBy = runnerId,
                        AssignedAtUtc = DateTimeOffset.UtcNow,
                        AssignmentSource = "runner"
                    };
                    await store.UpsertJobAsync(updated, scope, cancellationToken).ConfigureAwait(false);
                    return Results.Ok(ToJobResponse(updated));
                }

                return Results.Ok(ToJobResponse(existing));
            }

            var registrationOptions = apiOptions.Value?.RunnerJobRegistration ?? new RunnerJobRegistrationOptions();
            var policy = registrationOptions.Resolve(scope);
            if (policy == RunnerJobRegistrationPolicy.Deny)
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status403Forbidden,
                    title: "runner-registration-denied",
                    detail: "Runner self-registration is not allowed for this tenant/environment.");
            }

            var metadata = BuildRunnerRegistrationMetadata(
                request.Metadata,
                runnerId,
                request.RunnerInstanceId,
                ResolveCallerIdentity(callerContextAccessor));

            var job = new JobDefinition(
                jobKey.Value,
                jobKey.NamespaceSegment,
                jobKey.JobName,
                jobKey.Variant,
                request.Description,
                metadata,
                IsActive: policy == RunnerJobRegistrationPolicy.AutoActivate,
                AssignedRunnerId: runnerId,
                AssignedBy: runnerId,
                AssignedAtUtc: DateTimeOffset.UtcNow,
                AssignmentSource: "runner");

            await store.UpsertJobAsync(job, scope, cancellationToken).ConfigureAwait(false);
            return Results.Created($"/tenants/{tenantId}/jobs/{Uri.EscapeDataString(job.JobKey)}", ToJobResponse(job));
        })
        .WithDocs("Jobs_Register", "Register job (runner)", "Registers a job definition via runner self-registration, honoring the registration policy.")
        .Produces<JobResponse>(StatusCodes.Status200OK)
        .Produces<JobResponse>(StatusCodes.Status201Created)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status403Forbidden)
        .RequireCroniqTenantScopeFromBodyOrQuery<RunnerJobRegistrationRequest>(r => r.EnvironmentTag, requireEnvironment: true, CroniqScopes.JobsRegister);

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
            var existing = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            var isActive = request.IsActive ?? existing?.IsActive ?? true;

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

            var normalizedAssignedRunnerId = NormalizeNullableString(request.AssignedRunnerId);
            var assignedRunnerId = normalizedAssignedRunnerId ?? existing?.AssignedRunnerId;
            var assignmentNotes = request.AssignmentNotes is null
                ? existing?.AssignmentNotes
                : NormalizeNullableString(request.AssignmentNotes);
            var assignmentChanged = normalizedAssignedRunnerId is not null
                && !string.Equals(normalizedAssignedRunnerId, existing?.AssignedRunnerId, StringComparison.OrdinalIgnoreCase);

            if (existing is not null && assignmentChanged && existing.IsActive && isActive)
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status409Conflict,
                    title: "job-assignment-conflict",
                    detail: "Reassignment is only allowed when the job is inactive.");
            }

            if (isActive && string.IsNullOrWhiteSpace(assignedRunnerId))
            {
                return Results.BadRequest(new { error = "assignment-required", message = "AssignedRunnerId is required for active jobs." });
            }

            var assignedBy = existing?.AssignedBy;
            var assignedAtUtc = existing?.AssignedAtUtc;
            var assignmentSource = existing?.AssignmentSource;
            if (assignmentChanged)
            {
                if (string.IsNullOrWhiteSpace(assignedRunnerId))
                {
                    assignedBy = null;
                    assignedAtUtc = null;
                    assignmentSource = null;
                }
                else
                {
                    assignedBy = ResolveCallerIdentity(callerContextAccessor);
                    assignedAtUtc = DateTimeOffset.UtcNow;
                    assignmentSource = "api";
                }
            }

            var job = new JobDefinition(
                jobKey.Value,
                request.Namespace,
                request.Name,
                request.Variant,
                request.Description,
                ToReadOnly(request.Metadata),
                isActive,
                assignedRunnerId,
                assignedBy,
                assignedAtUtc,
                assignmentSource,
                assignmentNotes);

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

            if (string.IsNullOrWhiteSpace(job.AssignedRunnerId))
            {
                return Results.BadRequest(new { error = "assignment-required", message = "AssignedRunnerId is required to activate a job." });
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
            var existing = await store.GetJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            var job = existing is null
                ? new JobDefinition(
                    jobKey.Value,
                    jobKey.NamespaceSegment,
                    jobKey.JobName,
                    jobKey.Variant,
                    request.Description,
                    metadata,
                    IsActive: false)
                : existing with
                {
                    Description = request.Description,
                    Metadata = metadata
                };

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

    private static string? NormalizeNullableString(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        return value.Trim();
    }

    private static IReadOnlyDictionary<string, string>? BuildRunnerRegistrationMetadata(
        IDictionary<string, string>? metadata,
        string runnerId,
        string? runnerInstanceId,
        string createdBy)
    {
        var result = metadata is null || metadata.Count == 0
            ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            : new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);

        result["registrationSource"] = "runner";
        result["createdBy"] = createdBy;
        result["createdAtUtc"] = DateTimeOffset.UtcNow.ToString("O");
        result["runnerId"] = runnerId;

        if (!string.IsNullOrWhiteSpace(runnerInstanceId))
        {
            result["runnerInstanceId"] = runnerInstanceId.Trim();
        }

        return result.Count == 0 ? null : result;
    }
}
