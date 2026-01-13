using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Observability;
using Croniq.Core.Policies;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Croniq.Webhooks.Options;
using Grpc.Net.Client;
using Grpc.Core;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Webhooks.Relay;

internal sealed class WebhookIngressRelayService : BackgroundService
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Webhooks.Ingress.Relay");
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private const int PollWaitMaxMs = 30_000;
    private readonly IOptionsMonitor<CroniqWebhookOptions> _webhookOptions;
    private readonly CroniqOptions _coreOptions;
    private readonly IJobRegistry _jobRegistry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly IPolicyResolver _policyResolver;
    private readonly IExecutionLogStore _executionLogStore;
    private readonly ILogger<WebhookIngressRelayService> _logger;

    public WebhookIngressRelayService(
        IOptionsMonitor<CroniqWebhookOptions> webhookOptions,
        IOptions<CroniqOptions> coreOptions,
        IJobRegistry jobRegistry,
        IJobExecutionPipeline pipeline,
        IPolicyResolver policyResolver,
        IExecutionLogStore executionLogStore,
        ILogger<WebhookIngressRelayService> logger)
    {
        _webhookOptions = webhookOptions ?? throw new ArgumentNullException(nameof(webhookOptions));
        _coreOptions = coreOptions?.Value ?? throw new ArgumentNullException(nameof(coreOptions));
        _jobRegistry = jobRegistry ?? throw new ArgumentNullException(nameof(jobRegistry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _executionLogStore = executionLogStore ?? throw new ArgumentNullException(nameof(executionLogStore));
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
        var fallbackMode = remote.StreamFallback;

        if (remote.AllowInvalidServerCertificate)
        {
            _logger.LogWarning("Webhook ingress relay is configured to skip TLS certificate validation; use only in trusted environments.");
        }

        if (remote.StreamMode == WebhookIngressStreamMode.Grpc || fallbackMode == WebhookIngressStreamMode.Grpc)
        {
            AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        }

        var currentMode = remote.StreamMode;
        var fallbackActivated = false;

        while (!stoppingToken.IsCancellationRequested)
        {
            var failed = false;
            try
            {
                await ConnectAndRunAsync(remote, currentMode, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex) when (IsExpectedDisconnect(ex, stoppingToken))
            {
                break;
            }
            catch (Exception ex)
            {
                failed = true;
                _logger.LogWarning(ex, "Webhook ingress relay failed; reconnecting in {Delay}.", reconnectDelay);
            }

            if (failed
                && !fallbackActivated
                && fallbackMode.HasValue
                && fallbackMode.Value != currentMode)
            {
                currentMode = fallbackMode.Value;
                fallbackActivated = true;
                _logger.LogWarning("Webhook ingress relay switching to {Fallback} after {Primary} failure.", currentMode, remote.StreamMode);
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
    private Task ConnectAndRunAsync(WebhookRemoteOptions remote, WebhookIngressStreamMode mode, CancellationToken stoppingToken)
    {
        return mode switch
        {
            WebhookIngressStreamMode.Grpc => ConnectAndRunGrpcAsync(remote, stoppingToken),
            WebhookIngressStreamMode.Sse => ConnectAndRunSseAsync(remote, stoppingToken),
            WebhookIngressStreamMode.Polling => ConnectAndRunPollingAsync(remote, stoppingToken),
            _ => throw new InvalidOperationException($"Unsupported webhook ingress stream mode '{mode}'.")
        };
    }

    private async Task ConnectAndRunGrpcAsync(WebhookRemoteOptions remote, CancellationToken stoppingToken)
    {
        if (!TryResolveRemote(remote, out var endpoint, out var apiKey))
        {
            return;
        }

        if (!TryResolveScope(out var scope))
        {
            return;
        }

        using var httpClient = BuildGrpcHttpClient(endpoint, apiKey, remote.TimeoutSeconds, remote.AllowInvalidServerCertificate);
        using var channel = GrpcChannel.ForAddress(endpoint, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new WebhookIngress.WebhookIngressClient(channel);

        using var call = client.Connect(cancellationToken: stoppingToken);

        var maxInflight = NormalizeMaxInflight(remote.MaxInflight);
        var consumerId = ResolveConsumerId();

        var hashedTenantId = IdentifierHashing.HashTenantId(scope.TenantId) ?? string.Empty;
        _logger.LogInformation(
            "Webhook ingress relay connected to {Endpoint} for {Tenant}/{Environment} (max inflight {MaxInflight}, mode {Mode}).",
            endpoint, hashedTenantId, scope.EnvironmentTag, maxInflight, WebhookIngressStreamMode.Grpc);

        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = consumerId,
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

                var entry = MapGrpcEvent(message.Event);
                var actions = new IngressActions(
                    (succeeded, error, ct) => SendGrpcAckAsync(outbound.Writer, entry, succeeded, error, ct),
                    (leaseExpiresAtUtc, ct) => SendGrpcExtendAsync(outbound.Writer, entry, leaseExpiresAtUtc, ct));

                await semaphore.WaitAsync(stoppingToken).ConfigureAwait(false);
                tasks.Add(Task.Run(async () =>
                {
                    try
                    {
                        await ProcessEventAsync(entry, scope, actions, stoppingToken).ConfigureAwait(false);
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
    private async Task ConnectAndRunSseAsync(WebhookRemoteOptions remote, CancellationToken stoppingToken)
    {
        if (!TryResolveRemote(remote, out var endpoint, out var apiKey))
        {
            return;
        }

        if (!TryResolveScope(out var scope))
        {
            return;
        }

        var maxInflight = NormalizeMaxInflight(remote.MaxInflight);
        var consumerId = ResolveConsumerId();

        using var streamClient = BuildHttpClient(endpoint, apiKey, Timeout.InfiniteTimeSpan, remote.AllowInvalidServerCertificate);
        using var controlClient = BuildHttpClient(endpoint, apiKey, TimeSpan.FromSeconds(Math.Max(1, remote.TimeoutSeconds)), remote.AllowInvalidServerCertificate);

        var streamUrl = BuildIngressUrl("stream", scope, new Dictionary<string, string>
        {
            ["consumerId"] = consumerId,
            ["maxInflight"] = maxInflight.ToString(),
            ["maxBatchSize"] = maxInflight.ToString()
        });

        using var request = new HttpRequestMessage(HttpMethod.Get, streamUrl);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("text/event-stream"));

        using var response = await streamClient
            .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, stoppingToken)
            .ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        var hashedTenantId = IdentifierHashing.HashTenantId(scope.TenantId) ?? string.Empty;
        _logger.LogInformation(
            "Webhook ingress relay connected to {Endpoint} for {Tenant}/{Environment} (max inflight {MaxInflight}, mode {Mode}).",
            endpoint, hashedTenantId, scope.EnvironmentTag, maxInflight, WebhookIngressStreamMode.Sse);

        var tasks = new List<Task>();
        using var semaphore = new SemaphoreSlim(maxInflight, maxInflight);

        await foreach (var entry in ReadSseEventsAsync(response.Content, stoppingToken).ConfigureAwait(false))
        {
            var actions = CreateHttpActions(controlClient, scope, consumerId, entry);

            await semaphore.WaitAsync(stoppingToken).ConfigureAwait(false);
            tasks.Add(Task.Run(async () =>
            {
                try
                {
                    await ProcessEventAsync(entry, scope, actions, stoppingToken).ConfigureAwait(false);
                }
                finally
                {
                    semaphore.Release();
                }
            }, stoppingToken));
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
    private async Task ConnectAndRunPollingAsync(WebhookRemoteOptions remote, CancellationToken stoppingToken)
    {
        if (!TryResolveRemote(remote, out var endpoint, out var apiKey))
        {
            return;
        }

        if (!TryResolveScope(out var scope))
        {
            return;
        }

        var maxInflight = NormalizeMaxInflight(remote.MaxInflight);
        var consumerId = ResolveConsumerId();
        var pollWaitMs = Math.Clamp((remote.TimeoutSeconds * 1000) - 1000, 0, PollWaitMaxMs);

        using var httpClient = BuildHttpClient(endpoint, apiKey, TimeSpan.FromSeconds(Math.Max(1, remote.TimeoutSeconds)), remote.AllowInvalidServerCertificate);

        var hashedTenantId = IdentifierHashing.HashTenantId(scope.TenantId) ?? string.Empty;
        _logger.LogInformation(
            "Webhook ingress relay polling {Endpoint} for {Tenant}/{Environment} (max inflight {MaxInflight}, mode {Mode}).",
            endpoint, hashedTenantId, scope.EnvironmentTag, maxInflight, WebhookIngressStreamMode.Polling);

        var tasks = new List<Task>();
        using var semaphore = new SemaphoreSlim(maxInflight, maxInflight);

        while (!stoppingToken.IsCancellationRequested)
        {
            var entries = await PollIngressAsync(httpClient, scope, maxInflight, pollWaitMs, stoppingToken).ConfigureAwait(false);
            if (entries.Count == 0)
            {
                continue;
            }

            foreach (var entry in entries)
            {
                var actions = CreateHttpActions(httpClient, scope, consumerId, entry);
                await semaphore.WaitAsync(stoppingToken).ConfigureAwait(false);
                tasks.Add(Task.Run(async () =>
                {
                    try
                    {
                        await ProcessEventAsync(entry, scope, actions, stoppingToken).ConfigureAwait(false);
                    }
                    finally
                    {
                        semaphore.Release();
                    }
                }, stoppingToken));
            }
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
    private async Task<IReadOnlyCollection<IngressEvent>> PollIngressAsync(
        HttpClient httpClient,
        PartitionScope scope,
        int maxBatchSize,
        int waitMs,
        CancellationToken cancellationToken)
    {
        var query = new Dictionary<string, string>
        {
            ["maxBatchSize"] = maxBatchSize.ToString()
        };

        if (waitMs > 0)
        {
            query["waitMs"] = waitMs.ToString();
        }

        var url = BuildIngressUrl("poll", scope, query);
        var response = await httpClient
            .GetFromJsonAsync<WebhookIngressPollResponseDto>(url, JsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (response?.Events is null || response.Events.Length == 0)
        {
            return Array.Empty<IngressEvent>();
        }

        var list = new List<IngressEvent>(response.Events.Length);
        foreach (var entry in response.Events)
        {
            if (TryMapIngressEvent(entry, out var mapped))
            {
                list.Add(mapped);
            }
        }

        return list;
    }

    private IngressActions CreateHttpActions(HttpClient httpClient, PartitionScope scope, string consumerId, IngressEvent entry)
    {
        return new IngressActions(
            (succeeded, error, ct) => SafeSendHttpAckAsync(httpClient, scope, consumerId, entry, succeeded, error, ct),
            (leaseExpiresAtUtc, ct) => SafeSendHttpExtendAsync(httpClient, scope, consumerId, entry, leaseExpiresAtUtc, ct));
    }

    private async Task SafeSendHttpAckAsync(
        HttpClient httpClient,
        PartitionScope scope,
        string consumerId,
        IngressEvent entry,
        bool succeeded,
        string? errorMessage,
        CancellationToken cancellationToken)
    {
        try
        {
            var url = BuildIngressUrl("ack", scope);
            var payload = new WebhookIngressAckRequestDto(
                entry.EventId,
                entry.LeaseId,
                succeeded,
                errorMessage,
                consumerId);

            using var response = await httpClient
                .PostAsJsonAsync(url, payload, JsonOptions, cancellationToken)
                .ConfigureAwait(false);
            response.EnsureSuccessStatusCode();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Webhook ingress relay failed to acknowledge event {EventId}.", entry.EventId);
        }
    }

    private async Task SafeSendHttpExtendAsync(
        HttpClient httpClient,
        PartitionScope scope,
        string consumerId,
        IngressEvent entry,
        long leaseExpiresAtUtc,
        CancellationToken cancellationToken)
    {
        try
        {
            var url = BuildIngressUrl("extend", scope);
            var payload = new WebhookIngressExtendRequestDto(
                entry.EventId,
                entry.LeaseId,
                leaseExpiresAtUtc,
                consumerId);

            using var response = await httpClient
                .PostAsJsonAsync(url, payload, JsonOptions, cancellationToken)
                .ConfigureAwait(false);
            response.EnsureSuccessStatusCode();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Webhook ingress relay failed to extend lease {LeaseId}.", entry.LeaseId);
        }
    }
    private async IAsyncEnumerable<IngressEvent> ReadSseEventsAsync(
        HttpContent content,
        [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        await using var stream = await content.ReadAsStreamAsync(cancellationToken).ConfigureAwait(false);
        using var reader = new StreamReader(stream, Encoding.UTF8);
        var dataBuilder = new StringBuilder();

        while (!cancellationToken.IsCancellationRequested)
        {
            string? line;
            try
            {
                line = await reader.ReadLineAsync().WaitAsync(cancellationToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                yield break;
            }

            if (line is null)
            {
                yield break;
            }

            if (line.Length == 0)
            {
                if (dataBuilder.Length > 0)
                {
                    var json = dataBuilder.ToString();
                    dataBuilder.Clear();
                    if (TryParseIngressEvent(json, out var entry))
                    {
                        yield return entry;
                    }
                }

                continue;
            }

            if (line.StartsWith("data:", StringComparison.Ordinal))
            {
                var data = line[5..];
                if (data.StartsWith(" ", StringComparison.Ordinal))
                {
                    data = data[1..];
                }

                if (dataBuilder.Length > 0)
                {
                    dataBuilder.Append('\n');
                }

                dataBuilder.Append(data);
            }
        }
    }

    private bool TryParseIngressEvent(string json, out IngressEvent entry)
    {
        entry = default!;
        if (string.IsNullOrWhiteSpace(json))
        {
            return false;
        }

        try
        {
            var payload = JsonSerializer.Deserialize<WebhookIngressEventDto>(json, JsonOptions);
            if (payload is null)
            {
                return false;
            }

            return TryMapIngressEvent(payload, out entry);
        }
        catch (JsonException ex)
        {
            _logger.LogDebug(ex, "Webhook ingress relay failed to parse SSE payload.");
            return false;
        }
    }

    private static bool TryMapIngressEvent(WebhookIngressEventDto dto, out IngressEvent entry)
    {
        entry = default!;
        if (dto is null
            || string.IsNullOrWhiteSpace(dto.EventId)
            || string.IsNullOrWhiteSpace(dto.LeaseId))
        {
            return false;
        }

        entry = new IngressEvent(
            dto.EventId,
            dto.LeaseId,
            dto.LeaseExpiresAtUtc,
            dto.HookKey ?? string.Empty,
            dto.JobKey ?? string.Empty,
            dto.Payload ?? string.Empty,
            NormalizeDictionary(dto.Headers),
            NormalizeDictionary(dto.Metadata),
            dto.ReceivedAtUtc);
        return true;
    }

    private static IngressEvent MapGrpcEvent(WebhookIngressEvent entry)
    {
        return new IngressEvent(
            entry.EventId,
            entry.LeaseId,
            entry.LeaseExpiresAtUtc,
            entry.HookKey ?? string.Empty,
            entry.JobKey ?? string.Empty,
            entry.Payload ?? string.Empty,
            NormalizeDictionary(entry.Headers),
            NormalizeDictionary(entry.Metadata),
            entry.ReceivedAtUtc);
    }
    private async Task ProcessEventAsync(
        IngressEvent entry,
        PartitionScope scope,
        IngressActions actions,
        CancellationToken cancellationToken)
    {
        using var activity = ActivitySource.StartActivity("Croniq.Webhooks.Ingress.Relay", ActivityKind.Client);
        activity?.SetTag("croniq.webhook.event_id", entry.EventId);
        activity?.SetTag("croniq.webhook.hook_key", entry.HookKey);
        activity?.SetTag("croniq.job.key", entry.JobKey);
        activity?.SetTag("croniq.tenant_id", IdentifierHashing.HashTenantId(scope.TenantId));
        activity?.SetTag("croniq.environment", scope.EnvironmentTag);

        using var leaseCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        var extendLoop = StartLeaseExtensionLoop(entry, actions.ExtendAsync, leaseCts.Token);

        try
        {
            if (!JobKey.TryParse(entry.JobKey, out var jobKey))
            {
                await actions.AckAsync(false, "invalid-job-key", cancellationToken).ConfigureAwait(false);
                return;
            }

            if (!_jobRegistry.TryGet(jobKey, out var descriptor))
            {
                await actions.AckAsync(false, "job-not-registered", cancellationToken).ConfigureAwait(false);
                return;
            }

            var metadata = NormalizeDictionary(entry.Metadata);
            var executionOptions = _policyResolver.ResolveExecution(jobKey, scope);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, ActivitySource);
            var startedAtUtc = DateTimeOffset.UtcNow;
            var fireAtUtc = entry.ReceivedAtUtc > 0
                ? DateTimeOffset.FromUnixTimeMilliseconds(entry.ReceivedAtUtc)
                : startedAtUtc;
            await TryStoreExecutionStartedAsync(new ExecutionRecord(
                executionId,
                ExecutionKind.Job,
                WorkflowId: null,
                jobKey.Value,
                scope.TenantId,
                scope.EnvironmentTag,
                TriggerId: null,
                FireAtUtc: fireAtUtc,
                StartedAtUtc: startedAtUtc,
                _coreOptions.InstanceId,
                activity?.TraceId.ToString(),
                activity?.SpanId.ToString(),
                TryGetCorrelationId(activity, metadata)), cancellationToken).ConfigureAwait(false);

            var stopwatch = Stopwatch.StartNew();
            try
            {
                await _pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                stopwatch.Stop();
                activity?.SetStatus(ActivityStatusCode.Ok);
                await TryStoreExecutionCompletedAsync(
                    executionId,
                    ExecutionStatus.Succeeded,
                    stopwatch.Elapsed.TotalMilliseconds,
                    error: null,
                    cancellationToken).ConfigureAwait(false);
                await actions.AckAsync(true, null, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                stopwatch.Stop();
                var canceled = IsCancellation(ex, cancellationToken);
                await TryStoreExecutionCompletedAsync(
                    executionId,
                    canceled ? ExecutionStatus.Canceled : ExecutionStatus.Failed,
                    stopwatch.Elapsed.TotalMilliseconds,
                    canceled ? null : ex,
                    cancellationToken).ConfigureAwait(false);
                throw;
            }
        }
        catch (Exception ex)
        {
            activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
            _logger.LogError(ex, "Webhook ingress relay failed for event {EventId}.", entry.EventId);
            await actions.AckAsync(false, ex.Message, cancellationToken).ConfigureAwait(false);
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
        IngressEvent entry,
        Func<long, CancellationToken, Task>? extendAsync,
        CancellationToken cancellationToken)
    {
        if (extendAsync is null || entry.LeaseExpiresAtUtc <= 0)
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
                await extendAsync(expiresAt.ToUnixTimeMilliseconds(), cancellationToken).ConfigureAwait(false);
            }
        }, cancellationToken);
    }

    private static async Task SendGrpcAckAsync(
        ChannelWriter<WebhookIngressClientMessage> outbound,
        IngressEvent entry,
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

    private static async Task SendGrpcExtendAsync(
        ChannelWriter<WebhookIngressClientMessage> outbound,
        IngressEvent entry,
        long leaseExpiresAtUtc,
        CancellationToken cancellationToken)
    {
        await TryWriteAsync(outbound, new WebhookIngressClientMessage
        {
            Extend = new WebhookEventExtend
            {
                EventId = entry.EventId,
                LeaseId = entry.LeaseId,
                LeaseExpiresAtUtc = leaseExpiresAtUtc
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
    private static IReadOnlyDictionary<string, string>? NormalizeDictionary(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0)
        {
            return null;
        }

        return new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);
    }

    private static bool IsCancellation(Exception exception, CancellationToken cancellationToken)
        => cancellationToken.IsCancellationRequested && exception is OperationCanceledException;

    private static string? TryGetCorrelationId(Activity? activity, IReadOnlyDictionary<string, string>? metadata)
    {
        if (activity?.GetBaggageItem("croniq.correlation_id") is { Length: > 0 } baggageCorrelation)
        {
            return baggageCorrelation;
        }

        if (activity?.GetTagItem("croniq.correlation_id") is string tagCorrelation && !string.IsNullOrWhiteSpace(tagCorrelation))
        {
            return tagCorrelation;
        }

        if (metadata is not null && metadata.TryGetValue("correlation_id", out var value) && !string.IsNullOrWhiteSpace(value))
        {
            return value;
        }

        return null;
    }

    private async Task TryStoreExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken)
    {
        try
        {
            await _executionLogStore.OnExecutionStartedAsync(record, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to persist execution start for {ExecutionId}", record.ExecutionId);
        }
    }

    private async Task TryStoreExecutionCompletedAsync(
        string executionId,
        ExecutionStatus status,
        double? durationMs,
        Exception? error,
        CancellationToken cancellationToken)
    {
        try
        {
            var completion = new ExecutionCompletion(
                executionId,
                DateTimeOffset.UtcNow,
                status,
                durationMs,
                error?.GetType().FullName ?? error?.GetType().Name,
                error?.Message);

            await _executionLogStore.OnExecutionCompletedAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to persist execution completion for {ExecutionId}", executionId);
        }
    }
    private static HttpClient BuildGrpcHttpClient(Uri endpoint, string apiKey, int timeoutSeconds, bool allowInvalidServerCertificate)
    {
        var handler = BuildHttpClientHandler(allowInvalidServerCertificate);
        var client = new HttpClient(handler)
        {
            BaseAddress = endpoint,
            Timeout = TimeSpan.FromSeconds(Math.Max(1, timeoutSeconds))
        };

        client.DefaultRequestVersion = new Version(2, 0);
        client.DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher;
        client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
        return client;
    }

    private static HttpClient BuildHttpClient(Uri endpoint, string apiKey, TimeSpan timeout, bool allowInvalidServerCertificate)
    {
        var handler = BuildHttpClientHandler(allowInvalidServerCertificate);
        var client = new HttpClient(handler)
        {
            BaseAddress = endpoint,
            Timeout = timeout
        };

        client.DefaultRequestVersion = HttpVersion.Version11;
        client.DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrLower;
        client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
        return client;
    }

    private static HttpClientHandler BuildHttpClientHandler(bool allowInvalidServerCertificate)
    {
        var handler = new HttpClientHandler();
        if (allowInvalidServerCertificate)
        {
            handler.ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator;
        }

        return handler;
    }

    private static int NormalizeMaxInflight(int maxInflight)
    {
        if (maxInflight <= 0)
        {
            return 1;
        }

        return Math.Min(maxInflight, 250);
    }

    private string ResolveConsumerId()
    {
        var instanceId = _coreOptions.InstanceId?.Trim();
        return string.IsNullOrWhiteSpace(instanceId)
            ? Environment.MachineName
            : instanceId;
    }

    private bool TryResolveScope(out PartitionScope scope)
    {
        scope = default;
        var tenantId = _coreOptions.TenantId?.Trim();
        var environmentTag = _coreOptions.EnvironmentTag?.Trim();

        if (string.IsNullOrWhiteSpace(tenantId) || string.IsNullOrWhiteSpace(environmentTag))
        {
            _logger.LogWarning("Webhook ingress relay scope is not configured.");
            return false;
        }

        scope = new PartitionScope(tenantId, environmentTag);
        return true;
    }

    private bool TryResolveRemote(WebhookRemoteOptions remote, out Uri endpoint, out string apiKey)
    {
        endpoint = new Uri("http://localhost");
        apiKey = string.Empty;

        var baseUrl = remote.BaseUrl?.Trim();
        if (string.IsNullOrWhiteSpace(baseUrl))
        {
            _logger.LogWarning("Webhook ingress relay base URL is not configured.");
            return false;
        }

        if (!Uri.TryCreate(baseUrl, UriKind.Absolute, out var parsed) || parsed is null)
        {
            _logger.LogWarning("Webhook ingress relay base URL is not configured.");
            return false;
        }

        endpoint = parsed;
        apiKey = remote.ApiKey?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(apiKey))
        {
            _logger.LogWarning("Webhook ingress relay API key is not configured.");
            return false;
        }

        return true;
    }

    private static string BuildIngressUrl(string path, PartitionScope scope, IReadOnlyDictionary<string, string>? extraQuery = null)
    {
        var builder = new StringBuilder($"tenants/{Escape(scope.TenantId)}/webhooks/ingress/{path}");
        var hasQuery = false;

        void Append(string key, string? value)
        {
            if (string.IsNullOrWhiteSpace(key) || string.IsNullOrWhiteSpace(value))
            {
                return;
            }

            builder.Append(hasQuery ? '&' : '?');
            builder.Append(Escape(key));
            builder.Append('=');
            builder.Append(Escape(value));
            hasQuery = true;
        }

        Append("environment", scope.EnvironmentTag);
        if (extraQuery is not null)
        {
            foreach (var entry in extraQuery)
            {
                Append(entry.Key, entry.Value);
            }
        }

        return builder.ToString();
    }

    private static string Escape(string value) => Uri.EscapeDataString(value);

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

        reason = "enabled";
        return true;
    }

    private static bool IsExpectedDisconnect(Exception exception, CancellationToken stoppingToken)
    {
        if (!stoppingToken.IsCancellationRequested)
        {
            return false;
        }

        if (exception is OperationCanceledException)
        {
            return true;
        }

        if (exception is RpcException rpcException)
        {
            return rpcException.StatusCode == StatusCode.Cancelled
                || rpcException.StatusCode == StatusCode.Unavailable
                || rpcException.StatusCode == StatusCode.DeadlineExceeded;
        }

        if (exception is AggregateException aggregateException
            && aggregateException.InnerExceptions.Count == 1)
        {
            return IsExpectedDisconnect(aggregateException.InnerExceptions[0], stoppingToken);
        }

        return false;
    }
    private sealed record IngressEvent(
        string EventId,
        string LeaseId,
        long LeaseExpiresAtUtc,
        string HookKey,
        string JobKey,
        string Payload,
        IReadOnlyDictionary<string, string>? Headers,
        IReadOnlyDictionary<string, string>? Metadata,
        long ReceivedAtUtc);

    private sealed record IngressActions(
        Func<bool, string?, CancellationToken, Task> AckAsync,
        Func<long, CancellationToken, Task>? ExtendAsync);

    private sealed record WebhookIngressEventDto(
        string EventId,
        string LeaseId,
        long LeaseExpiresAtUtc,
        string HookKey,
        string JobKey,
        string Payload,
        Dictionary<string, string>? Headers,
        long ReceivedAtUtc,
        Dictionary<string, string>? Metadata);

    private sealed record WebhookIngressPollResponseDto(
        WebhookIngressEventDto[] Events,
        long ServerTimeUtc);

    private sealed record WebhookIngressAckRequestDto(
        string EventId,
        string LeaseId,
        bool Succeeded,
        string? ErrorMessage,
        string? ConsumerId);

    private sealed record WebhookIngressExtendRequestDto(
        string EventId,
        string LeaseId,
        long LeaseExpiresAtUtc,
        string? ConsumerId);
}
