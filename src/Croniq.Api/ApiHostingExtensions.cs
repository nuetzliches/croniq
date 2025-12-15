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

            var response = new ApiClientResponse(
                client.ClientId,
                client.TenantId,
                client.Name,
                client.EnvironmentTag,
                client.Scopes,
                client.IsActive,
                client.ExpiresAt);

            return Results.Ok(response);
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
