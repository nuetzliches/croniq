using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using Croniq.Auth.Abstractions;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

internal sealed class WebhookIngressGrpcService : WebhookIngress.WebhookIngressBase
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Api.Grpc.WebhookIngress");
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly IWebhookIngressEventStore _store;
    private readonly WebhookIngressStreamOptions _options;
    private readonly ILogger<WebhookIngressGrpcService> _logger;

    public WebhookIngressGrpcService(
        ICallerContextAccessor callerAccessor,
        IWebhookIngressEventStore store,
        IOptions<WebhookIngressStreamOptions> options,
        ILogger<WebhookIngressGrpcService> logger)
    {
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _options = options?.Value ?? new WebhookIngressStreamOptions();
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public override async Task Connect(
        IAsyncStreamReader<WebhookIngressClientMessage> requestStream,
        IServerStreamWriter<WebhookIngressServerMessage> responseStream,
        ServerCallContext context)
    {
        using var activity = ActivitySource.StartActivity("Croniq.Grpc.WebhookIngress.Connect", ActivityKind.Server);

        var caller = _callerAccessor.Current;
        if (caller is null)
        {
            throw new RpcException(new Status(StatusCode.Unauthenticated, "caller context is not available."));
        }

        if (!await requestStream.MoveNext().ConfigureAwait(false))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "consumer hello required."));
        }

        var hello = requestStream.Current?.Hello;
        if (hello is null)
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "first message must be hello."));
        }

        var tenantId = string.IsNullOrWhiteSpace(hello.TenantId) ? caller.TenantId : hello.TenantId.Trim();
        if (string.IsNullOrWhiteSpace(tenantId))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "tenant_id is required."));
        }

        var environmentTag = string.IsNullOrWhiteSpace(hello.EnvironmentTag) ? caller.EnvironmentTag : hello.EnvironmentTag?.Trim();
        if (string.IsNullOrWhiteSpace(environmentTag))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "environment_tag is required."));
        }

        EnsureTenantOrThrow(TenantGuard.EnsureTenant(_callerAccessor, tenantId, environmentTag, CroniqScopes.WebhooksIngress));

        var maxInflight = NormalizeMaxInflight(hello.MaxInflight);
        var scope = new PartitionScope(tenantId, environmentTag);

        activity?.SetTag("croniq.tenant_id", tenantId);
        activity?.SetTag("croniq.environment", environmentTag);
        activity?.SetTag("croniq.webhook.consumer_id", hello.ConsumerId);

        await responseStream.WriteAsync(new WebhookIngressServerMessage
        {
            Hello = new WebhookServerHello
            {
                ServerId = Environment.MachineName,
                TenantId = tenantId,
                EnvironmentTag = environmentTag,
                ServerTimeUtc = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
            }
        }).ConfigureAwait(false);

        var inflight = new ConcurrentDictionary<string, WebhookIngressLease>(StringComparer.OrdinalIgnoreCase);
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(context.CancellationToken);
        var assignmentLoop = Task.Run(() => AssignIngressLoopAsync(
            responseStream,
            inflight,
            scope,
            maxInflight,
            cts.Token), cts.Token);

        try
        {
            while (await requestStream.MoveNext().ConfigureAwait(false))
            {
                var message = requestStream.Current;
                if (message is null)
                {
                    continue;
                }

                await HandleClientMessageAsync(message, inflight, cts.Token).ConfigureAwait(false);
            }

            activity?.SetStatus(ActivityStatusCode.Ok);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Webhook ingress stream ended unexpectedly.");
            activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
        }
        finally
        {
            cts.Cancel();
            try
            {
                await assignmentLoop.ConfigureAwait(false);
            }
            catch
            {
                // ignore assignment errors on shutdown
            }
        }
    }

    private async Task AssignIngressLoopAsync(
        IServerStreamWriter<WebhookIngressServerMessage> responseStream,
        ConcurrentDictionary<string, WebhookIngressLease> inflight,
        PartitionScope scope,
        int maxInflight,
        CancellationToken cancellationToken)
    {
        var pollInterval = ResolvePollingInterval();
        var leaseDuration = ResolveLeaseDuration();

        while (!cancellationToken.IsCancellationRequested)
        {
            try
            {
                var now = DateTimeOffset.UtcNow;
                foreach (var pair in inflight)
                {
                    if (pair.Value.LeaseExpiresAtUtc <= now)
                    {
                        inflight.TryRemove(pair.Key, out _);
                    }
                }

                var available = maxInflight - inflight.Count;
                if (available > 0)
                {
                    var batchSize = Math.Min(available, ResolveBatchSize());
                    var leases = await _store.AcquireAsync(
                        new WebhookIngressAcquireRequest(scope, now, batchSize, leaseDuration),
                        cancellationToken).ConfigureAwait(false);

                    foreach (var lease in leases)
                    {
                        if (!inflight.TryAdd(lease.LeaseId, lease))
                        {
                            continue;
                        }

                        await responseStream.WriteAsync(BuildEventMessage(lease)).ConfigureAwait(false);
                    }
                }
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Webhook ingress assignment loop failed.");
            }

            try
            {
                await Task.Delay(pollInterval, cancellationToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private async Task HandleClientMessageAsync(
        WebhookIngressClientMessage message,
        ConcurrentDictionary<string, WebhookIngressLease> inflight,
        CancellationToken cancellationToken)
    {
        if (message.Ack is not null)
        {
            await HandleAckAsync(message.Ack, inflight, cancellationToken).ConfigureAwait(false);
            return;
        }

        if (message.Nack is not null)
        {
            await HandleNackAsync(message.Nack, inflight, cancellationToken).ConfigureAwait(false);
            return;
        }

        if (message.Extend is not null)
        {
            await HandleExtendAsync(message.Extend, inflight, cancellationToken).ConfigureAwait(false);
        }
    }

    private async Task HandleAckAsync(
        WebhookEventAck ack,
        ConcurrentDictionary<string, WebhookIngressLease> inflight,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(ack.LeaseId))
        {
            return;
        }

        if (!inflight.TryRemove(ack.LeaseId, out var lease))
        {
            _logger.LogWarning("Ack received for unknown webhook lease {LeaseId}.", ack.LeaseId);
            return;
        }

        if (!string.IsNullOrWhiteSpace(ack.EventId)
            && !string.Equals(ack.EventId, lease.EventId, StringComparison.OrdinalIgnoreCase))
        {
            _logger.LogWarning("Ack event id mismatch for lease {LeaseId}.", ack.LeaseId);
            return;
        }

        await _store.AcknowledgeAsync(
            new WebhookIngressAck(
                lease.EventId,
                lease.LeaseId,
                ack.Succeeded,
                ack.ErrorMessage,
                DateTimeOffset.UtcNow),
            cancellationToken).ConfigureAwait(false);
    }

    private async Task HandleNackAsync(
        WebhookEventNack nack,
        ConcurrentDictionary<string, WebhookIngressLease> inflight,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(nack.LeaseId))
        {
            return;
        }

        if (!inflight.TryRemove(nack.LeaseId, out var lease))
        {
            _logger.LogWarning("Nack received for unknown webhook lease {LeaseId}.", nack.LeaseId);
            return;
        }

        if (!string.IsNullOrWhiteSpace(nack.EventId)
            && !string.Equals(nack.EventId, lease.EventId, StringComparison.OrdinalIgnoreCase))
        {
            _logger.LogWarning("Nack event id mismatch for lease {LeaseId}.", nack.LeaseId);
            return;
        }

        await _store.NackAsync(
            new WebhookIngressNack(
                lease.EventId,
                lease.LeaseId,
                nack.Reason,
                DateTimeOffset.UtcNow),
            cancellationToken).ConfigureAwait(false);
    }

    private async Task HandleExtendAsync(
        WebhookEventExtend extend,
        ConcurrentDictionary<string, WebhookIngressLease> inflight,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(extend.LeaseId) || extend.LeaseExpiresAtUtc <= 0)
        {
            return;
        }

        if (!inflight.TryGetValue(extend.LeaseId, out var lease))
        {
            _logger.LogWarning("Lease extend received for unknown webhook lease {LeaseId}.", extend.LeaseId);
            return;
        }

        if (!string.IsNullOrWhiteSpace(extend.EventId)
            && !string.Equals(extend.EventId, lease.EventId, StringComparison.OrdinalIgnoreCase))
        {
            _logger.LogWarning("Lease extend event id mismatch for lease {LeaseId}.", extend.LeaseId);
            return;
        }

        var newExpiry = DateTimeOffset.FromUnixTimeMilliseconds(extend.LeaseExpiresAtUtc);
        var renewal = new WebhookIngressLeaseRenewal(lease.EventId, lease.LeaseId, newExpiry, DateTimeOffset.UtcNow);
        if (await _store.TryExtendLeaseAsync(renewal, cancellationToken).ConfigureAwait(false))
        {
            var updated = lease with { LeaseExpiresAtUtc = newExpiry };
            inflight.TryUpdate(lease.LeaseId, updated, lease);
        }
    }

    private static WebhookIngressServerMessage BuildEventMessage(WebhookIngressLease lease)
    {
        var message = new WebhookIngressServerMessage
        {
            Event = new WebhookIngressEvent
            {
                EventId = lease.EventId,
                LeaseId = lease.LeaseId,
                LeaseExpiresAtUtc = lease.LeaseExpiresAtUtc.ToUnixTimeMilliseconds(),
                HookKey = lease.HookKey,
                JobKey = lease.JobKey,
                Payload = lease.Payload ?? string.Empty,
                ReceivedAtUtc = lease.ReceivedAtUtc.ToUnixTimeMilliseconds()
            }
        };

        if (lease.Headers is not null)
        {
            foreach (var pair in lease.Headers)
            {
                message.Event.Headers[pair.Key] = pair.Value;
            }
        }

        if (lease.Metadata is not null)
        {
            foreach (var pair in lease.Metadata)
            {
                message.Event.Metadata[pair.Key] = pair.Value;
            }
        }

        return message;
    }

    private static void EnsureTenantOrThrow(IResult? failure)
    {
        if (failure is null)
        {
            return;
        }

        var status = StatusCode.PermissionDenied;
        var detail = "tenant_mismatch";

        if (failure is IStatusCodeHttpResult statusResult)
        {
            if (statusResult.StatusCode == StatusCodes.Status401Unauthorized)
            {
                status = StatusCode.Unauthenticated;
                detail = "unauthenticated";
            }
        }

        throw new RpcException(new Status(status, detail));
    }

    private static int NormalizeMaxInflight(int maxInflight)
    {
        if (maxInflight <= 0)
        {
            return 1;
        }

        return Math.Min(maxInflight, 250);
    }

    private TimeSpan ResolveLeaseDuration()
    {
        var seconds = Math.Clamp(_options.LeaseSeconds, 5, 600);
        return TimeSpan.FromSeconds(seconds);
    }

    private int ResolveBatchSize()
    {
        return Math.Clamp(_options.MaxBatchSize, 1, 500);
    }

    private TimeSpan ResolvePollingInterval()
    {
        return TimeSpan.FromMilliseconds(Math.Clamp(_options.PollingIntervalMilliseconds, 100, 5000));
    }
}
