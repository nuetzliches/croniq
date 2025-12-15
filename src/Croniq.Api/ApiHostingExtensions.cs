using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using System.Linq.Expressions;
using System.Reflection;
using System.Text.Json;
using System.Threading.RateLimiting;
using Croniq.Api.Models;
using Croniq.Api.Security;
using Croniq.Api.Telemetry;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Auth.SqlServer;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.Core.Security;
using Croniq.Data.SqlServer;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Providers.Default;
using Croniq.Sdk;
using Croniq.Hosting;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.OpenApi;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static class ApiHostingExtensions
{
    private static readonly ActivitySource TriggerActivitySource = new("Croniq.Api.Trigger");
    private const string CorrelationHeaderName = "X-Croniq-CorrelationId";

    public static IServiceCollection AddCroniqApiServices(this IServiceCollection services, IConfiguration configuration)
    {
        services.Configure<CroniqApiOptions>(configuration.GetSection("Croniq:Api"));
        services.AddCroniqPlatformServices(configuration);
        services.AddSingleton<TenantRateLimitDecider>();
        services.AddGrpc(options => options.Interceptors.Add<TenantRateLimitInterceptor>());
        return services;
    }

    public static WebApplication UseCroniqApi(this WebApplication app)
    {
        var apiOpts = app.Services.GetRequiredService<IOptions<CroniqApiOptions>>().Value;

        if (apiOpts.RequestsPerMinute > 0)
        {
            app.UseRateLimiter();
        }

        var anonymousPrefixes = new List<PathString>
        {
            "/health",
            "/webhooks"
        };

        if (apiOpts.AnonymousPathPrefixes?.Count > 0)
        {
            anonymousPrefixes.AddRange(apiOpts.AnonymousPathPrefixes.Select(p => new PathString(p)));
        }

        app.Use(async (context, next) =>
        {
            var path = context.Request.Path;
            if (anonymousPrefixes.Any(prefix => path.StartsWithSegments(prefix, StringComparison.OrdinalIgnoreCase)))
            {
                await next().ConfigureAwait(false);
                return;
            }

            var callerAccessor = context.RequestServices.GetRequiredService<ICallerContextAccessor>();
            var callerFactory = context.RequestServices.GetRequiredService<ICallerContextFactory>();
            var rateLimitDecider = context.RequestServices.GetRequiredService<TenantRateLimitDecider>();

            var authHeader = context.Request.Headers.Authorization.FirstOrDefault();
            if (!string.IsNullOrWhiteSpace(authHeader))
            {
                var bearerCaller = await callerFactory.FromBearerTokenAsync(authHeader, context.RequestAborted).ConfigureAwait(false);
                if (bearerCaller is null || !bearerCaller.IsActive)
                {
                    context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                    await context.Response.WriteAsync("invalid bearer token");
                    return;
                }

                callerAccessor.Current = bearerCaller;
                await next().ConfigureAwait(false);
                return;
            }

            var provided = context.Request.Headers["X-Croniq-Key"].FirstOrDefault();
            if (string.IsNullOrWhiteSpace(provided))
            {
                context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                await context.Response.WriteAsync("missing credentials");
                return;
            }

            var caller = await callerFactory.FromApiKeyAsync(provided, context.RequestAborted).ConfigureAwait(false);
            if (caller is null || !caller.IsActive)
            {
                context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                await context.Response.WriteAsync("invalid api key");
                return;
            }

            callerAccessor.Current = caller;
            await next().ConfigureAwait(false);
        });

        app.MapGet("/health", () => Results.Ok(new { status = "ok" }))
            .WithDocs("Health_Get", "Health probe", "Returns 200 when the Croniq API process is alive.");

        app.MapGet("/health/persistence", async ([FromServices] IServiceProvider sp, CancellationToken ct) =>
        {
            var provider = sp.GetService<IJobPersistenceProvider>();
            var providerName = provider?.GetType().FullName ?? "unknown";

            var health = sp.GetService<IPersistenceHealth>();
            if (health is null)
            {
                return Results.Ok(new { status = "ok", provider = providerName, note = "no-db-provider-configured" });
            }

            try
            {
                var result = await health.CheckAsync(ct).ConfigureAwait(false);
                if (result.IsHealthy)
                {
                    return Results.Ok(new { status = "ok", provider = providerName, db = "reachable" });
                }

                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unhealthy", detail: result.Detail);
            }
            catch (Exception ex)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unreachable", detail: ex.Message);
            }
        })
        .WithDocs("Health_Persistence_Get", "Persistence health", "Checks the configured job persistence provider for reachability.");

        app.MapGet("/me", ([FromServices] ICallerContextAccessor callerContextAccessor) =>
        {
            var caller = callerContextAccessor.Current;
            if (caller is null || !caller.IsActive)
            {
                return Results.Unauthorized();
            }

            return Results.Ok(ToCallerInfoResponse(caller));
        })
        .WithDocs("Caller_Get", "Inspect caller", "Returns the current caller context (tenant, environment, scopes) after authentication.");

        app.MapPost("/tenants", async (
            UpsertTenantRequest request,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (request is null || string.IsNullOrWhiteSpace(request.Reference) || string.IsNullOrWhiteSpace(request.Name))
            {
                return Results.BadRequest(new { error = "invalid-request", message = "Reference and name are required." });
            }

            var descriptor = await tenantStore.CreateAsync(request.Reference, request.Name, cancellationToken).ConfigureAwait(false);
            return Results.Created($"/tenants/{descriptor.TenantId}", ToTenantResponse(descriptor));
        })
        .WithDocs("Tenants_Create", "Create tenant", "Creates or updates a tenant record based on the provided reference and name.");

        app.MapGet("/tenants", async (
            string? state,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var normalizedState = string.IsNullOrWhiteSpace(state) ? "active" : state.Trim();
            if (!string.Equals(normalizedState, "active", StringComparison.OrdinalIgnoreCase)
                && !string.Equals(normalizedState, "all", StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "invalid-state", message = "state must be 'active' or 'all'." });
            }

            var tenants = await tenantStore.ListAsync(cancellationToken).ConfigureAwait(false);
            var filtered = string.Equals(normalizedState, "all", StringComparison.OrdinalIgnoreCase)
                ? tenants
                : tenants.Where(t => t.IsActive).ToArray();
            var payload = filtered.Select(ToTenantResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Tenants_List", "List tenants", "Returns tenant metadata. Use state=all to include inactive tenants.");

        app.MapGet("/tenants/{tenantId}", async (
            string tenantId,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "TenantId is required." });
            }

            var descriptor = await tenantStore.GetByIdAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (descriptor is null)
            {
                return Results.NotFound(new { error = "tenant-not-found", tenantId });
            }

            return Results.Ok(ToTenantResponse(descriptor));
        })
        .WithDocs("Tenants_Get", "Get tenant", "Returns tenant metadata for the provided tenant identifier.");

        app.MapDelete("/tenants/{tenantId}", async (
            string tenantId,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "TenantId is required." });
            }

            var deactivated = await tenantStore.DeactivateAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (!deactivated)
            {
                return Results.NotFound(new { error = "tenant-not-found", tenantId });
            }

            return Results.NoContent();
        })
        .WithDocs("Tenants_Deactivate", "Deactivate tenant", "Marks the tenant as inactive without deleting historical data.");

        app.MapGet("/tenants/{tenantId}/jobs", async (
            string tenantId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.JobsRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
            var jobs = await store.ListJobsAsync(scope, cancellationToken).ConfigureAwait(false);
            var payload = jobs.Select(ToJobResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Jobs_List", "List jobs", "Returns all job definitions for the tenant/environment scope.");

        app.MapGet("/tenants/{tenantId}/jobs/{jobId}", async (
            string tenantId,
            string jobId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            if (!JobKey.TryParse(jobId, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            if (!string.Equals(jobKey.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(jobKey.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "scope-mismatch", detail: "JobKey tenant/environment must match the request scope.");
            }

            var authFailure = TenantGuard.EnsureJobScope(callerContextAccessor, jobKey, CroniqScopes.JobsRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var job = await store.GetJobAsync(jobKey.Value, cancellationToken).ConfigureAwait(false);
            if (job is null)
            {
                return Results.NotFound(new { error = "job-not-found", jobId });
            }

            return Results.Ok(ToJobResponse(job));
        })
        .WithDocs("Jobs_Get", "Get job", "Returns the job definition for the specified job key.");

        app.MapPost("/tenants/{tenantId}/jobs", async (
            string tenantId,
            string environment,
            UpsertJobRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            if (!string.Equals(jobKey.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(jobKey.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "scope-mismatch", detail: "JobKey tenant/environment must match the request scope.");
            }

            var authFailure = TenantGuard.EnsureJobScope(callerContextAccessor, jobKey, CroniqScopes.JobsWrite);
            if (authFailure is not null)
            {
                return authFailure;
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
                ToReadOnly(request.Metadata));

            await store.UpsertJobAsync(job, cancellationToken).ConfigureAwait(false);
            return Results.Created($"/tenants/{tenantId}/jobs/{Uri.EscapeDataString(job.JobKey)}", ToJobResponse(job));
        })
        .WithDocs("Jobs_Upsert", "Create or update job", "Creates or updates the job definition for the specified job key.");

        app.MapDelete("/tenants/{tenantId}/jobs/{jobId}", async (
            string tenantId,
            string jobId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            if (!JobKey.TryParse(jobId, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            if (!string.Equals(jobKey.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(jobKey.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "scope-mismatch", detail: "JobKey tenant/environment must match the request scope.");
            }

            var authFailure = TenantGuard.EnsureJobScope(callerContextAccessor, jobKey, CroniqScopes.JobsWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
            await store.DeleteJobAsync(jobKey.Value, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Jobs_Delete", "Delete job", "Deletes the job definition and associated triggers within the tenant/environment scope.");

        app.MapGet("/tenants/{tenantId}/executions", async (
            string tenantId,
            string environment,
            string? jobKey,
            ExecutionStatus? status,
            DateTimeOffset? startedAfterUtc,
            DateTimeOffset? startedBeforeUtc,
            int? limit,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IExecutionHistoryReader historyReader,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            if (startedAfterUtc.HasValue && startedBeforeUtc.HasValue && startedAfterUtc.Value >= startedBeforeUtc.Value)
            {
                return Results.BadRequest(new { error = "invalid-range", message = "startedAfterUtc must be earlier than startedBeforeUtc." });
            }

            if (!string.IsNullOrWhiteSpace(jobKey))
            {
                if (!JobKey.TryParse(jobKey, out var parsed))
                {
                    return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
                }

                if (!string.Equals(parsed.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                    || !string.Equals(parsed.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
                {
                    return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "scope-mismatch", detail: "JobKey tenant/environment must match the request scope.");
                }
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.ExecutionsRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
            var query = new ExecutionHistoryQuery
            {
                JobKey = jobKey,
                Status = status,
                StartedAfterUtc = startedAfterUtc,
                StartedBeforeUtc = startedBeforeUtc,
                Limit = Math.Clamp(limit ?? ExecutionHistoryQuery.DefaultLimit, 1, ExecutionHistoryQuery.MaxLimit)
            };

            var summaries = await historyReader.ListExecutionsAsync(scope, query, cancellationToken).ConfigureAwait(false);
            var payload = summaries.Select(ToExecutionResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Executions_List", "List executions", "Returns execution summaries for the tenant/environment scope with optional filters.");

        app.MapGet("/tenants/{tenantId}/executions/{executionId}", async (
            string tenantId,
            string executionId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IExecutionHistoryReader historyReader,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.ExecutionsRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var summary = await historyReader.GetExecutionAsync(executionId, cancellationToken).ConfigureAwait(false);
            if (summary is null
                || !string.Equals(summary.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(summary.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return Results.NotFound(new { error = "execution-not-found", executionId });
            }

            return Results.Ok(ToExecutionResponse(summary));
        })
        .WithDocs("Executions_Get", "Get execution", "Returns metadata for a single execution in the tenant/environment scope.");

        app.MapPost("/tenants/{tenantId}/schedules", async (
            string tenantId,
            UpsertScheduleRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(request.JobKey) || string.IsNullOrWhiteSpace(request.CronExpression))
            {
                return Results.BadRequest(new { error = "invalid-request", message = "JobKey and CronExpression are required." });
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            if (!string.Equals(jobKey.TenantId, tenantId, StringComparison.OrdinalIgnoreCase))
            {
                return Results.StatusCode(StatusCodes.Status403Forbidden);
            }

            var authFailure = TenantGuard.EnsureJobScope(callerContextAccessor, jobKey, CroniqScopes.SchedulesWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var triggerId = string.IsNullOrWhiteSpace(request.TriggerId)
                ? $"{request.JobKey}:{request.CronExpression}"
                : request.TriggerId;

            var scope = new PartitionScope(jobKey.TenantId, jobKey.EnvironmentTag);

            var metadata = ToReadOnly(request.Metadata);
            var job = new JobDefinition(
                jobKey.Value,
                jobKey.NamespaceSegment,
                jobKey.JobName,
                jobKey.Variant,
                request.Description,
                metadata);

            var trigger = new TriggerDefinition(
                triggerId,
                jobKey.Value,
                request.CronExpression,
                scope,
                request.StartAtUtc,
                request.EndAtUtc,
                request.Enabled,
                metadata);

            await store.UpsertJobAsync(job, cancellationToken).ConfigureAwait(false);
            await store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);
            ApiMetrics.RecordScheduleUpsert(jobKey.TenantId, jobKey.EnvironmentTag, jobKey.Value);

            return Results.Created($"/tenants/{tenantId}/schedules/{trigger.TriggerId}", new { trigger.TriggerId, trigger.JobKey, trigger.ScheduleExpression });
        })
        .WithDocs("Schedules_Upsert", "Create or update a schedule", "Registers a Cron-based trigger for the specified tenant-scoped job key.");

        app.MapGet("/tenants/{tenantId}/schedules", async (
            string tenantId,
            string environment,
            string? jobKey,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            if (!string.IsNullOrWhiteSpace(jobKey))
            {
                if (!JobKey.TryParse(jobKey, out var parsed))
                {
                    return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
                }

                if (!string.Equals(parsed.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                    || !string.Equals(parsed.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
                {
                    return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "scope-mismatch", detail: "JobKey tenant/environment must match the request scope.");
                }
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.SchedulesWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
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
        .WithDocs("Schedules_List", "List schedules", "Returns all persisted schedules for the tenant/environment scope, optionally filtered by job key.");

        app.MapGet("/tenants/{tenantId}/schedules/{triggerId}", async (
            string tenantId,
            string triggerId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.SchedulesWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
            var triggers = await store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
            var match = triggers.FirstOrDefault(t => string.Equals(t.TriggerId, triggerId, StringComparison.OrdinalIgnoreCase));
            if (match is null)
            {
                return Results.NotFound(new { error = "schedule-not-found", triggerId });
            }

            return Results.Ok(ToScheduleResponse(match));
        })
        .WithDocs("Schedules_Get", "Get schedule", "Returns the persisted schedule metadata for the requested trigger identifier.");

        app.MapDelete("/tenants/{tenantId}/schedules/{triggerId}", async (
            string tenantId,
            string triggerId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.SchedulesWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
            await store.DeleteTriggerAsync(triggerId, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Schedules_Delete", "Delete schedule", "Deletes the persisted trigger for the tenant/environment scope.");

        app.MapGet("/tenants/{tenantId}/webhooks", async (
            string tenantId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var scope = new PartitionScope(tenantId, environment);
            var endpoints = await webhookStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);
            var response = endpoints.Select(def => ToWebhookResponse(def)).ToList();
            return Results.Ok(response);
        })
        .WithDocs("Webhooks_List", "List webhook endpoints", "Returns all webhook endpoints for the specified tenant/environment scope.");

        app.MapPost("/tenants/{tenantId}/webhooks", async (
            string tenantId,
            string environment,
            bool allowUnsigned,
            UpsertWebhookEndpointRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            [FromServices] IConfiguration configuration,
            [FromServices] ILogger<WebhookEndpointApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            if (!string.Equals(jobKey.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(jobKey.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "scope-mismatch", message = "JobKey tenant/environment must match the request scope." });
            }

            var defaultLimit = configuration.GetValue<int?>("Croniq:Webhooks:RequestsPerMinute") ?? 60;
            var rpm = request.RequestsPerMinute ?? defaultLimit;
            if (rpm <= 0)
            {
                return Results.BadRequest(new { error = "invalid-rate-limit", message = "RequestsPerMinute must be greater than zero." });
            }

            var metadata = request.Metadata is null
                ? null
                : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);

            var unsignedAllowedGlobally = configuration.GetValue<bool?>("Croniq:Webhooks:Security:AllowUnsignedHooks") ?? false;
            if (!request.RequireSignature && (!unsignedAllowedGlobally || !allowUnsigned))
            {
                return Results.BadRequest(new { error = "unsigned-hooks-disallowed", message = "Signature validation can only be disabled when Croniq:Webhooks:Security:AllowUnsignedHooks=true and the allowUnsigned query flag is set." });
            }

            var upsert = new WebhookEndpointUpsert(
                request.HookKey,
                request.JobKey,
                tenantId,
                environment,
                request.Enabled,
                request.RequireSignature,
                rpm,
                request.Secret,
                request.SignatureVersion,
                metadata);

            try
            {
                await webhookStore.UpsertAsync(upsert, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "failed to upsert webhook {HookKey}", request.HookKey);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "webhook-upsert-failed", detail: ex.Message);
            }

            var persisted = await webhookStore.FindByHookKeyAsync(request.HookKey, cancellationToken).ConfigureAwait(false);
            if (persisted is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "webhook-not-persisted", detail: "Webhook endpoint could not be read after upsert.");
            }

            var response = ToWebhookResponse(persisted, request.Secret);
            return Results.Ok(response);
        })
        .WithDocs("Webhooks_Upsert", "Create or update a webhook", "Registers a webhook endpoint for a tenant/environment, optionally overriding rate limits and signatures.");

        app.MapDelete("/tenants/{tenantId}/webhooks/{hookKey}", async (
            string tenantId,
            string hookKey,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var scope = new PartitionScope(tenantId, environment);
            await webhookStore.DeleteAsync(hookKey, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Webhooks_Delete", "Delete a webhook", "Removes a webhook endpoint and its metadata for the tenant/environment scope.");

        app.MapPost("/tenants/{tenantId}/webhooks/{hookKey}/rotate-secret", async (
            string tenantId,
            string hookKey,
            string environment,
            RotateWebhookSecretRequest request,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksRotate);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var caller = callerContextAccessor.Current;
            var rotatedBy = caller is null
                ? "cronq.api"
                : $"{caller.CallerType}:{caller.CallerId}";

            var rotate = new WebhookSecretRotate(
                hookKey,
                tenantId,
                environment,
                request.ActivateInSeconds,
                request.GracePeriodSeconds,
                rotatedBy,
                request.Notes);

            try
            {
                var result = await webhookStore.RotateSecretAsync(rotate, cancellationToken).ConfigureAwait(false);
                var response = new RotateWebhookSecretResponse(
                    result.HookKey,
                    result.ActivatedAtUtc,
                    result.ExpiresAtUtc,
                    result.Secret,
                    result.SecretHash);
                return Results.Ok(response);
            }
            catch (Exception ex)
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "secret-rotation-failed", detail: ex.Message);
            }
        })
        .WithDocs("Webhooks_RotateSecret", "Rotate webhook secret", "Schedules or immediately rotates a webhook secret and returns the new plaintext.");

        app.MapGet("/tenants/{tenantId}/webhooks/{hookKey}/ip-rules", async (
            string tenantId,
            string hookKey,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var scope = new PartitionScope(tenantId, environment);
            var rules = await webhookStore.ListIpRulesAsync(hookKey, scope, cancellationToken).ConfigureAwait(false);
            var payload = rules.Select(ToWebhookIpRuleResponse).ToList();
            return Results.Ok(payload);
        })
        .WithDocs("WebhookIpRules_List", "List webhook IP rules", "Returns the CIDR allow-list associated with a webhook endpoint.");

        app.MapPost("/tenants/{tenantId}/webhooks/{hookKey}/ip-rules", async (
            string tenantId,
            string hookKey,
            string environment,
            CreateWebhookIpRuleRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            if (!IpNetwork.TryParse(request.Cidr, out var network, out var error))
            {
                return Results.BadRequest(new { error = "invalid-cidr", message = $"CIDR '{request.Cidr}' is invalid ({error})." });
            }

            var createdBy = ResolveCallerIdentity(callerContextAccessor);
            var correlationId = ResolveCorrelationId(httpContext);

            var create = new WebhookIpRuleCreate(
                hookKey,
                tenantId,
                environment,
                network!.ToString(),
                request.Description,
                createdBy,
                correlationId);

            try
            {
                var result = await webhookStore.AddIpRuleAsync(create, cancellationToken).ConfigureAwait(false);
                return Results.Ok(ToWebhookIpRuleResponse(result));
            }
            catch (Exception ex)
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "ip-rule-create-failed", detail: ex.Message);
            }
        })
        .WithDocs("WebhookIpRules_Create", "Add webhook IP rule", "Adds a CIDR block to the allow-list for the webhook endpoint.");

        app.MapDelete("/tenants/{tenantId}/webhooks/{hookKey}/ip-rules/{ruleId:long}", async (
            string tenantId,
            string hookKey,
            long ruleId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            _ = hookKey ?? throw new ArgumentNullException(nameof(hookKey));

            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var scope = new PartitionScope(tenantId, environment);
            var deletedBy = ResolveCallerIdentity(callerContextAccessor);
            var correlationId = ResolveCorrelationId(httpContext);
            await webhookStore.DeleteIpRuleAsync(ruleId, scope, deletedBy, correlationId, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("WebhookIpRules_Delete", "Delete webhook IP rule", "Removes a CIDR allow-list entry from the webhook endpoint.");

        app.MapGet("/tenants/{tenantId}/webhooks/deadletters", async (
            string tenantId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookDeadLetterStore? deadLetterStore,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksDeadLetter);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-deadletter-unavailable", detail: "Webhook dead-letter store not configured.");
            }

            var scope = new PartitionScope(tenantId, environment);
            var entries = await deadLetterStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);
            var response = entries.Select(ToWebhookDeadLetterResponse).ToList();
            return Results.Ok(response);
        })
        .WithDocs("WebhookDeadLetters_List", "List webhook dead letters", "Enumerates failed webhook deliveries for investigation or replay.");

        app.MapPost("/tenants/{tenantId}/webhooks/deadletters/{deadLetterId}/replay", async (
            string tenantId,
            long deadLetterId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookDeadLetterStore? deadLetterStore,
            [FromServices] IJobRegistry registry,
            [FromServices] IJobExecutionPipeline pipeline,
            [FromServices] IPolicyResolver policyResolver,
            [FromServices] ILogger<WebhookDeadLetterApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.WebhooksDeadLetter);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-deadletter-unavailable", detail: "Webhook dead-letter store not configured.");
            }

            var scope = new PartitionScope(tenantId, environment);
            var entry = await deadLetterStore.FindAsync(deadLetterId, scope, cancellationToken).ConfigureAwait(false);
            if (entry is null)
            {
                return Results.NotFound(new { error = "deadletter-not-found", id = deadLetterId });
            }

            if (!JobKey.TryParse(entry.JobKey, out var jobKey))
            {
                await deadLetterStore.RecordFailureAsync(deadLetterId, scope, new WebhookDeadLetterFailure("invalid-job-key", StatusCodes.Status500InternalServerError, "Stored job key is invalid.", null), cancellationToken).ConfigureAwait(false);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "invalid-job-key", detail: "Stored job key is invalid.");
            }

            if (!registry.TryGet(jobKey, out var descriptor))
            {
                await deadLetterStore.RecordFailureAsync(deadLetterId, scope, new WebhookDeadLetterFailure("job-not-registered", StatusCodes.Status404NotFound, "Job not registered", null), cancellationToken).ConfigureAwait(false);
                return Results.Problem(statusCode: StatusCodes.Status404NotFound, title: "job-not-registered", detail: "Job not registered for this webhook.");
            }

            var metadata = entry.Metadata is null
                ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                : new Dictionary<string, string>(entry.Metadata, StringComparer.OrdinalIgnoreCase);

            if (!string.IsNullOrWhiteSpace(entry.Payload) && !metadata.ContainsKey("webhook:payload"))
            {
                metadata["webhook:payload"] = entry.Payload;
            }

            metadata["webhook:deadletter:id"] = entry.Id.ToString(CultureInfo.InvariantCulture);
            metadata["webhook:deadletter:attempts"] = entry.Attempts.ToString(CultureInfo.InvariantCulture);
            metadata["webhook:deadletter:replay_at"] = DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture);

            var executionOptions = policyResolver.ResolveExecution(jobKey);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, descriptor, executionOptions, metadata, TriggerActivitySource);

            using var replayActivity = TriggerActivitySource.StartActivity("Croniq.Api.WebhookReplay", ActivityKind.Server);
            replayActivity?.SetTag("croniq.webhook.deadletter", entry.Id);
            replayActivity?.SetTag("croniq.webhook.key", entry.HookKey);
            replayActivity?.SetTag("croniq.job.key", jobKey.Value);

            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                await deadLetterStore.ResolveAsync(deadLetterId, scope, cancellationToken).ConfigureAwait(false);
                replayActivity?.SetStatus(ActivityStatusCode.Ok);
                return Results.Ok(new { status = "replayed", hook = entry.HookKey, job = entry.JobKey });
            }
            catch (Exception ex)
            {
                replayActivity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                logger.LogError(ex, "failed to replay webhook deadletter {DeadLetterId}", deadLetterId);
                await deadLetterStore.RecordFailureAsync(deadLetterId, scope, new WebhookDeadLetterFailure("execution-error", StatusCodes.Status500InternalServerError, ex.Message, null), cancellationToken).ConfigureAwait(false);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "replay-failed", detail: ex.Message);
            }
        })
        .WithDocs("WebhookDeadLetters_Replay", "Replay webhook dead letter", "Re-dispatches a failed webhook payload via the job execution pipeline.");

        app.MapGet("/tenants/{tenantId}/api-clients", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var clients = await apiKeyStore.ListClientsAsync(tenantId, environment, cancellationToken).ConfigureAwait(false);
            var payload = clients.Select(ToApiClientResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("ApiClients_List", "List API clients", "Returns all registered API clients for the tenant, optionally filtered by environment.");

        app.MapPost("/tenants/{tenantId}/api-clients", async (
            string tenantId,
            string? environment,
            UpsertApiClientRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(request.ClientId))
            {
                return Results.BadRequest(new { error = "client-required", message = "ClientId is required." });
            }

            if (!string.IsNullOrWhiteSpace(environment)
                && !string.IsNullOrWhiteSpace(request.EnvironmentTag)
                && !string.Equals(environment, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "environment-mismatch", message = "Body environmentTag must match the query parameter value." });
            }

            var effectiveEnvironment = request.EnvironmentTag ?? environment;
            var scopes = NormalizeScopes(request.Scopes);
            var isActive = request.IsActive ?? true;

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, effectiveEnvironment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var upsert = new ApiClientUpsertRequest(
                tenantId,
                request.ClientId,
                request.Name,
                effectiveEnvironment,
                scopes,
                isActive);

            var descriptor = await apiKeyStore.UpsertClientAsync(upsert, cancellationToken).ConfigureAwait(false);
            return Results.Ok(ToApiClientResponse(descriptor));
        })
        .WithDocs("ApiClients_Upsert", "Create or update API client", "Creates a tenant-scoped API client or updates metadata/scopes when the client already exists.");

        app.MapDelete("/tenants/{tenantId}/api-clients/{clientId}", async (
            string tenantId,
            string clientId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var deleted = await apiKeyStore.DeleteClientAsync(tenantId, clientId, cancellationToken).ConfigureAwait(false);
            if (!deleted)
            {
                return Results.NotFound(new { error = "api-client-not-found", clientId });
            }

            return Results.NoContent();
        })
        .WithDocs("ApiClients_Delete", "Delete API client", "Deletes the API client metadata and revokes any associated API keys.");

        app.MapGet("/tenants/{tenantId}/api-clients/{clientId}", async (
            string tenantId,
            string clientId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var client = await apiKeyStore.GetClientAsync(tenantId, clientId, cancellationToken).ConfigureAwait(false);
            if (client is null)
            {
                return Results.NotFound(new { error = "api-client-not-found", clientId });
            }

            return Results.Ok(ToApiClientResponse(client));
        })
        .WithDocs("ApiClients_Get", "Get API client", "Returns metadata about a tenant-scoped API client, including scopes and activity flags.");

        app.MapPost("/tenants/{tenantId}/api-keys", async (
            string tenantId,
            IssueApiKeyRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, request.EnvironmentTag, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (string.IsNullOrWhiteSpace(request.ClientId))
            {
                return Results.BadRequest(new { error = "client-required", message = "ClientId is required." });
            }

            if (request.TtlHours.HasValue && request.TtlHours.Value <= 0)
            {
                return Results.BadRequest(new { error = "invalid-ttl", message = "TtlHours must be greater than zero." });
            }

            var scopes = NormalizeScopes(request.Scopes);
            TimeSpan? ttl = request.TtlHours.HasValue ? TimeSpan.FromHours(request.TtlHours.Value) : null;
            var issueRequest = new ApiKeyIssueRequest(tenantId, request.ClientId, request.EnvironmentTag, scopes, ttl);

            try
            {
                var result = await apiKeyStore.IssueAsync(issueRequest, cancellationToken).ConfigureAwait(false);
                return Results.Ok(ToIssueApiKeyResponse(result));
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "failed to issue api key for tenant {TenantId} client {ClientId}", tenantId, request.ClientId);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "api-key-issue-failed", detail: ex.Message);
            }
        })
        .WithDocs("ApiKeys_Issue", "Issue API key", "Creates a new API key for the specified tenant client and returns the plaintext once.");

        app.MapPost("/tenants/{tenantId}/api-keys/{keyId}/rotate", async (
            string tenantId,
            string keyId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            try
            {
                var result = await apiKeyStore.RotateAsync(tenantId, keyId, cancellationToken).ConfigureAwait(false);
                if (result is null)
                {
                    return Results.NotFound(new { error = "api-key-not-found", keyId });
                }

                return Results.Ok(ToIssueApiKeyResponse(result));
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "failed to rotate api key {KeyId} for tenant {TenantId}", keyId, tenantId);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "api-key-rotation-failed", detail: ex.Message);
            }
        })
        .WithDocs("ApiKeys_Rotate", "Rotate API key", "Revokes an existing API key and returns a fresh secret for the same client.");

        app.MapDelete("/tenants/{tenantId}/api-keys/{keyId}", async (
            string tenantId,
            string keyId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var revoked = await apiKeyStore.RevokeAsync(tenantId, keyId, cancellationToken).ConfigureAwait(false);
            if (!revoked)
            {
                return Results.NotFound(new { error = "api-key-not-found", keyId });
            }

            return Results.NoContent();
        })
        .WithDocs("ApiKeys_Delete", "Revoke API key", "Immediately revokes an API key for the tenant.");

        app.MapPost("/tenants/{tenantId}/tokens", async (
            string tenantId,
            string? environment,
            IssueTokenRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ICroniqTokenIssuer tokenIssuer,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            return await IssueTokenAsync(
                    tenantId,
                    environment,
                    routeClientId: null,
                    request,
                    callerContextAccessor,
                    apiKeyStore,
                    tokenIssuer,
                    logger,
                    cancellationToken)
                .ConfigureAwait(false);
        })
        .WithDocs("Tokens_Issue_Tenant", "Issue tenant token", "Mints a Croniq-signed bearer token for the specified client (tenant-level variant).");

        app.MapPost("/tenants/{tenantId}/api-clients/{clientId}/tokens", async (
            string tenantId,
            string clientId,
            string? environment,
            IssueTokenRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ICroniqTokenIssuer tokenIssuer,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            return await IssueTokenAsync(
                    tenantId,
                    environment,
                    clientId,
                    request,
                    callerContextAccessor,
                    apiKeyStore,
                    tokenIssuer,
                    logger,
                    cancellationToken)
                .ConfigureAwait(false);
        })
        .WithDocs("Tokens_Issue_Client", "Issue client token", "Same payload as the tenant route but infers the clientId from the path.");

        app.MapGet("/tenants/{tenantId}/executions/{executionId}/logs", async (
            string tenantId,
            string executionId,
            [FromServices] IExecutionLogReader reader,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            await using var enumerator = reader.ReadLinesAsync(executionId, cancellationToken).GetAsyncEnumerator(cancellationToken);
            if (!await enumerator.MoveNextAsync().ConfigureAwait(false))
            {
                await Results.NotFound(new { error = "execution-logs-not-found", executionId })
                    .ExecuteAsync(httpContext)
                    .ConfigureAwait(false);
                return;
            }

            var firstLine = enumerator.Current;
            if (!TryExtractExecutionScope(firstLine, out var logTenantId, out var environmentTag))
            {
                await Results.Problem(
                        statusCode: StatusCodes.Status500InternalServerError,
                        title: "execution-log-invalid",
                        detail: "Execution log entry missing tenant/environment metadata.")
                    .ExecuteAsync(httpContext)
                    .ConfigureAwait(false);
                return;
            }

            if (!string.Equals(logTenantId, tenantId, StringComparison.OrdinalIgnoreCase))
            {
                await Results.StatusCode(StatusCodes.Status403Forbidden)
                    .ExecuteAsync(httpContext)
                    .ConfigureAwait(false);
                return;
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, logTenantId!, environmentTag, Array.Empty<string>());
            if (authFailure is not null)
            {
                await authFailure.ExecuteAsync(httpContext).ConfigureAwait(false);
                return;
            }

            var response = httpContext.Response;
            response.ContentType = "application/x-ndjson";
            await response.WriteAsync(firstLine, cancellationToken).ConfigureAwait(false);
            await response.WriteAsync("\n", cancellationToken).ConfigureAwait(false);

            while (await enumerator.MoveNextAsync().ConfigureAwait(false))
            {
                await response.WriteAsync(enumerator.Current, cancellationToken).ConfigureAwait(false);
                await response.WriteAsync("\n", cancellationToken).ConfigureAwait(false);
            }
        })
        .WithDocs("Executions_GetLogs", "Stream execution logs", "Streams NDJSON execution logs for a tenant-scoped execution after authorizing tenant scope.");

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

        return app;
    }

    private static readonly Lazy<Func<RouteHandlerBuilder, string, string?, RouteHandlerBuilder>?> OpenApiSummaryApplier = new(CreateOpenApiSummaryApplier);

    private static RouteHandlerBuilder WithDocs(this RouteHandlerBuilder builder, string name, string summary, string? description = null)
    {
        if (builder is null)
        {
            throw new ArgumentNullException(nameof(builder));
        }

        builder.WithName(name);

        var applier = OpenApiSummaryApplier.Value;
        if (applier is not null)
        {
            return applier(builder, summary, description);
        }

        builder.WithMetadata(new EndpointDocsMetadata(summary, description));
        return builder;
    }

    private static Func<RouteHandlerBuilder, string, string?, RouteHandlerBuilder>? CreateOpenApiSummaryApplier()
    {
        var operationType = Type.GetType("Microsoft.OpenApi.Models.OpenApiOperation, Microsoft.OpenApi");
        if (operationType is null)
        {
            return null;
        }

        var extensionsType = Type.GetType("Microsoft.AspNetCore.OpenApi.OpenApiRouteHandlerBuilderExtensions, Microsoft.AspNetCore.OpenApi");
        var withOpenApiMethod = extensionsType?
            .GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(method =>
            {
                if (method.Name != "WithOpenApi")
                {
                    return false;
                }

                var parameters = method.GetParameters();
                if (parameters.Length != 2)
                {
                    return false;
                }

                return parameters[0].ParameterType == typeof(RouteHandlerBuilder);
            });

        if (withOpenApiMethod is null)
        {
            return null;
        }

        var applyDocsTemplate = typeof(ApiHostingExtensions)
            .GetMethod(nameof(ApplyOpenApiDocs), BindingFlags.NonPublic | BindingFlags.Static);

        if (applyDocsTemplate is null)
        {
            return null;
        }

        return (builder, summary, description) =>
        {
            var operationDelegate = CreateOpenApiOperationDelegate(operationType, applyDocsTemplate, summary, description);
            var result = withOpenApiMethod.Invoke(null, new object[] { builder, operationDelegate });
            return result as RouteHandlerBuilder ?? builder;
        };
    }

    private static Delegate CreateOpenApiOperationDelegate(Type operationType, MethodInfo applyDocsTemplate, string summary, string? description)
    {
        var applyDocsMethod = applyDocsTemplate.MakeGenericMethod(operationType);
        var operationParameter = Expression.Parameter(operationType, "operation");
        var summaryConstant = Expression.Constant(summary, typeof(string));
        var descriptionConstant = Expression.Constant(description, typeof(string));
        var call = Expression.Call(applyDocsMethod, operationParameter, summaryConstant, descriptionConstant);
        var funcType = typeof(Func<,>).MakeGenericType(operationType, operationType);
        return Expression.Lambda(funcType, call, operationParameter).Compile();
    }

    private static T ApplyOpenApiDocs<T>(T operation, string summary, string? description)
    {
        if (operation is null)
        {
            return operation!;
        }

        var type = typeof(T);
        var summaryProperty = type.GetProperty("Summary");
        summaryProperty?.SetValue(operation, summary);

        if (!string.IsNullOrWhiteSpace(description))
        {
            var descriptionProperty = type.GetProperty("Description");
            descriptionProperty?.SetValue(operation, description);
        }

        return operation!;
    }

    private sealed record EndpointDocsMetadata(string Summary, string? Description);

    private static string ResolveCallerIdentity(ICallerContextAccessor accessor)
    {
        var caller = accessor?.Current;
        return caller is null ? "cronq.api" : $"{caller.CallerType}:{caller.CallerId}";
    }

    private static string ResolveCorrelationId(HttpContext httpContext)
    {
        if (httpContext is null)
        {
            return Guid.NewGuid().ToString("N");
        }

        if (httpContext.Request.Headers.TryGetValue(CorrelationHeaderName, out var values))
        {
            var candidate = values.Count > 0 ? values[0] : null;
            if (!string.IsNullOrWhiteSpace(candidate))
            {
                return candidate!;
            }
        }

        if (!string.IsNullOrWhiteSpace(httpContext.TraceIdentifier))
        {
            return httpContext.TraceIdentifier!;
        }

        return Guid.NewGuid().ToString("N");
    }

    public static IServiceCollection AddCroniqApiRateLimiter(this IServiceCollection services)
    {
        services.AddRateLimiter(options =>
        {
            options.RejectionStatusCode = StatusCodes.Status429TooManyRequests;
            options.GlobalLimiter = PartitionedRateLimiter.Create<HttpContext, string>(context =>
            {
                var configured = context.RequestServices.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
                var decider = context.RequestServices.GetRequiredService<TenantRateLimitDecider>();
                var callerAccessor = context.RequestServices.GetRequiredService<ICallerContextAccessor>();
                var caller = callerAccessor.Current;
                var key = decider.GetPartitionId(caller, context.Request.Headers["X-Croniq-Key"].FirstOrDefault() ?? "anonymous");
                var permits = decider.GetPermitLimit(caller);

                return RateLimitPartition.GetFixedWindowLimiter(key, _ => new FixedWindowRateLimiterOptions
                {
                    PermitLimit = permits,
                    Window = TimeSpan.FromMinutes(1),
                    QueueLimit = permits,
                    QueueProcessingOrder = QueueProcessingOrder.OldestFirst
                });
            });
        });
        return services;
    }

    public static WebApplication MapCroniqSchedulerGrpc(this WebApplication app)
    {
        app.MapGrpcService<SchedulerGrpcService>();
        return app;
    }

    private static bool TryExtractExecutionScope(string line, out string? tenantId, out string? environmentTag)
    {
        tenantId = null;
        environmentTag = null;

        if (string.IsNullOrWhiteSpace(line))
        {
            return false;
        }

        try
        {
            using var doc = JsonDocument.Parse(line);
            foreach (var property in doc.RootElement.EnumerateObject())
            {
                if (tenantId is null && string.Equals(property.Name, "tenantId", StringComparison.OrdinalIgnoreCase))
                {
                    tenantId = property.Value.GetString();
                }

                if (environmentTag is null
                    && (string.Equals(property.Name, "environmentTag", StringComparison.OrdinalIgnoreCase)
                        || string.Equals(property.Name, "environment", StringComparison.OrdinalIgnoreCase)))
                {
                    environmentTag = property.Value.GetString();
                }
            }

            return !string.IsNullOrWhiteSpace(tenantId);
        }
        catch (JsonException)
        {
            return false;
        }
    }

    private static IReadOnlyDictionary<string, string>? ToReadOnly(IDictionary<string, string>? source)
    {
        if (source is null) return null;
        if (source is IReadOnlyDictionary<string, string> ro) return ro;
        return new Dictionary<string, string>(source);
    }

    private static ApiClientResponse ToApiClientResponse(ApiClientDescriptor descriptor)
    {
        var scopes = descriptor.Scopes ?? Array.Empty<string>();
        return new ApiClientResponse(
            descriptor.ClientId,
            descriptor.TenantId,
            descriptor.Name,
            descriptor.EnvironmentTag,
            scopes,
            descriptor.IsActive,
            descriptor.ExpiresAt);
    }

    private static TenantResponse ToTenantResponse(TenantDescriptor descriptor)
    {
        return new TenantResponse(
            descriptor.TenantId,
            descriptor.Reference,
            descriptor.Name,
            descriptor.IsActive,
            descriptor.CreatedAt);
    }

    private static CallerInfoResponse ToCallerInfoResponse(ICallerContext caller)
    {
        return new CallerInfoResponse(
            caller.TenantId,
            caller.EnvironmentTag,
            caller.CallerId,
            caller.CallerType,
            caller.Scopes,
            caller.IsActive);
    }

    private static IssueApiKeyResponse ToIssueApiKeyResponse(ApiKeyIssueResult result)
    {
        return new IssueApiKeyResponse(
            result.ClientId,
            result.TenantId,
            result.KeyId,
            result.PlaintextSecret,
            result.ExpiresAt,
            result.EnvironmentTag);
    }

    private static ScheduleResponse ToScheduleResponse(TriggerDefinition definition)
    {
        IReadOnlyDictionary<string, string>? metadata = definition.Metadata is null
            ? null
            : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

        return new ScheduleResponse(
            definition.TriggerId,
            definition.JobKey,
            definition.ScheduleExpression,
            definition.Scope.TenantId,
            definition.Scope.EnvironmentTag,
            definition.StartAtUtc,
            definition.EndAtUtc,
            definition.Enabled,
            metadata);
    }

    private static ExecutionResponse ToExecutionResponse(ExecutionSummary summary)
    {
        return new ExecutionResponse(
            summary.ExecutionId,
            summary.JobKey,
            summary.TenantId,
            summary.EnvironmentTag,
            summary.Kind,
            summary.Status,
            summary.FireAtUtc,
            summary.StartedAtUtc,
            summary.CompletedAtUtc,
            summary.DurationMs,
            summary.TriggerId,
            summary.InstanceId,
            summary.TraceId,
            summary.CorrelationId,
            summary.ErrorType,
            summary.ErrorMessage);
    }

    private static JobResponse ToJobResponse(JobDefinition job)
    {
        IReadOnlyDictionary<string, string>? metadata = job.Metadata is null
            ? null
            : new Dictionary<string, string>(job.Metadata, StringComparer.OrdinalIgnoreCase);

        return new JobResponse(
            job.JobKey,
            job.Namespace,
            job.Name,
            job.Variant,
            job.Description,
            metadata);
    }

    private static IReadOnlyCollection<string> NormalizeScopes(IReadOnlyCollection<string>? requestedScopes)
    {
        if (requestedScopes is null || requestedScopes.Count == 0)
        {
            return Array.Empty<string>();
        }

        var normalized = requestedScopes
            .Where(scope => !string.IsNullOrWhiteSpace(scope))
            .Select(scope => scope.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return normalized.Length == 0 ? Array.Empty<string>() : normalized;
    }

    private static bool AreScopesAllowed(IReadOnlyCollection<string> requested, IReadOnlyCollection<string> allowed)
    {
        if (requested.Count == 0)
        {
            return true;
        }

        if (allowed.Count == 0)
        {
            return false;
        }

        var permitted = new HashSet<string>(allowed, StringComparer.OrdinalIgnoreCase);
        return requested.All(permitted.Contains);
    }

    private static async Task<IResult> IssueTokenAsync(
        string tenantId,
        string? environment,
        string? routeClientId,
        IssueTokenRequest request,
        ICallerContextAccessor callerContextAccessor,
        IApiKeyStore apiKeyStore,
        ICroniqTokenIssuer tokenIssuer,
        ILogger<ApiKeyAdminApiMarker> logger,
        CancellationToken cancellationToken)
    {
        var clientId = routeClientId ?? request.ClientId;
        if (string.IsNullOrWhiteSpace(clientId))
        {
            return Results.BadRequest(new { error = "client-required", message = "ClientId is required." });
        }

        if (request.TtlMinutes.HasValue && request.TtlMinutes.Value <= 0)
        {
            return Results.BadRequest(new { error = "invalid-ttl", message = "TtlMinutes must be greater than zero." });
        }

        var client = await apiKeyStore.GetClientAsync(tenantId, clientId, cancellationToken).ConfigureAwait(false);
        if (client is null)
        {
            return Results.NotFound(new { error = "api-client-not-found", clientId });
        }

        if (!client.IsActive)
        {
            return Results.BadRequest(new { error = "client-inactive", message = "Inactive API clients cannot issue tokens." });
        }

        if (!string.IsNullOrWhiteSpace(environment)
            && !string.IsNullOrWhiteSpace(client.EnvironmentTag)
            && !string.Equals(client.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
        {
            return Results.BadRequest(new { error = "environment-mismatch", message = "Client environment does not match the requested environment." });
        }

        var guardEnvironment = environment ?? client.EnvironmentTag;
        var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, guardEnvironment, CroniqScopes.ApiKeysManage);
        if (authFailure is not null)
        {
            return authFailure;
        }

        var allowedScopes = client.Scopes ?? Array.Empty<string>();
        var requestedScopes = NormalizeScopes(request.Scopes);
        var tokenScopes = requestedScopes.Count == 0 ? allowedScopes : requestedScopes;
        if (tokenScopes.Count == 0)
        {
            return Results.BadRequest(new { error = "missing-scopes", message = "Assign scopes to the client before issuing tokens." });
        }

        if (!AreScopesAllowed(tokenScopes, allowedScopes))
        {
            return Results.BadRequest(new { error = "invalid-scopes", message = "Requested scopes must be a subset of the client scopes." });
        }

        TimeSpan? lifetime = null;
        if (request.TtlMinutes.HasValue)
        {
            lifetime = TimeSpan.FromMinutes(request.TtlMinutes.Value);
        }

        try
        {
            var token = await tokenIssuer.IssueAsync(new CroniqTokenIssueRequest(
                tenantId,
                clientId,
                guardEnvironment,
                tokenScopes,
                request.Audience,
                lifetime), cancellationToken).ConfigureAwait(false);

            return Results.Ok(new IssueTokenResponse(token.AccessToken, token.TokenType, token.ExpiresInSeconds));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "failed to issue token for tenant {TenantId} client {ClientId}", tenantId, clientId);
            return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "token-issue-failed", detail: ex.Message);
        }
    }

    private static WebhookEndpointResponse ToWebhookResponse(WebhookEndpointDefinition definition, string? secretOverride = null)
    {
        IDictionary<string, string>? metadata = definition.Metadata is null
            ? null
            : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

        var ipRules = definition.IpRules.Count == 0
            ? Array.Empty<WebhookIpRuleResponse>()
            : definition.IpRules.Select(ToWebhookIpRuleResponse).ToArray();

        return new WebhookEndpointResponse(
            definition.HookKey,
            definition.JobKey,
            definition.Enabled,
            definition.RequireSignature,
            definition.RequestsPerMinute,
            metadata,
            ipRules,
            definition.CreatedAtUtc,
            definition.UpdatedAtUtc,
            secretOverride);
    }

    private static WebhookIpRuleResponse ToWebhookIpRuleResponse(WebhookIpRuleDefinition definition)
    {
        return new WebhookIpRuleResponse(
            definition.Id,
            definition.Cidr,
            definition.Description,
            definition.CreatedBy,
            definition.CreatedAtUtc,
            definition.UpdatedAtUtc);
    }

    private static WebhookDeadLetterResponse ToWebhookDeadLetterResponse(WebhookDeadLetterEntry entry)
    {
        IDictionary<string, string>? headers = entry.Headers is null
            ? null
            : new Dictionary<string, string>(entry.Headers, StringComparer.OrdinalIgnoreCase);

        IDictionary<string, string>? metadata = entry.Metadata is null
            ? null
            : new Dictionary<string, string>(entry.Metadata, StringComparer.OrdinalIgnoreCase);

        return new WebhookDeadLetterResponse(
            entry.Id,
            entry.HookKey,
            entry.JobKey,
            entry.TenantId,
            entry.EnvironmentTag,
            entry.Payload,
            headers,
            metadata,
            entry.FailureReason,
            entry.Attempts,
            entry.StatusCode,
            entry.ErrorDetails,
            entry.CreatedAtUtc,
            entry.LastAttemptAtUtc,
            entry.NextAttemptAtUtc,
            entry.ExpiresAtUtc);
    }

    private sealed class WebhookEndpointApiMarker
    {
    }

    private sealed class WebhookDeadLetterApiMarker
    {
    }

    private sealed class ApiKeyAdminApiMarker
    {
    }
}
