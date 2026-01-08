using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static readonly JsonSerializerOptions WebhookIngressJsonOptions = new(JsonSerializerDefaults.Web);

    private static void MapWebhookIngressHttpEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapGet("/tenants/{tenantId}/webhooks/ingress/stream", async (
            string tenantId,
            string? environment,
            string? consumerId,
            int? maxInflight,
            int? maxBatchSize,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookIngressEventStore store,
            [FromServices] IOptions<WebhookIngressStreamOptions> options,
            [FromServices] WebhookIngressConsumerTracker tracker,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(consumerId))
            {
                httpContext.Response.StatusCode = StatusCodes.Status400BadRequest;
                await httpContext.Response.WriteAsJsonAsync(new { error = "consumer-required", message = "Query parameter 'consumerId' is required." }, cancellationToken);
                return;
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                httpContext.Response.StatusCode = StatusCodes.Status400BadRequest;
                await httpContext.Response.WriteAsJsonAsync(new { error = "missing-environment", message = "Query parameter 'environment' is required." }, cancellationToken);
                return;
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var streamOptions = options?.Value ?? new WebhookIngressStreamOptions();
            var leaseDuration = ResolveLeaseDuration(streamOptions);
            var pollingInterval = ResolvePollingInterval(streamOptions);
            var maxInflightValue = NormalizeMaxInflight(maxInflight, streamOptions);
            var maxBatchValue = NormalizeBatchSize(maxBatchSize, streamOptions);

            httpContext.Response.StatusCode = StatusCodes.Status200OK;
            httpContext.Response.ContentType = "text/event-stream";
            httpContext.Response.Headers["Cache-Control"] = "no-cache";
            httpContext.Response.Headers.Append("X-Accel-Buffering", "no");

            tracker.Reset(consumerId);

            try
            {
                while (!cancellationToken.IsCancellationRequested)
                {
                    tracker.RemoveExpired(consumerId, DateTimeOffset.UtcNow);
                    var available = maxInflightValue - tracker.GetCount(consumerId);
                    if (available > 0)
                    {
                        var batchSize = Math.Min(available, maxBatchValue);
                        var leases = await store.AcquireAsync(
                            new WebhookIngressAcquireRequest(scope, DateTimeOffset.UtcNow, batchSize, leaseDuration),
                            cancellationToken).ConfigureAwait(false);

                        foreach (var lease in leases)
                        {
                            tracker.AddLease(consumerId, lease);
                            var token = ToIngressToken(lease);
                            var json = JsonSerializer.Serialize(token, WebhookIngressJsonOptions);
                            await WriteSseDataAsync(httpContext.Response, json, cancellationToken).ConfigureAwait(false);
                        }

                        if (leases.Count > 0)
                        {
                            continue;
                        }
                    }

                    await WriteSseCommentAsync(httpContext.Response, "heartbeat", cancellationToken).ConfigureAwait(false);
                    await Task.Delay(pollingInterval, cancellationToken).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                // ignore cancellation
            }
            finally
            {
                tracker.RemoveConsumer(consumerId);
            }
        })
        .WithDocs(
            "WebhookIngress_Stream",
            "Stream webhook ingress events",
            "Server-sent events stream for remote webhook ingress consumption.")
        .RequireCroniqTenantScope(CroniqScopes.WebhooksIngress);

        app.MapGet("/tenants/{tenantId}/webhooks/ingress/poll", async (
            string tenantId,
            string? environment,
            int? maxBatchSize,
            int? waitMs,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookIngressEventStore store,
            [FromServices] IOptions<WebhookIngressStreamOptions> options,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var pollWaitMs = waitMs.GetValueOrDefault(0);
            if (pollWaitMs < 0 || pollWaitMs > 30_000)
            {
                return Results.BadRequest(new { error = "invalid-wait", message = "waitMs must be between 0 and 30000." });
            }

            var scope = new PartitionScope(tenantId.Trim(), resolvedEnvironment);
            var streamOptions = options?.Value ?? new WebhookIngressStreamOptions();
            var leaseDuration = ResolveLeaseDuration(streamOptions);
            var pollingInterval = ResolvePollingInterval(streamOptions);
            var batchSize = NormalizeBatchSize(maxBatchSize, streamOptions);

            IReadOnlyCollection<WebhookIngressLease> leases = Array.Empty<WebhookIngressLease>();
            var deadlineUtc = pollWaitMs > 0
                ? DateTimeOffset.UtcNow.AddMilliseconds(pollWaitMs)
                : DateTimeOffset.UtcNow;

            while (true)
            {
                leases = await store.AcquireAsync(
                    new WebhookIngressAcquireRequest(scope, DateTimeOffset.UtcNow, batchSize, leaseDuration),
                    cancellationToken).ConfigureAwait(false);

                if (leases.Count > 0 || pollWaitMs <= 0)
                {
                    break;
                }

                var remaining = deadlineUtc - DateTimeOffset.UtcNow;
                if (remaining <= TimeSpan.Zero)
                {
                    break;
                }

                var delay = remaining < pollingInterval
                    ? remaining
                    : pollingInterval;

                await Task.Delay(delay, cancellationToken).ConfigureAwait(false);
            }

            var payload = leases.Select(ToIngressToken).ToArray();
            return Results.Ok(new WebhookIngressPollResponse(payload, DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()));
        })
        .WithDocs(
            "WebhookIngress_Poll",
            "Poll webhook ingress events",
            "Polls the ingress event store for remote webhook events.")
        .Produces<WebhookIngressPollResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksIngress);

        app.MapPost("/tenants/{tenantId}/webhooks/ingress/ack", async (
            string tenantId,
            string? environment,
            WebhookIngressAckRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookIngressEventStore store,
            [FromServices] WebhookIngressConsumerTracker tracker,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            if (string.IsNullOrWhiteSpace(request.EventId))
            {
                return Results.BadRequest(new { error = "event-required", message = "EventId is required." });
            }

            if (string.IsNullOrWhiteSpace(request.LeaseId))
            {
                return Results.BadRequest(new { error = "lease-required", message = "LeaseId is required." });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var ack = new WebhookIngressAck(
                request.EventId.Trim(),
                request.LeaseId.Trim(),
                request.Succeeded,
                string.IsNullOrWhiteSpace(request.ErrorMessage) ? null : request.ErrorMessage.Trim(),
                DateTimeOffset.UtcNow);

            await store.AcknowledgeAsync(ack, cancellationToken).ConfigureAwait(false);
            tracker.RemoveLease(request.ConsumerId, ack.LeaseId);
            return Results.NoContent();
        })
        .WithDocs(
            "WebhookIngress_Ack",
            "Acknowledge webhook ingress event",
            "Acknowledges a webhook ingress event and marks it delivered or failed.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksIngress);

        app.MapPost("/tenants/{tenantId}/webhooks/ingress/nack", async (
            string tenantId,
            string? environment,
            WebhookIngressNackRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookIngressEventStore store,
            [FromServices] WebhookIngressConsumerTracker tracker,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            if (string.IsNullOrWhiteSpace(request.EventId))
            {
                return Results.BadRequest(new { error = "event-required", message = "EventId is required." });
            }

            if (string.IsNullOrWhiteSpace(request.LeaseId))
            {
                return Results.BadRequest(new { error = "lease-required", message = "LeaseId is required." });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var nack = new WebhookIngressNack(
                request.EventId.Trim(),
                request.LeaseId.Trim(),
                string.IsNullOrWhiteSpace(request.Reason) ? null : request.Reason.Trim(),
                DateTimeOffset.UtcNow);

            await store.NackAsync(nack, cancellationToken).ConfigureAwait(false);
            tracker.RemoveLease(request.ConsumerId, nack.LeaseId);
            return Results.NoContent();
        })
        .WithDocs(
            "WebhookIngress_Nack",
            "Nack webhook ingress event",
            "Re-queues a webhook ingress event for retry.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksIngress);

        app.MapPost("/tenants/{tenantId}/webhooks/ingress/extend", async (
            string tenantId,
            string? environment,
            WebhookIngressExtendRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IWebhookIngressEventStore store,
            [FromServices] WebhookIngressConsumerTracker tracker,
            CancellationToken cancellationToken) =>
        {
            if (request is null)
            {
                return Results.BadRequest(new { error = "invalid-request" });
            }

            if (string.IsNullOrWhiteSpace(request.EventId))
            {
                return Results.BadRequest(new { error = "event-required", message = "EventId is required." });
            }

            if (string.IsNullOrWhiteSpace(request.LeaseId))
            {
                return Results.BadRequest(new { error = "lease-required", message = "LeaseId is required." });
            }

            if (request.LeaseExpiresAtUtc <= 0)
            {
                return Results.BadRequest(new { error = "lease-expiry-required", message = "LeaseExpiresAtUtc must be set." });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var expiresAt = DateTimeOffset.FromUnixTimeMilliseconds(request.LeaseExpiresAtUtc);
            var renewal = new WebhookIngressLeaseRenewal(
                request.EventId.Trim(),
                request.LeaseId.Trim(),
                expiresAt,
                DateTimeOffset.UtcNow);

            var extended = await store.TryExtendLeaseAsync(renewal, cancellationToken).ConfigureAwait(false);
            if (extended)
            {
                tracker.UpdateLeaseExpiry(request.ConsumerId, renewal.LeaseId, renewal.LeaseExpiresAtUtc);
            }

            return Results.Ok(new WebhookIngressExtendResponse(extended));
        })
        .WithDocs(
            "WebhookIngress_Extend",
            "Extend webhook ingress lease",
            "Extends the lease for a webhook ingress event.")
        .Produces<WebhookIngressExtendResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.WebhooksIngress);
    }

    private static WebhookIngressEventToken ToIngressToken(WebhookIngressLease lease)
    {
        return new WebhookIngressEventToken(
            lease.EventId,
            lease.LeaseId,
            lease.LeaseExpiresAtUtc.ToUnixTimeMilliseconds(),
            lease.HookKey,
            lease.JobKey,
            lease.Payload ?? string.Empty,
            lease.Headers is null ? null : new Dictionary<string, string>(lease.Headers, StringComparer.OrdinalIgnoreCase),
            lease.ReceivedAtUtc.ToUnixTimeMilliseconds(),
            lease.Metadata is null ? null : new Dictionary<string, string>(lease.Metadata, StringComparer.OrdinalIgnoreCase));
    }

    private static TimeSpan ResolveLeaseDuration(WebhookIngressStreamOptions options)
    {
        var seconds = Math.Clamp(options.LeaseSeconds, 5, 600);
        return TimeSpan.FromSeconds(seconds);
    }

    private static int NormalizeBatchSize(int? requested, WebhookIngressStreamOptions options)
    {
        var value = requested.GetValueOrDefault(options.MaxBatchSize);
        return Math.Clamp(value, 1, 500);
    }

    private static int NormalizeMaxInflight(int? requested, WebhookIngressStreamOptions options)
    {
        var value = requested.GetValueOrDefault(options.MaxBatchSize);
        return Math.Clamp(value, 1, 250);
    }

    private static TimeSpan ResolvePollingInterval(WebhookIngressStreamOptions options)
    {
        return TimeSpan.FromMilliseconds(Math.Clamp(options.PollingIntervalMilliseconds, 100, 5000));
    }

    private static async Task WriteSseDataAsync(HttpResponse response, string data, CancellationToken cancellationToken)
    {
        await response.WriteAsync("data: ", cancellationToken).ConfigureAwait(false);
        await response.WriteAsync(data, cancellationToken).ConfigureAwait(false);
        await response.WriteAsync("\n\n", cancellationToken).ConfigureAwait(false);
        await response.Body.FlushAsync(cancellationToken).ConfigureAwait(false);
    }

    private static async Task WriteSseCommentAsync(HttpResponse response, string comment, CancellationToken cancellationToken)
    {
        await response.WriteAsync(": ", cancellationToken).ConfigureAwait(false);
        await response.WriteAsync(comment, cancellationToken).ConfigureAwait(false);
        await response.WriteAsync("\n\n", cancellationToken).ConfigureAwait(false);
        await response.Body.FlushAsync(cancellationToken).ConfigureAwait(false);
    }
}
