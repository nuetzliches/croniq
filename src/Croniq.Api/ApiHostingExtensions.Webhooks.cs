using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Core.Security;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapWebhookEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

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
    }
}
