using System.Diagnostics;
using System.Globalization;
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
using Croniq.Data.SqlServer;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Providers.Default;
using Croniq.Sdk;
using Croniq.Hosting;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static class ApiHostingExtensions
{
    private static readonly ActivitySource TriggerActivitySource = new("Croniq.Api.Trigger");

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

        app.Use(async (context, next) =>
        {
            if (context.Request.Path.StartsWithSegments("/health", StringComparison.OrdinalIgnoreCase))
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

        app.MapGet("/health", () => Results.Ok(new { status = "ok" }));
        app.MapGet("/health/persistence", async (IServiceProvider sp, CancellationToken ct) =>
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
        });

        app.MapPost("/schedules", async (
            UpsertScheduleRequest request,
            IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(request.JobKey) || string.IsNullOrWhiteSpace(request.CronExpression))
            {
                return Results.BadRequest(new { error = "invalid-request", message = "JobKey and CronExpression are required." });
            }

            var parts = ParseJobKey(request.JobKey);
            var triggerId = string.IsNullOrWhiteSpace(request.TriggerId)
                ? $"{request.JobKey}:{request.CronExpression}"
                : request.TriggerId;

            var scope = new PartitionScope(parts.TenantId, parts.EnvironmentTag);

            var metadata = ToReadOnly(request.Metadata);
            var job = new JobDefinition(
                request.JobKey,
                parts.NamespaceSegment,
                parts.JobName,
                parts.Variant,
                request.Description,
                metadata);

            var trigger = new TriggerDefinition(
                triggerId,
                request.JobKey,
                request.CronExpression,
                scope,
                request.StartAtUtc,
                request.EndAtUtc,
                request.Enabled,
                metadata);

            await store.UpsertJobAsync(job, cancellationToken).ConfigureAwait(false);
            await store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);
            ApiMetrics.RecordScheduleUpsert(parts.TenantId, parts.EnvironmentTag, request.JobKey);

            return Results.Created($"/schedules/{trigger.TriggerId}", new { trigger.TriggerId, trigger.JobKey, trigger.ScheduleExpression });
        });

        app.MapGet("/tenants/{tenantId}/webhooks", async (
            string tenantId,
            string environment,
            ICallerContextAccessor callerContextAccessor,
            IWebhookPersistenceProvider? webhookStore,
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
        });

        app.MapPost("/tenants/{tenantId}/webhooks", async (
            string tenantId,
            string environment,
            bool allowUnsigned,
            UpsertWebhookEndpointRequest request,
            ICallerContextAccessor callerContextAccessor,
            IWebhookPersistenceProvider? webhookStore,
            IConfiguration configuration,
            ILogger<WebhookEndpointApiMarker> logger,
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
        });

        app.MapDelete("/tenants/{tenantId}/webhooks/{hookKey}", async (
            string tenantId,
            string hookKey,
            string environment,
            ICallerContextAccessor callerContextAccessor,
            IWebhookPersistenceProvider? webhookStore,
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
        });

        app.MapPost("/tenants/{tenantId}/webhooks/{hookKey}/rotate-secret", async (
            string tenantId,
            string hookKey,
            string environment,
            RotateWebhookSecretRequest request,
            IWebhookPersistenceProvider? webhookStore,
            ICallerContextAccessor callerContextAccessor,
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
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured." );
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
        });

        app.MapGet("/tenants/{tenantId}/webhooks/deadletters", async (
            string tenantId,
            string environment,
            ICallerContextAccessor callerContextAccessor,
            IWebhookDeadLetterStore? deadLetterStore,
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
        });

        app.MapPost("/tenants/{tenantId}/webhooks/deadletters/{deadLetterId}/replay", async (
            string tenantId,
            long deadLetterId,
            string environment,
            ICallerContextAccessor callerContextAccessor,
            IWebhookDeadLetterStore? deadLetterStore,
            IJobRegistry registry,
            IJobExecutionPipeline pipeline,
            IPolicyResolver policyResolver,
            ILogger<WebhookDeadLetterApiMarker> logger,
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
            var execRequest = new JobExecutionRequest(jobKey, descriptor, executionOptions, metadata, TriggerActivitySource);

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
        });

        app.MapPost("/jobs/trigger", async (
            TriggerJobRequest request,
            IJobRegistry registry,
            IJobExecutionPipeline pipeline,
            IPolicyResolver policyResolver,
            CancellationToken cancellationToken) =>
        {
            if (!JobKey.TryParse(request.JobKey, out var jobKey) || !registry.TryGet(jobKey, out var descriptor))
            {
                return Results.NotFound(new { error = "job-not-registered", request.JobKey });
            }

            var metadata = ToReadOnly(request.Metadata) ?? new Dictionary<string, string>();
            var executionOptions = policyResolver.ResolveExecution(jobKey);
            var execRequest = new JobExecutionRequest(jobKey, descriptor, executionOptions, metadata, TriggerActivitySource);

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
        });

        return app;
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


    private static (string TenantId, string EnvironmentTag, string NamespaceSegment, string JobName, string? Variant) ParseJobKey(string jobKey)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new ArgumentException($"Invalid JobKey format: {jobKey}", nameof(jobKey));
        }

        return (parsed.TenantId, parsed.EnvironmentTag, parsed.NamespaceSegment, parsed.JobName, parsed.Variant);
    }

    private static IReadOnlyDictionary<string, string>? ToReadOnly(IDictionary<string, string>? source)
    {
        if (source is null) return null;
        if (source is IReadOnlyDictionary<string, string> ro) return ro;
        return new Dictionary<string, string>(source);
    }

    private static WebhookEndpointResponse ToWebhookResponse(WebhookEndpointDefinition definition, string? secretOverride = null)
    {
        IDictionary<string, string>? metadata = definition.Metadata is null
            ? null
            : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

        return new WebhookEndpointResponse(
            definition.HookKey,
            definition.JobKey,
            definition.Enabled,
            definition.RequireSignature,
            definition.RequestsPerMinute,
            metadata,
            definition.CreatedAtUtc,
            definition.UpdatedAtUtc,
            secretOverride);
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
}
