using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using Croniq.Api.Models;
using ApiWebhookActivityBucket = Croniq.Api.Models.WebhookActivityBucket;
using ApiWebhookActivitySummary = Croniq.Api.Models.WebhookActivitySummary;
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
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static readonly JsonSerializerOptions WebhookActivityStreamJsonOptions = new(JsonSerializerDefaults.Web);
    private static readonly TimeSpan WebhookActivityStreamPollInterval = TimeSpan.FromSeconds(5);

    private static void MapWebhookEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        var webhooksMode = app.Configuration.GetValue<string?>("Croniq:Webhooks:Mode") ?? string.Empty;
        if (string.Equals(webhooksMode, "Remote", StringComparison.OrdinalIgnoreCase))
        {
            app.MapPost("/tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}", async (
                string tenantId,
                string environmentTag,
                string hookKey,
                HttpRequest request,
                [FromServices] IConfiguration configuration,
                [FromServices] IHttpClientFactory httpClientFactory,
                [FromServices] IHostEnvironment hostEnvironment,
                [FromServices] ILogger<WebhookEndpointApiMarker> logger,
                CancellationToken cancellationToken) =>
            {
                if (string.IsNullOrWhiteSpace(environmentTag))
                {
                    return MissingEnvironment("environmentTag");
                }

                var remoteBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:BaseUrl") ?? string.Empty;
                var remoteIngressBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:IngressBaseUrl");
                var resolvedIngressBaseUrl = string.IsNullOrWhiteSpace(remoteIngressBaseUrl)
                    ? remoteBaseUrl
                    : remoteIngressBaseUrl;
                var remoteApiKey = configuration.GetValue<string?>("Croniq:Webhooks:Remote:ApiKey") ?? string.Empty;

                if (string.IsNullOrWhiteSpace(resolvedIngressBaseUrl))
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-remote-unavailable", detail: "Remote webhook relay is not configured.");
                }

                if (!Uri.TryCreate(resolvedIngressBaseUrl, UriKind.Absolute, out var remoteUri))
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-remote-unavailable", detail: "Remote webhook relay ingress base URL is invalid.");
                }

                var allowInvalidCertificate = configuration.GetValue<bool?>("Croniq:Webhooks:Remote:AllowInvalidServerCertificate") ?? false;
                if (allowInvalidCertificate && !hostEnvironment.IsDevelopment())
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-remote-unavailable", detail: "Croniq:Webhooks:Remote:AllowInvalidServerCertificate is only supported in Development.");
                }

                var client = allowInvalidCertificate
                    ? new HttpClient(new HttpClientHandler
                    {
                        ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator
                    }, disposeHandler: true)
                    : httpClientFactory.CreateClient();

                client.BaseAddress = remoteUri;
                if (!string.IsNullOrWhiteSpace(remoteApiKey))
                {
                    client.DefaultRequestHeaders.Remove("X-Croniq-Relay-Key");
                    client.DefaultRequestHeaders.Add("X-Croniq-Relay-Key", remoteApiKey);
                }

                var path = $"tenants/{Uri.EscapeDataString(tenantId)}/environments/{Uri.EscapeDataString(environmentTag)}/webhooks/{Uri.EscapeDataString(hookKey)}{request.QueryString.Value}";
                try
                {
                    await using var payloadStream = new MemoryStream();
                    await request.Body.CopyToAsync(payloadStream, cancellationToken).ConfigureAwait(false);
                    payloadStream.Position = 0;

                    using var content = new StreamContent(payloadStream);
                    if (!string.IsNullOrWhiteSpace(request.ContentType))
                    {
                        content.Headers.TryAddWithoutValidation("Content-Type", request.ContentType);
                    }

                    var forward = new HttpRequestMessage(HttpMethod.Post, path)
                    {
                        Content = content
                    };

                    foreach (var header in request.Headers)
                    {
                        if (string.Equals(header.Key, "Host", StringComparison.OrdinalIgnoreCase)
                            || string.Equals(header.Key, "Content-Length", StringComparison.OrdinalIgnoreCase))
                        {
                            continue;
                        }

                        if (!forward.Headers.TryAddWithoutValidation(header.Key, header.Value.ToArray()))
                        {
                            forward.Content?.Headers.TryAddWithoutValidation(header.Key, header.Value.ToArray());
                        }
                    }

                    using var response = await client.SendAsync(forward, cancellationToken).ConfigureAwait(false);
                    var responseBody = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);

                    if (!response.IsSuccessStatusCode)
                    {
                        logger.LogWarning(
                            "remote webhook ingress relay failed for {HookKey} ({TenantId}/{EnvironmentTag}) with status {StatusCode} {ReasonPhrase}. Body: {Body}",
                            hookKey,
                            tenantId,
                            environmentTag,
                            (int)response.StatusCode,
                            response.ReasonPhrase,
                            string.IsNullOrWhiteSpace(responseBody) ? "<empty>" : responseBody);

                        var detail = string.IsNullOrWhiteSpace(responseBody)
                            ? $"Remote webhook relay failed with status {(int)response.StatusCode} ({response.ReasonPhrase})."
                            : responseBody;
                        return Results.Problem(statusCode: (int)response.StatusCode, title: "webhook-relay-failed", detail: detail);
                    }

                    if (string.IsNullOrWhiteSpace(responseBody))
                    {
                        return Results.Accepted();
                    }

                    return Results.Content(
                        responseBody,
                        contentType: response.Content.Headers.ContentType?.ToString(),
                        statusCode: (int)response.StatusCode);
                }
                catch (HttpRequestException ex)
                {
                    logger.LogError(ex, "remote webhook ingress relay failed for {HookKey}", hookKey);
                    return Results.Problem(statusCode: StatusCodes.Status502BadGateway, title: "webhook-relay-failed", detail: ex.Message);
                }
            })
            .WithDocs("Webhooks_Ingress_Relay", "Relay webhook ingress", "Relays webhook ingress to the DMZ host when Mode=Remote.")
            .Produces(StatusCodes.Status202Accepted)
            .Produces(StatusCodes.Status502BadGateway);
        }

        app.MapGet("/tenants/{tenantId}/webhooks", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            [FromServices] IWebhookActivityStore? activityStore,
            CancellationToken cancellationToken) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var endpoints = await webhookStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);
            var lastDeliveries = await TryResolveLastDeliveriesAsync(activityStore, scope, endpoints, cancellationToken)
                .ConfigureAwait(false);
            var response = endpoints
                .Select(def =>
                {
                    var endpoint = ToWebhookResponse(def);
                    if (endpoint.LastDeliveryAtUtc.HasValue || lastDeliveries.Count == 0)
                    {
                        return endpoint;
                    }

                    return lastDeliveries.TryGetValue(def.HookKey, out var lastDeliveryAtUtc)
                        ? endpoint with { LastDeliveryAtUtc = lastDeliveryAtUtc }
                        : endpoint;
                })
                .ToList();
            return Results.Ok(response);
        })
        .WithDocs("Webhooks_List", "List webhook endpoints", "Returns all webhook endpoints for the specified tenant/environment scope.")
        .Produces<List<WebhookEndpointResponse>>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapGet("/tenants/{tenantId}/webhooks/capabilities", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookCapabilitiesProvider? capabilitiesProvider,
            [FromServices] IConfiguration configuration,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            WebhookCapabilities capabilities;
            if (capabilitiesProvider is not null)
            {
                capabilities = await capabilitiesProvider
                    .GetCapabilitiesAsync(new PartitionScope(tenantId, resolvedEnvironment), cancellationToken)
                    .ConfigureAwait(false);
            }
            else
            {
                var allowUnsignedHooks = configuration.GetValue<bool?>("Croniq:Webhooks:Security:AllowUnsignedHooks") ?? false;
                var defaultRequestsPerMinute = configuration.GetValue<int?>("Croniq:Webhooks:RequestsPerMinute") ?? 60;
                capabilities = new WebhookCapabilities(allowUnsignedHooks, defaultRequestsPerMinute);
            }

            var mode = configuration.GetValue<string?>("Croniq:Webhooks:Mode") ?? "InMemory";
            mode = string.IsNullOrWhiteSpace(mode) ? "InMemory" : mode.Trim();

            string? remoteBaseUrl = null;
            string? remoteIngressBaseUrl = null;
            if (string.Equals(mode, "Remote", StringComparison.OrdinalIgnoreCase))
            {
                var rawBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:BaseUrl");
                remoteBaseUrl = string.IsNullOrWhiteSpace(rawBaseUrl) ? null : rawBaseUrl.Trim();
                var rawIngressBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:IngressBaseUrl");
                remoteIngressBaseUrl = string.IsNullOrWhiteSpace(rawIngressBaseUrl)
                    ? remoteBaseUrl
                    : rawIngressBaseUrl.Trim();
            }

            return Results.Ok(new WebhookCapabilitiesResponse(
                capabilities.AllowUnsignedHooks,
                capabilities.DefaultRequestsPerMinute,
                mode,
                remoteBaseUrl,
                remoteIngressBaseUrl));
        })
        .WithDocs(
            "Webhooks_Capabilities",
            "Get webhook capabilities",
            "Returns the webhook defaults/capabilities for the tenant/environment scope.")
        .Produces<WebhookCapabilitiesResponse>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapGet("/tenants/{tenantId}/webhooks/remote/health", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IConfiguration configuration,
            [FromServices] IHttpClientFactory httpClientFactory,
            [FromServices] IHostEnvironment hostEnvironment,
            [FromServices] ILogger<WebhookEndpointApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var mode = configuration.GetValue<string?>("Croniq:Webhooks:Mode") ?? string.Empty;
            if (!string.Equals(mode, "Remote", StringComparison.OrdinalIgnoreCase))
            {
                return Results.Ok(new WebhookRemoteHealthResponse(
                    "not-configured",
                    DateTimeOffset.UtcNow,
                    null,
                    "Webhook mode is not Remote."));
            }

            var remoteBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:BaseUrl") ?? string.Empty;
            var remoteIngressBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:IngressBaseUrl");
            var resolvedIngressBaseUrl = string.IsNullOrWhiteSpace(remoteIngressBaseUrl)
                ? remoteBaseUrl
                : remoteIngressBaseUrl;
            var remoteApiKey = configuration.GetValue<string?>("Croniq:Webhooks:Remote:ApiKey") ?? string.Empty;
            if (string.IsNullOrWhiteSpace(resolvedIngressBaseUrl) || string.IsNullOrWhiteSpace(remoteApiKey))
            {
                return Results.Ok(new WebhookRemoteHealthResponse(
                    "unavailable",
                    DateTimeOffset.UtcNow,
                    null,
                    "Remote webhook relay is not configured."));
            }

            if (!Uri.TryCreate(resolvedIngressBaseUrl, UriKind.Absolute, out var remoteUri))
            {
                return Results.Ok(new WebhookRemoteHealthResponse(
                    "unavailable",
                    DateTimeOffset.UtcNow,
                    null,
                    "Remote webhook relay ingress base URL is invalid."));
            }

            var allowInvalidCertificate = configuration.GetValue<bool?>("Croniq:Webhooks:Remote:AllowInvalidServerCertificate") ?? false;
            if (allowInvalidCertificate && !hostEnvironment.IsDevelopment())
            {
                return Results.Ok(new WebhookRemoteHealthResponse(
                    "unavailable",
                    DateTimeOffset.UtcNow,
                    null,
                    "Croniq:Webhooks:Remote:AllowInvalidServerCertificate is only supported in Development."));
            }

            HttpClient? client = null;
            try
            {
                client = allowInvalidCertificate
                    ? new HttpClient(new HttpClientHandler
                    {
                        ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator
                    }, disposeHandler: true)
                    : httpClientFactory.CreateClient();

                client.BaseAddress = remoteUri;
                client.DefaultRequestHeaders.Remove("X-Croniq-Key");
                client.DefaultRequestHeaders.Add("X-Croniq-Key", remoteApiKey);
                client.DefaultRequestHeaders.Remove("X-Croniq-Relay-Key");
                client.DefaultRequestHeaders.Add("X-Croniq-Relay-Key", remoteApiKey);

                using var response = await client.GetAsync("health", cancellationToken).ConfigureAwait(false);
                if (!response.IsSuccessStatusCode)
                {
                    var body = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
                    var detail = string.IsNullOrWhiteSpace(body)
                        ? $"Remote health probe failed with status {(int)response.StatusCode} ({response.ReasonPhrase})."
                        : body;

                    logger.LogWarning(
                        "remote webhook health check failed with status {StatusCode} {ReasonPhrase}",
                        (int)response.StatusCode,
                        response.ReasonPhrase);

                    return Results.Ok(new WebhookRemoteHealthResponse(
                        "unhealthy",
                        DateTimeOffset.UtcNow,
                        (int)response.StatusCode,
                        detail));
                }

                return Results.Ok(new WebhookRemoteHealthResponse(
                    "ok",
                    DateTimeOffset.UtcNow,
                    (int)response.StatusCode,
                    null));
            }
            catch (HttpRequestException ex)
            {
                logger.LogWarning(ex, "remote webhook health check failed");
                return Results.Ok(new WebhookRemoteHealthResponse(
                    "unreachable",
                    DateTimeOffset.UtcNow,
                    null,
                    ex.Message));
            }
            finally
            {
                if (allowInvalidCertificate)
                {
                    client?.Dispose();
                }
            }
        })
        .WithDocs(
            "Webhooks_RemoteHealth",
            "Check remote webhook health",
            "Proxies /health against the remote webhook host when Croniq:Webhooks:Mode=Remote.")
        .Produces<WebhookRemoteHealthResponse>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapPost("/tenants/{tenantId}/webhooks", async (
            string tenantId,
            string? environment,
            UpsertWebhookEndpointRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            [FromServices] IConfiguration configuration,
            [FromServices] ILogger<WebhookEndpointApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
            }

            var isRemote = string.Equals(
                configuration.GetValue<string?>("Croniq:Webhooks:Mode"),
                "Remote",
                StringComparison.OrdinalIgnoreCase);

            int rpm;
            if (isRemote)
            {
                if (request.RequestsPerMinute.HasValue && request.RequestsPerMinute.Value <= 0)
                {
                    return Results.BadRequest(new { error = "invalid-rate-limit", message = "RequestsPerMinute must be greater than zero." });
                }

                rpm = request.RequestsPerMinute ?? 0;
            }
            else
            {
                var defaultLimit = configuration.GetValue<int?>("Croniq:Webhooks:RequestsPerMinute") ?? 60;
                rpm = request.RequestsPerMinute ?? defaultLimit;
                if (rpm <= 0)
                {
                    return Results.BadRequest(new { error = "invalid-rate-limit", message = "RequestsPerMinute must be greater than zero." });
                }
            }

            var metadata = request.Metadata is null
                ? null
                : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);

            if (!request.RequireSignature && !request.AllowUnsigned)
            {
                return Results.BadRequest(new { error = "unsigned-hooks-flag-required", message = "Payload field 'allowUnsigned=true' is required when RequireSignature=false." });
            }

            if (!request.RequireSignature && !isRemote)
            {
                var unsignedAllowedGlobally = configuration.GetValue<bool?>("Croniq:Webhooks:Security:AllowUnsignedHooks") ?? false;
                if (!unsignedAllowedGlobally)
                {
                    return Results.BadRequest(new { error = "unsigned-hooks-disallowed", message = "Signature validation can only be disabled when Croniq:Webhooks:Security:AllowUnsignedHooks=true." });
                }
            }

            var upsert = new WebhookEndpointUpsert(
                request.HookKey,
                request.JobKey,
                tenantId,
                resolvedEnvironment,
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

            var persisted = await webhookStore.FindByHookKeyAsync(request.HookKey, new PartitionScope(tenantId, resolvedEnvironment), cancellationToken).ConfigureAwait(false);
            if (persisted is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "webhook-not-persisted", detail: "Webhook endpoint could not be read after upsert.");
            }

            var response = ToWebhookResponse(persisted, request.Secret);
            return Results.Ok(response);
        })
        .WithDocs("Webhooks_Upsert", "Create or update a webhook", "Registers a webhook endpoint for a tenant/environment, optionally overriding rate limits and signatures.")
        .Produces<WebhookEndpointResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status500InternalServerError)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksWrite);

        app.MapDelete("/tenants/{tenantId}/webhooks/{hookKey}", async (
            string tenantId,
            string hookKey,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            CancellationToken cancellationToken,
            bool hardDelete = false) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            await webhookStore.DeleteAsync(hookKey, scope, hardDelete, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Webhooks_Delete", "Delete a webhook", "Removes a webhook endpoint and its metadata for the tenant/environment scope.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksWrite);

        app.MapPost("/tenants/{tenantId}/webhooks/{hookKey}/rotate-secret", async (
            string tenantId,
            string hookKey,
            string? environment,
            RotateWebhookSecretRequest request,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var caller = callerContextAccessor.Current;
            var rotatedBy = caller is null
                ? "cronq.api"
                : $"{caller.CallerType}:{caller.CallerId}";

            var rotate = new WebhookSecretRotate(
                hookKey,
                tenantId,
                resolvedEnvironment,
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
        .WithDocs("Webhooks_RotateSecret", "Rotate webhook secret", "Schedules or immediately rotates a webhook secret and returns the new plaintext.")
        .Produces<RotateWebhookSecretResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status500InternalServerError)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRotate);

        app.MapGet("/tenants/{tenantId}/webhooks/{hookKey}/ip-rules", async (
            string tenantId,
            string hookKey,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            CancellationToken cancellationToken) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var rules = await webhookStore.ListIpRulesAsync(hookKey, scope, cancellationToken).ConfigureAwait(false);
            var payload = rules.Select(ToWebhookIpRuleResponse).ToList();
            return Results.Ok(payload);
        })
        .WithDocs("WebhookIpRules_List", "List webhook IP rules", "Returns the CIDR allow-list associated with a webhook endpoint.")
        .Produces<List<WebhookIpRuleResponse>>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapPost("/tenants/{tenantId}/webhooks/{hookKey}/ip-rules", async (
            string tenantId,
            string hookKey,
            string? environment,
            CreateWebhookIpRuleRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
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
                resolvedEnvironment,
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
        .WithDocs("WebhookIpRules_Create", "Add webhook IP rule", "Adds a CIDR block to the allow-list for the webhook endpoint.")
        .Produces<WebhookIpRuleResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status500InternalServerError)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksWrite);

        app.MapDelete("/tenants/{tenantId}/webhooks/{hookKey}/ip-rules/{ruleId:long}", async (
            string tenantId,
            string hookKey,
            long ruleId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            _ = hookKey ?? throw new ArgumentNullException(nameof(hookKey));

            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var deletedBy = ResolveCallerIdentity(callerContextAccessor);
            var correlationId = ResolveCorrelationId(httpContext);
            await webhookStore.DeleteIpRuleAsync(ruleId, scope, deletedBy, correlationId, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("WebhookIpRules_Delete", "Delete webhook IP rule", "Removes a CIDR allow-list entry from the webhook endpoint.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksWrite);

        app.MapGet("/tenants/{tenantId}/webhooks/{hookKey}/events", async (
            string tenantId,
            string hookKey,
            string? environment,
            long? afterId,
            int? limit,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookEndpointChangefeed? changefeed,
            CancellationToken cancellationToken) =>
        {
            if (changefeed is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-events-unavailable", detail: "Webhook endpoint changefeed not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var normalizedAfterId = afterId is > 0 ? afterId.Value : 0;
            var batchSize = Math.Clamp(limit ?? 50, 1, 200);

            var events = await changefeed.FetchAsync(normalizedAfterId, batchSize, cancellationToken).ConfigureAwait(false);
            var response = events
                .Where(entry => string.Equals(entry.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(entry.EnvironmentTag, resolvedEnvironment, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(entry.HookKey, hookKey, StringComparison.OrdinalIgnoreCase))
                .Select(entry => new WebhookEndpointEventResponse(
                    entry.Id,
                    entry.HookKey,
                    entry.EventType,
                    entry.OccurredAtUtc,
                    entry.Actor,
                    entry.CorrelationId))
                .ToList();

            return Results.Ok(response);
        })
        .WithDocs("Webhooks_Events", "List webhook endpoint events", "Returns endpoint changefeed events for a webhook in the tenant/environment scope.")
        .Produces<List<WebhookEndpointEventResponse>>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapPost("/tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}/invoke", async (
            string tenantId,
            string environmentTag,
            string hookKey,
            HttpRequest request,
            [FromServices] IWebhookPersistenceProvider? webhookStore,
            [FromServices] IConfiguration configuration,
            [FromServices] IHttpClientFactory httpClientFactory,
            [FromServices] IHostEnvironment hostEnvironment,
            [FromServices] IWebhookIngressEventStore? ingressStore,
            [FromServices] IWebhookActivityRecorder? activityRecorder,
            [FromServices] IJobRegistry registry,
            [FromServices] IJobExecutionPipeline pipeline,
            [FromServices] IPolicyResolver policyResolver,
            [FromServices] ILogger<WebhookEndpointApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (webhookStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhooks-unavailable", detail: "Webhook persistence provider not configured.");
            }

            if (string.IsNullOrWhiteSpace(environmentTag))
            {
                return MissingEnvironment("environmentTag");
            }

            var scope = new PartitionScope(tenantId, environmentTag);
            var endpoint = await webhookStore.FindByHookKeyAsync(hookKey, scope, cancellationToken).ConfigureAwait(false);
            if (endpoint is null)
            {
                return Results.NotFound(new { error = "webhook-not-found", hookKey });
            }

            if (!endpoint.Enabled)
            {
                return Results.Conflict(new { error = "webhook-disabled", hookKey, message = "Webhook is disabled and cannot be invoked." });
            }

            var payload = await ReadPayloadAsync(request).ConfigureAwait(false);

            var webhooksMode = configuration.GetValue<string?>("Croniq:Webhooks:Mode") ?? string.Empty;
            if (string.Equals(webhooksMode, "Remote", StringComparison.OrdinalIgnoreCase))
            {
                var remoteBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:BaseUrl") ?? string.Empty;
                var remoteIngressBaseUrl = configuration.GetValue<string?>("Croniq:Webhooks:Remote:IngressBaseUrl");
                var resolvedIngressBaseUrl = string.IsNullOrWhiteSpace(remoteIngressBaseUrl)
                    ? remoteBaseUrl
                    : remoteIngressBaseUrl;
                var remoteApiKey = configuration.GetValue<string?>("Croniq:Webhooks:Remote:ApiKey") ?? string.Empty;
                if (string.IsNullOrWhiteSpace(resolvedIngressBaseUrl) || string.IsNullOrWhiteSpace(remoteApiKey))
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-remote-unavailable", detail: "Remote webhook relay is not configured.");
                }

                if (!Uri.TryCreate(resolvedIngressBaseUrl, UriKind.Absolute, out var remoteUri))
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-remote-unavailable", detail: "Remote webhook relay ingress base URL is invalid.");
                }

                var allowInvalidCertificate = configuration.GetValue<bool?>("Croniq:Webhooks:Remote:AllowInvalidServerCertificate") ?? false;
                if (allowInvalidCertificate && !hostEnvironment.IsDevelopment())
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-remote-unavailable", detail: "Croniq:Webhooks:Remote:AllowInvalidServerCertificate is only supported in Development.");
                }

                var client = allowInvalidCertificate
                    ? new HttpClient(new HttpClientHandler
                    {
                        ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator
                    }, disposeHandler: true)
                    : httpClientFactory.CreateClient();
                client.BaseAddress = remoteUri;
                client.DefaultRequestHeaders.Remove("X-Croniq-Key");
                client.DefaultRequestHeaders.Add("X-Croniq-Key", remoteApiKey);
                client.DefaultRequestHeaders.Remove("X-Croniq-Relay-Key");
                client.DefaultRequestHeaders.Add("X-Croniq-Relay-Key", remoteApiKey);
                client.DefaultRequestHeaders.Remove(WebhookActivityHeaders.SourceHeaderName);
                client.DefaultRequestHeaders.Add(WebhookActivityHeaders.SourceHeaderName, WebhookActivitySources.Invoke);

                var path = $"tenants/{Uri.EscapeDataString(tenantId)}/environments/{Uri.EscapeDataString(environmentTag)}/webhooks/{Uri.EscapeDataString(hookKey)}";
                try
                {
                    using var content = new StringContent(payload ?? string.Empty, Encoding.UTF8, "application/json");
                    using var response = await client.PostAsync(path, content, cancellationToken).ConfigureAwait(false);
                    if (!response.IsSuccessStatusCode)
                    {
                        var errorBody = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
                        logger.LogWarning(
                            "remote webhook relay failed for {HookKey} ({TenantId}/{EnvironmentTag}) with status {StatusCode} {ReasonPhrase}. Body: {Body}",
                            endpoint.HookKey,
                            tenantId,
                            environmentTag,
                            (int)response.StatusCode,
                            response.ReasonPhrase,
                            string.IsNullOrWhiteSpace(errorBody) ? "<empty>" : errorBody);
                        var detail = string.IsNullOrWhiteSpace(errorBody)
                            ? $"Remote webhook relay failed with status {(int)response.StatusCode} ({response.ReasonPhrase})."
                            : errorBody;
                        await TryUpdateLastDeliveryAsync(webhookStore, endpoint, DateTimeOffset.UtcNow, cancellationToken).ConfigureAwait(false);
                        return Results.Problem(statusCode: (int)response.StatusCode, title: "webhook-relay-failed", detail: detail);
                    }
                }
                catch (HttpRequestException ex)
                {
                    logger.LogError(ex, "remote webhook relay failed for {HookKey}", endpoint.HookKey);
                    await TryUpdateLastDeliveryAsync(webhookStore, endpoint, DateTimeOffset.UtcNow, cancellationToken).ConfigureAwait(false);
                    return Results.Problem(statusCode: StatusCodes.Status502BadGateway, title: "webhook-relay-failed", detail: ex.Message);
                }

                await TryUpdateLastDeliveryAsync(webhookStore, endpoint, DateTimeOffset.UtcNow, cancellationToken).ConfigureAwait(false);
                return Results.Accepted(value: new WebhookInvokeResult("relayed", endpoint.HookKey, endpoint.JobKey, string.Empty));
            }

            if (!JobKey.TryParse(endpoint.JobKey, out var jobKey))
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "invalid-job-key", detail: "Configured job key is invalid.");
            }

            if (!registry.TryGet(jobKey, out var descriptor))
            {
                return Results.NotFound(new { error = "job-not-registered", endpoint.JobKey });
            }

            var metadata = CreateWebhookMetadata(endpoint, payload, WebhookActivitySources.Invoke);

            var dispatchMode = configuration.GetValue<string?>("Croniq:Webhooks:Ingress:DispatchMode") ?? string.Empty;
            if (string.Equals(dispatchMode, "StoreOnly", StringComparison.OrdinalIgnoreCase))
            {
                if (ingressStore is null)
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "ingress-store-missing", detail: "Webhook ingress store is not configured.");
                }

                var eventId = Guid.NewGuid().ToString("N");
                await ingressStore.EnqueueAsync(
                    new WebhookIngressEventCreate(
                        eventId,
                        endpoint.HookKey,
                        endpoint.JobKey,
                        scope.TenantId,
                        scope.EnvironmentTag,
                        payload,
                        Headers: null,
                        metadata,
                        DateTimeOffset.UtcNow),
                    cancellationToken).ConfigureAwait(false);

                return Results.Accepted(value: new { status = "stored", eventId, hook = endpoint.HookKey, job = endpoint.JobKey });
            }

            var executionOptions = policyResolver.ResolveExecution(jobKey, scope);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, TriggerActivitySource);
            var occurredAtUtc = DateTimeOffset.UtcNow;

            using var invokeActivity = TriggerActivitySource.StartActivity("Croniq.Api.WebhookInvoke", ActivityKind.Server);
            invokeActivity?.SetTag("croniq.webhook.key", endpoint.HookKey);
            invokeActivity?.SetTag("croniq.job.key", jobKey.Value);

            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                invokeActivity?.SetStatus(ActivityStatusCode.Ok);
                await TryRecordInvokeActivityAsync(
                    activityRecorder,
                    logger,
                    new WebhookActivityRecord(
                        executionId,
                        endpoint.HookKey,
                        jobKey.Value,
                        scope.TenantId,
                        scope.EnvironmentTag,
                        occurredAtUtc,
                        WebhookActivityStatus.Success,
                        WebhookActivitySources.Invoke,
                        Reason: null,
                        Payload: payload,
                        Metadata: metadata),
                    cancellationToken).ConfigureAwait(false);
                await TryUpdateLastDeliveryAsync(webhookStore, endpoint, occurredAtUtc, cancellationToken).ConfigureAwait(false);
                return Results.Accepted(value: new WebhookInvokeResult("invoked", endpoint.HookKey, endpoint.JobKey, executionId));
            }
            catch (Exception ex)
            {
                invokeActivity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                logger.LogError(ex, "failed to invoke webhook {HookKey}", endpoint.HookKey);
                await TryRecordInvokeActivityAsync(
                    activityRecorder,
                    logger,
                    new WebhookActivityRecord(
                        executionId,
                        endpoint.HookKey,
                        jobKey.Value,
                        scope.TenantId,
                        scope.EnvironmentTag,
                        occurredAtUtc,
                        WebhookActivityStatus.Failed,
                        WebhookActivitySources.Invoke,
                        Reason: ex.Message,
                        Payload: payload,
                        Metadata: metadata),
                    cancellationToken).ConfigureAwait(false);
                await TryUpdateLastDeliveryAsync(webhookStore, endpoint, occurredAtUtc, cancellationToken).ConfigureAwait(false);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "webhook-invoke-failed", detail: ex.Message);
            }
        })
        .WithDocs("Webhooks_Invoke", "Invoke webhook endpoint", "Triggers a webhook endpoint through the job execution pipeline.")
        .Produces<WebhookInvokeResult>(StatusCodes.Status202Accepted)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status409Conflict)
        .Produces(StatusCodes.Status502BadGateway)
        .Produces(StatusCodes.Status500InternalServerError)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScopeFromRoute("environmentTag", CroniqScopes.WebhooksWrite);

        app.MapGet("/tenants/{tenantId}/webhooks/deadletters", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookDeadLetterStore? deadLetterStore,
            CancellationToken cancellationToken) =>
        {
            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-deadletter-unavailable", detail: "Webhook dead-letter store not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var entries = await deadLetterStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);
            var response = entries.Select(ToWebhookDeadLetterResponse).ToList();
            return Results.Ok(response);
        })
        .WithDocs("WebhookDeadLetters_List", "List webhook dead letters", "Enumerates failed webhook deliveries for investigation or replay.")
        .Produces<List<WebhookDeadLetterResponse>>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksDeadLetter);

        app.MapGet("/tenants/{tenantId}/webhooks/activity", async (
            string tenantId,
            string? environment,
            DateTimeOffset? fromUtc,
            DateTimeOffset? toUtc,
            DateTimeOffset? updatedSinceUtc,
            string? hookKeys,
            string? jobKeys,
            int? limit,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookActivityStore? activityStore,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (activityStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-activity-unavailable", detail: "Webhook activity store not configured.");
            }

            var parsedHookKeys = ParseWebhookActivityKeys(hookKeys);
            var parsedJobKeys = ParseWebhookActivityKeys(jobKeys);
            if (!TryNormalizeWebhookActivityQuery(fromUtc, toUtc, updatedSinceUtc, limit, parsedHookKeys, parsedJobKeys, out var query, out var error))
            {
                return Results.BadRequest(new { error = "invalid-activity-query", message = error });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var entries = await activityStore.ListAsync(scope, query, cancellationToken).ConfigureAwait(false);
            var response = entries.Select(ToWebhookActivityTimelineEntry).ToList();
            return Results.Ok(response);
        })
        .WithDocs("WebhookActivity_List", "List webhook activity", "Returns webhook activity entries for the tenant/environment scope.")
        .Produces<List<WebhookActivityTimelineEntry>>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapGet("/tenants/{tenantId}/webhooks/activity/stream", async (
            string tenantId,
            string? environment,
            DateTimeOffset? fromUtc,
            DateTimeOffset? toUtc,
            DateTimeOffset? updatedSinceUtc,
            string? hookKeys,
            string? jobKeys,
            int? limit,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookActivityStore? activityStore,
            [FromServices] ILogger<WebhookEndpointApiMarker> logger,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                await MissingEnvironment().ExecuteAsync(httpContext);
                return;
            }

            if (activityStore is null)
            {
                await Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-activity-unavailable", detail: "Webhook activity store not configured.")
                    .ExecuteAsync(httpContext);
                return;
            }

            var parsedHookKeys = ParseWebhookActivityKeys(hookKeys);
            var parsedJobKeys = ParseWebhookActivityKeys(jobKeys);
            var resolvedLimit = limit ?? 1;
            if (!TryNormalizeWebhookActivityQuery(fromUtc, toUtc, updatedSinceUtc, resolvedLimit, parsedHookKeys, parsedJobKeys, out var query, out var error))
            {
                await Results.BadRequest(new { error = "invalid-activity-stream-query", message = error }).ExecuteAsync(httpContext);
                return;
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);

            httpContext.Response.StatusCode = StatusCodes.Status200OK;
            httpContext.Response.ContentType = "text/event-stream";
            httpContext.Response.Headers["Cache-Control"] = "no-cache";
            httpContext.Response.Headers.Append("X-Accel-Buffering", "no");

            var lastSeenUtc = query.UpdatedSinceUtc ?? query.FromUtc ?? DateTimeOffset.UtcNow;
            if (query.ToUtc.HasValue && query.ToUtc.Value < lastSeenUtc)
            {
                lastSeenUtc = query.ToUtc.Value;
            }

            try
            {
                while (!cancellationToken.IsCancellationRequested)
                {
                    var pollStartedAt = DateTimeOffset.UtcNow;
                    var probeQuery = new WebhookActivityQuery
                    {
                        FromUtc = query.FromUtc,
                        ToUtc = query.ToUtc,
                        UpdatedSinceUtc = lastSeenUtc,
                        HookKeys = query.HookKeys,
                        JobKeys = query.JobKeys,
                        Limit = query.Limit
                    }.Normalize();

                    IReadOnlyCollection<WebhookActivityEntry> entries;
                    try
                    {
                        entries = await activityStore.ListAsync(scope, probeQuery, cancellationToken).ConfigureAwait(false);
                    }
                    catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
                    {
                        break;
                    }
                    catch (Exception ex)
                    {
                        logger.LogWarning(
                            ex,
                            "webhook activity stream poll failed for {TenantId}/{EnvironmentTag}",
                            scope.TenantId,
                            scope.EnvironmentTag);
                        await WriteSseCommentAsync(httpContext.Response, "upstream-error", cancellationToken).ConfigureAwait(false);
                        await Task.Delay(WebhookActivityStreamPollInterval, cancellationToken).ConfigureAwait(false);
                        continue;
                    }
                    if (entries.Count > 0)
                    {
                        var latestOccurredAtUtc = entries.Max(entry => entry.OccurredAtUtc);
                        var payload = new WebhookActivityStreamEvent(
                            "activity.updated",
                            DateTimeOffset.UtcNow,
                            latestOccurredAtUtc);
                        var json = JsonSerializer.Serialize(payload, WebhookActivityStreamJsonOptions);
                        await WriteSseDataAsync(httpContext.Response, json, cancellationToken).ConfigureAwait(false);
                    }
                    else
                    {
                        await WriteSseCommentAsync(httpContext.Response, "heartbeat", cancellationToken).ConfigureAwait(false);
                    }

                    lastSeenUtc = pollStartedAt;
                    await Task.Delay(WebhookActivityStreamPollInterval, cancellationToken).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                // ignore cancellation
            }
        })
        .WithDocs("WebhookActivity_Stream", "Stream webhook activity", "Server-sent events stream for webhook activity updates.")
        .Produces(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapGet("/tenants/{tenantId}/webhooks/activity/summary", async (
            string tenantId,
            string? environment,
            DateTimeOffset? fromUtc,
            DateTimeOffset? toUtc,
            string? hookKeys,
            string? jobKeys,
            int? bucketMinutes,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookActivityStore? activityStore,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (activityStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-activity-unavailable", detail: "Webhook activity store not configured.");
            }

            var parsedHookKeys = ParseWebhookActivityKeys(hookKeys);
            var parsedJobKeys = ParseWebhookActivityKeys(jobKeys);
            if (!TryNormalizeWebhookActivitySummaryQuery(fromUtc, toUtc, bucketMinutes, parsedHookKeys, parsedJobKeys, out var query, out var error))
            {
                return Results.BadRequest(new { error = "invalid-activity-summary-query", message = error });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var summary = await activityStore.SummarizeAsync(scope, query, cancellationToken).ConfigureAwait(false);
            return Results.Ok(ToWebhookActivitySummaryResponse(summary));
        })
        .WithDocs("WebhookActivity_Summary", "Summarize webhook activity", "Returns aggregated webhook activity counts for the tenant/environment scope.")
        .Produces<ApiWebhookActivitySummary>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksRead);

        app.MapPost("/tenants/{tenantId}/webhooks/deadletters/{deadLetterId}/replay", async (
            string tenantId,
            long deadLetterId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookDeadLetterStore? deadLetterStore,
            [FromServices] IJobRegistry registry,
            [FromServices] IJobExecutionPipeline pipeline,
            [FromServices] IPolicyResolver policyResolver,
            [FromServices] ILogger<WebhookDeadLetterApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-deadletter-unavailable", detail: "Webhook dead-letter store not configured.");
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

            var executionOptions = policyResolver.ResolveExecution(jobKey, scope);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, TriggerActivitySource);

            using var replayActivity = TriggerActivitySource.StartActivity("Croniq.Api.WebhookReplay", ActivityKind.Server);
            replayActivity?.SetTag("croniq.webhook.deadletter", entry.Id);
            replayActivity?.SetTag("croniq.webhook.key", entry.HookKey);
            replayActivity?.SetTag("croniq.job.key", jobKey.Value);

            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                await deadLetterStore.ResolveAsync(deadLetterId, scope, cancellationToken).ConfigureAwait(false);
                replayActivity?.SetStatus(ActivityStatusCode.Ok);
                return Results.Ok(new WebhookReplayResult("replayed", entry.HookKey, entry.JobKey));
            }
            catch (Exception ex)
            {
                replayActivity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                logger.LogError(ex, "failed to replay webhook deadletter {DeadLetterId}", deadLetterId);
                await deadLetterStore.RecordFailureAsync(deadLetterId, scope, new WebhookDeadLetterFailure("execution-error", StatusCodes.Status500InternalServerError, ex.Message, null), cancellationToken).ConfigureAwait(false);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "replay-failed", detail: ex.Message);
            }
        })
        .WithDocs("WebhookDeadLetters_Replay", "Replay webhook dead letter", "Re-dispatches a failed webhook payload via the job execution pipeline.")
        .Produces<WebhookReplayResult>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksDeadLetter);

        app.MapPost("/tenants/{tenantId}/webhooks/deadletters/{deadLetterId}:resolve", async (
            string tenantId,
            long deadLetterId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookDeadLetterStore? deadLetterStore,
            CancellationToken cancellationToken) =>
        {
            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-deadletter-unavailable", detail: "Webhook dead-letter store not configured.");
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            await deadLetterStore.ResolveAsync(deadLetterId, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("WebhookDeadLetters_Resolve", "Resolve webhook dead letter", "Marks a webhook dead letter as resolved without replaying it.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksDeadLetter);

        app.MapPost("/tenants/{tenantId}/webhooks/deadletters/{deadLetterId}:fail", async (
            string tenantId,
            long deadLetterId,
            string? environment,
            WebhookDeadLetterFailureRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookDeadLetterStore? deadLetterStore,
            CancellationToken cancellationToken) =>
        {
            if (deadLetterStore is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "webhook-deadletter-unavailable", detail: "Webhook dead-letter store not configured.");
            }

            if (request is null || string.IsNullOrWhiteSpace(request.FailureReason))
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var failure = new WebhookDeadLetterFailure(
                request.FailureReason,
                request.StatusCode,
                request.ErrorDetails,
                request.NextAttemptAtUtc);
            await deadLetterStore.RecordFailureAsync(deadLetterId, scope, failure, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("WebhookDeadLetters_RecordFailure", "Record webhook dead letter failure", "Stores a failure update for a webhook dead letter without replaying it.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status503ServiceUnavailable)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksDeadLetter);
    }

    private static async Task<string> ReadPayloadAsync(HttpRequest request)
    {
        if (request.Body.CanSeek)
        {
            request.Body.Position = 0;
        }

        using var reader = new StreamReader(request.Body, Encoding.UTF8, leaveOpen: true);
        var payload = await reader.ReadToEndAsync().ConfigureAwait(false);

        if (request.Body.CanSeek)
        {
            request.Body.Position = 0;
        }

        return payload;
    }

    private static IReadOnlyCollection<string>? ParseWebhookActivityKeys(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return null;
        }

        var values = raw
            .Split(new[] { ',', ';', ' ' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return values.Length == 0 ? null : values;
    }

    private static bool TryNormalizeWebhookActivityQuery(
        DateTimeOffset? fromUtc,
        DateTimeOffset? toUtc,
        DateTimeOffset? updatedSinceUtc,
        int? limit,
        IReadOnlyCollection<string>? hookKeys,
        IReadOnlyCollection<string>? jobKeys,
        out WebhookActivityQuery normalized,
        out string error)
    {
        normalized = new WebhookActivityQuery();
        error = string.Empty;

        if (fromUtc.HasValue && toUtc.HasValue && fromUtc > toUtc)
        {
            error = "fromUtc must be earlier than toUtc.";
            return false;
        }

        var query = new WebhookActivityQuery
        {
            FromUtc = fromUtc,
            ToUtc = toUtc,
            UpdatedSinceUtc = updatedSinceUtc,
            HookKeys = hookKeys,
            JobKeys = jobKeys,
            Limit = limit ?? WebhookActivityQuery.DefaultLimit
        };

        normalized = query.Normalize();
        return true;
    }

    private static bool TryNormalizeWebhookActivitySummaryQuery(
        DateTimeOffset? fromUtc,
        DateTimeOffset? toUtc,
        int? bucketMinutes,
        IReadOnlyCollection<string>? hookKeys,
        IReadOnlyCollection<string>? jobKeys,
        out WebhookActivitySummaryQuery normalized,
        out string error)
    {
        normalized = new WebhookActivitySummaryQuery();
        error = string.Empty;

        var nowUtc = DateTimeOffset.UtcNow;
        var windowEnd = toUtc ?? nowUtc;
        var windowStart = fromUtc ?? windowEnd.AddMinutes(-WebhookActivitySummaryQuery.DefaultWindowMinutes);

        if (windowStart > windowEnd)
        {
            error = "fromUtc must be earlier than toUtc.";
            return false;
        }

        var windowMinutes = (int)Math.Ceiling((windowEnd - windowStart).TotalMinutes);
        if (windowMinutes <= 0)
        {
            error = "window must be at least 1 minute.";
            return false;
        }

        if (windowMinutes > WebhookActivitySummaryQuery.MaxWindowMinutes)
        {
            error = $"window cannot exceed {WebhookActivitySummaryQuery.MaxWindowMinutes} minutes.";
            return false;
        }

        var resolvedBucket = bucketMinutes ?? WebhookActivitySummaryQuery.DefaultBucketMinutes;
        if (resolvedBucket <= 0)
        {
            error = "bucketMinutes must be greater than zero.";
            return false;
        }

        if (resolvedBucket > WebhookActivitySummaryQuery.MaxBucketMinutes)
        {
            error = $"bucketMinutes must be between 1 and {WebhookActivitySummaryQuery.MaxBucketMinutes}.";
            return false;
        }

        if (resolvedBucket > windowMinutes)
        {
            error = "bucketMinutes must be less than or equal to the window size.";
            return false;
        }

        normalized = new WebhookActivitySummaryQuery
        {
            FromUtc = windowStart,
            ToUtc = windowEnd,
            HookKeys = hookKeys,
            JobKeys = jobKeys,
            BucketMinutes = resolvedBucket
        };

        return true;
    }

    private static WebhookActivityTimelineEntry ToWebhookActivityTimelineEntry(WebhookActivityEntry entry)
    {
        var kind = entry.Kind == WebhookActivityKind.DeadLetter ? "deadLetter" : "delivery";
        var status = entry.Status switch
        {
            WebhookActivityStatus.Success => "success",
            WebhookActivityStatus.Failed => "failed",
            WebhookActivityStatus.Warning => "warning",
            WebhookActivityStatus.Pending => "pending",
            WebhookActivityStatus.Leased => "leased",
            _ => "warning"
        };
        var id = entry.Kind == WebhookActivityKind.DeadLetter
            ? $"deadletter:{entry.Id}"
            : $"delivery:{entry.Id}";
        var requestId = entry.Kind == WebhookActivityKind.Delivery ? entry.Id : null;
        var source = string.IsNullOrWhiteSpace(entry.Source)
            ? WebhookActivitySources.Ingress
            : entry.Source;

        return new WebhookActivityTimelineEntry(
            id,
            kind,
            status,
            entry.HookKey,
            entry.JobKey,
            entry.EnvironmentTag,
            source,
            entry.OccurredAtUtc,
            entry.LatencyMs,
            entry.Attempts,
            PayloadBytes: entry.PayloadBytes,
            RequestId: requestId,
            Reason: entry.Reason,
            DeadLetterId: entry.DeadLetterId);
    }

    private static async Task<IReadOnlyDictionary<string, DateTimeOffset>> TryResolveLastDeliveriesAsync(
        IWebhookActivityStore? activityStore,
        PartitionScope scope,
        IReadOnlyCollection<WebhookEndpointDefinition> endpoints,
        CancellationToken cancellationToken)
    {
        if (activityStore is null || endpoints.Count == 0)
        {
            return new Dictionary<string, DateTimeOffset>(StringComparer.OrdinalIgnoreCase);
        }

        var results = new Dictionary<string, DateTimeOffset>(StringComparer.OrdinalIgnoreCase);

        foreach (var endpoint in endpoints)
        {
            if (string.IsNullOrWhiteSpace(endpoint.HookKey))
            {
                continue;
            }

            var query = new WebhookActivityQuery
            {
                HookKeys = new[] { endpoint.HookKey },
                Limit = 10
            }.Normalize();

            IReadOnlyCollection<WebhookActivityEntry> entries;
            try
            {
                entries = await activityStore.ListAsync(scope, query, cancellationToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                throw;
            }
            catch
            {
                continue;
            }

            if (entries.Count == 0)
            {
                continue;
            }

            var latestDelivery = entries
                .Where(entry => entry.Kind == WebhookActivityKind.Delivery)
                .OrderByDescending(entry => entry.OccurredAtUtc)
                .FirstOrDefault();

            if (latestDelivery is not null)
            {
                results[endpoint.HookKey] = latestDelivery.OccurredAtUtc;
            }
        }

        return results;
    }

    private static ApiWebhookActivitySummary ToWebhookActivitySummaryResponse(
        Croniq.Persistence.Abstractions.WebhookActivitySummary summary)
    {
        var buckets = summary.Buckets
            .Select(bucket => new ApiWebhookActivityBucket(
                bucket.BucketStartUtc,
                bucket.BucketEndUtc,
                bucket.TotalCount,
                bucket.ErrorCount,
                bucket.WarningCount,
                bucket.PendingCount,
                bucket.LeasedCount,
                bucket.DeadLetterCount,
                bucket.P95LatencyMs))
            .ToArray();

        return new ApiWebhookActivitySummary(
            summary.BucketMinutes,
            summary.WindowStartUtc,
            summary.WindowEndUtc,
            buckets);
    }

    private static async Task TryRecordInvokeActivityAsync(
        IWebhookActivityRecorder? recorder,
        ILogger logger,
        WebhookActivityRecord record,
        CancellationToken cancellationToken)
    {
        if (recorder is null)
        {
            return;
        }

        try
        {
            await recorder.RecordAsync(record, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to record webhook invoke activity for {HookKey}", record.HookKey);
        }
    }

    private static async Task TryUpdateLastDeliveryAsync(
        IWebhookPersistenceProvider? webhookStore,
        WebhookEndpointDefinition endpoint,
        DateTimeOffset occurredAtUtc,
        CancellationToken cancellationToken)
    {
        if (webhookStore is null)
        {
            return;
        }

        var metadata = endpoint.Metadata is null
            ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            : new Dictionary<string, string>(endpoint.Metadata, StringComparer.OrdinalIgnoreCase);

        if (metadata.TryGetValue("lastDeliveryAtUtc", out var existing)
            && DateTimeOffset.TryParse(existing, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var parsed)
            && parsed >= occurredAtUtc)
        {
            return;
        }

        metadata["lastDeliveryAtUtc"] = occurredAtUtc.ToString("O", CultureInfo.InvariantCulture);

        try
        {
            await webhookStore.UpsertAsync(
                new WebhookEndpointUpsert(
                    endpoint.HookKey,
                    endpoint.JobKey,
                    endpoint.TenantId,
                    endpoint.EnvironmentTag,
                    endpoint.Enabled,
                    endpoint.RequireSignature,
                    endpoint.RequestsPerMinute,
                    Secret: null,
                    endpoint.SignatureVersion,
                    metadata),
                cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            // Ignore failures - last delivery is informational.
        }
    }

    private static Dictionary<string, string> CreateWebhookMetadata(
        WebhookEndpointDefinition endpoint,
        string payload,
        string source)
    {
        var metadata = endpoint.Metadata is null
            ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            : new Dictionary<string, string>(endpoint.Metadata, StringComparer.OrdinalIgnoreCase);

        metadata["webhook:hook"] = endpoint.HookKey;
        metadata[WebhookActivityMetadata.SourceKey] = source;

        if (!string.IsNullOrWhiteSpace(payload))
        {
            metadata["webhook:payload"] = payload;
            TryAddJsonHints(metadata, payload);
        }

        return metadata;
    }

    private static void TryAddJsonHints(IDictionary<string, string> metadata, string payload)
    {
        try
        {
            using var document = JsonDocument.Parse(payload);
            if (document.RootElement.ValueKind != JsonValueKind.Object)
            {
                return;
            }

            foreach (var property in document.RootElement.EnumerateObject())
            {
                var key = $"payload:{property.Name}";
                switch (property.Value.ValueKind)
                {
                    case JsonValueKind.String:
                        metadata[key] = property.Value.GetString() ?? string.Empty;
                        break;
                    case JsonValueKind.Number when property.Value.TryGetDecimal(out var number):
                        metadata[key] = number.ToString(CultureInfo.InvariantCulture);
                        break;
                    case JsonValueKind.True:
                    case JsonValueKind.False:
                        metadata[key] = property.Value.GetBoolean().ToString();
                        break;
                }
            }
        }
        catch (JsonException)
        {
            // ignore malformed payloads
        }
    }
}
