using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Net.Http;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Croniq.Webhooks.Options;
using Grpc.Net.Client;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Webhooks.Relay;

internal sealed class WebhookIngressRelayService : BackgroundService
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Webhooks.Ingress.Relay");
    private readonly IOptionsMonitor<CroniqWebhookOptions> _webhookOptions;
    private readonly CroniqOptions _coreOptions;
    private readonly IJobRegistry _jobRegistry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly IPolicyResolver _policyResolver;
    private readonly ILogger<WebhookIngressRelayService> _logger;

    public WebhookIngressRelayService(
        IOptionsMonitor<CroniqWebhookOptions> webhookOptions,
        IOptions<CroniqOptions> coreOptions,
        IJobRegistry jobRegistry,
        IJobExecutionPipeline pipeline,
        IPolicyResolver policyResolver,
        ILogger<WebhookIngressRelayService> logger)
    {
        _webhookOptions = webhookOptions ?? throw new ArgumentNullException(nameof(webhookOptions));
        _coreOptions = coreOptions?.Value ?? throw new ArgumentNullException(nameof(coreOptions));
        _jobRegistry = jobRegistry ?? throw new ArgumentNullException(nameof(jobRegistry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        var options = _webhookOptions.CurrentValue;
        if (!ShouldRun(options, out var reason))
        {
            _logger.LogInformation("Webhook ingress relay disabled: {Reason}", reason);
            return;
        }

        var remote = options.Remote ?? new WebhookRemoteOptions();
        var reconnectDelay = TimeSpan.FromSeconds(Math.Max(1, remote.ReconnectDelaySeconds));

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);

        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                await ConnectAndRunAsync(remote, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Webhook ingress relay failed; reconnecting in {Delay}.", reconnectDelay);
            }

            try
            {
                await Task.Delay(reconnectDelay, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private async Task ConnectAndRunAsync(WebhookRemoteOptions remote, CancellationToken stoppingToken)
    {
        var baseUrl = remote.BaseUrl?.Trim();
        if (string.IsNullOrWhiteSpace(baseUrl) || !Uri.TryCreate(baseUrl, UriKind.Absolute, out var endpoint))
        {
            _logger.LogWarning("Webhook ingress relay base URL is not configured.");
            return;
        }

        var apiKey = remote.ApiKey?.Trim();
        if (string.IsNullOrWhiteSpace(apiKey))
        {
            _logger.LogWarning("Webhook ingress relay API key is not configured.");
            return;
        }

        using var httpClient = BuildGrpcHttpClient(endpoint, apiKey, remote.TimeoutSeconds);
        using var channel = GrpcChannel.ForAddress(endpoint, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new WebhookIngress.WebhookIngressClient(channel);

        using var call = client.Connect(cancellationToken: stoppingToken);

        var scope = new PartitionScope(_coreOptions.TenantId.Trim(), _coreOptions.EnvironmentTag.Trim());
        var maxInflight = NormalizeMaxInflight(remote.MaxInflight);

        _logger.LogInformation("Webhook ingress relay connected to {Endpoint} for {Tenant}/{Environment} (max inflight {MaxInflight}).",
            endpoint, scope.TenantId, scope.EnvironmentTag, maxInflight);

        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = _coreOptions.InstanceId,
                MaxInflight = maxInflight,
                TenantId = scope.TenantId,
                EnvironmentTag = scope.EnvironmentTag
            }
        }).ConfigureAwait(false);

        var outbound = Channel.CreateUnbounded<WebhookIngressClientMessage>();
        var sendLoop = Task.Run(async () =>
        {
            await foreach (var message in outbound.Reader.ReadAllAsync(stoppingToken).ConfigureAwait(false))
            {
                await call.RequestStream.WriteAsync(message).ConfigureAwait(false);
            }
        }, stoppingToken);

        var tasks = new List<Task>();
        using var semaphore = new SemaphoreSlim(maxInflight, maxInflight);

        try
        {
            while (await call.ResponseStream.MoveNext(stoppingToken).ConfigureAwait(false))
            {
                var message = call.ResponseStream.Current;
                if (message?.Event is null)
                {
                    continue;
                }

                await semaphore.WaitAsync(stoppingToken).ConfigureAwait(false);
                tasks.Add(Task.Run(async () =>
                {
                    try
                    {
                        await ProcessEventAsync(message.Event, scope, outbound.Writer, stoppingToken).ConfigureAwait(false);
                    }
                    finally
                    {
                        semaphore.Release();
                    }
                }, stoppingToken));
            }
        }
        catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
        {
            // ignore cancellation
        }
        finally
        {
            outbound.Writer.TryComplete();
            try
            {
                await sendLoop.ConfigureAwait(false);
            }
            catch
            {
                // ignore send failures on shutdown
            }

            try
            {
                await call.RequestStream.CompleteAsync().ConfigureAwait(false);
            }
            catch
            {
                // ignore completion failures on shutdown
            }

            try
            {
                await Task.WhenAll(tasks).ConfigureAwait(false);
            }
            catch
            {
                // ignore processing failures on shutdown
            }
        }
    }

    private async Task ProcessEventAsync(
        WebhookIngressEvent entry,
        PartitionScope scope,
        ChannelWriter<WebhookIngressClientMessage> outbound,
        CancellationToken cancellationToken)
    {
        using var activity = ActivitySource.StartActivity("Croniq.Webhooks.Ingress.Relay", ActivityKind.Client);
        activity?.SetTag("croniq.webhook.event_id", entry.EventId);
        activity?.SetTag("croniq.webhook.hook_key", entry.HookKey);
        activity?.SetTag("croniq.job.key", entry.JobKey);
        activity?.SetTag("croniq.tenant_id", scope.TenantId);
        activity?.SetTag("croniq.environment", scope.EnvironmentTag);

        using var leaseCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        var extendLoop = StartLeaseExtensionLoop(entry, outbound, leaseCts.Token);

        try
        {
            if (!JobKey.TryParse(entry.JobKey, out var jobKey))
            {
                await SendAckAsync(outbound, entry, succeeded: false, "invalid-job-key", cancellationToken).ConfigureAwait(false);
                return;
            }

            if (!_jobRegistry.TryGet(jobKey, out var descriptor))
            {
                await SendAckAsync(outbound, entry, succeeded: false, "job-not-registered", cancellationToken).ConfigureAwait(false);
                return;
            }

            var metadata = BuildMetadata(entry.Metadata);
            var executionOptions = _policyResolver.ResolveExecution(jobKey, scope);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, ActivitySource);

            await _pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
            activity?.SetStatus(ActivityStatusCode.Ok);
            await SendAckAsync(outbound, entry, succeeded: true, null, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
            _logger.LogError(ex, "Webhook ingress relay failed for event {EventId}.", entry.EventId);
            await SendAckAsync(outbound, entry, succeeded: false, ex.Message, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            leaseCts.Cancel();
            try
            {
                await extendLoop.ConfigureAwait(false);
            }
            catch
            {
                // ignore extension failures after completion
            }
        }
    }

    private static Task StartLeaseExtensionLoop(
        WebhookIngressEvent entry,
        ChannelWriter<WebhookIngressClientMessage> outbound,
        CancellationToken cancellationToken)
    {
        if (entry.LeaseExpiresAtUtc <= 0)
        {
            return Task.CompletedTask;
        }

        var initialExpiry = DateTimeOffset.FromUnixTimeMilliseconds(entry.LeaseExpiresAtUtc);
        var leaseDuration = initialExpiry - DateTimeOffset.UtcNow;
        if (leaseDuration <= TimeSpan.Zero || leaseDuration > TimeSpan.FromMinutes(10))
        {
            leaseDuration = TimeSpan.FromSeconds(30);
        }

        return Task.Run(async () =>
        {
            var expiresAt = initialExpiry;
            var renewBuffer = TimeSpan.FromSeconds(5);

            while (!cancellationToken.IsCancellationRequested)
            {
                var renewAt = expiresAt - renewBuffer;
                var delay = renewAt - DateTimeOffset.UtcNow;
                if (delay < TimeSpan.Zero)
                {
                    delay = TimeSpan.FromSeconds(1);
                }

                await Task.Delay(delay, cancellationToken).ConfigureAwait(false);
                if (cancellationToken.IsCancellationRequested)
                {
                    break;
                }

                expiresAt = DateTimeOffset.UtcNow.Add(leaseDuration);
                await TryWriteAsync(outbound, new WebhookIngressClientMessage
                {
                    Extend = new WebhookEventExtend
                    {
                        EventId = entry.EventId,
                        LeaseId = entry.LeaseId,
                        LeaseExpiresAtUtc = expiresAt.ToUnixTimeMilliseconds()
                    }
                }, cancellationToken).ConfigureAwait(false);
            }
        }, cancellationToken);
    }

    private static async Task SendAckAsync(
        ChannelWriter<WebhookIngressClientMessage> outbound,
        WebhookIngressEvent entry,
        bool succeeded,
        string? errorMessage,
        CancellationToken cancellationToken)
    {
        await TryWriteAsync(outbound, new WebhookIngressClientMessage
        {
            Ack = new WebhookEventAck
            {
                EventId = entry.EventId,
                LeaseId = entry.LeaseId,
                Succeeded = succeeded,
                ErrorMessage = errorMessage ?? string.Empty
            }
        }, cancellationToken).ConfigureAwait(false);
    }

    private static async Task TryWriteAsync(
        ChannelWriter<WebhookIngressClientMessage> outbound,
        WebhookIngressClientMessage message,
        CancellationToken cancellationToken)
    {
        if (outbound.TryWrite(message))
        {
            return;
        }

        try
        {
            await outbound.WriteAsync(message, cancellationToken).ConfigureAwait(false);
        }
        catch (ChannelClosedException)
        {
            // ignore writes after shutdown
        }
    }

    private static IReadOnlyDictionary<string, string>? BuildMetadata(
        Google.Protobuf.Collections.MapField<string, string> metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        return new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);
    }

    private static HttpClient BuildGrpcHttpClient(Uri endpoint, string apiKey, int timeoutSeconds)
    {
        var client = new HttpClient
        {
            BaseAddress = endpoint,
            Timeout = TimeSpan.FromSeconds(Math.Max(1, timeoutSeconds))
        };

        client.DefaultRequestVersion = new Version(2, 0);
        client.DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher;
        client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
        return client;
    }

    private static int NormalizeMaxInflight(int maxInflight)
    {
        if (maxInflight <= 0)
        {
            return 1;
        }

        return Math.Min(maxInflight, 250);
    }

    private static bool ShouldRun(CroniqWebhookOptions options, out string reason)
    {
        if (options.Mode != WebhookPersistenceMode.Remote)
        {
            reason = "Croniq:Webhooks:Mode is not Remote.";
            return false;
        }

        var remote = options.Remote ?? new WebhookRemoteOptions();
        if (!remote.EnableRelay)
        {
            reason = "Croniq:Webhooks:Remote:EnableRelay is false.";
            return false;
        }

        if (remote.StreamMode != WebhookIngressStreamMode.Grpc)
        {
            reason = $"Croniq:Webhooks:Remote:StreamMode '{remote.StreamMode}' is not supported yet.";
            return false;
        }

        reason = "enabled";
        return true;
    }
}
