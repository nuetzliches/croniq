using System;
using System.Collections.Concurrent;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Threading.Channels;
using Croniq.Rpc;
using RpcWorkEvent = Croniq.Rpc.WorkEvent;
using Grpc.Core;
using Grpc.Net.Client;
using Microsoft.Extensions.Logging;

namespace Croniq.Runner;

public sealed class CroniqRunner : IAsyncDisposable
{
    private static readonly JsonSerializerOptions SerializerOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    private readonly RunnerConfig _config;
    private readonly string _runnerInstanceId;
    private readonly HttpClient _httpClient;
    private readonly GrpcChannel _grpcChannel;
    private readonly Croniq.Rpc.Runner.RunnerClient _grpcClient;
    private readonly IRunnerLogger _logger;
    private readonly ConcurrentDictionary<string, LeaseState> _inflight = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentQueue<Lease> _queue = new();
    private readonly SemaphoreSlim _queueSignal = new(0);
    private readonly SemaphoreSlim _inflightLimiter;
    private readonly ConcurrentDictionary<string, HandlerRegistration> _handlers = new(StringComparer.OrdinalIgnoreCase);
    private readonly OutboxStore _outbox;
    private readonly string _pollPath;
    private readonly string _renewPath;
    private readonly string _ackPath;
    private readonly string _eventsPath;
    private readonly string _heartbeatPath;
    private readonly string _jobRegisterPath;
    private Channel<RunnerMessage>? _grpcOutbound;
    private volatile bool _grpcConnected;
    private volatile bool _running;
    private volatile bool _acceptingWork;
    private volatile bool _draining;
    private CancellationTokenSource? _runCts;
    private Exception? _fatal;

    public CroniqRunner(RunnerConfig config)
    {
        _config = Normalize(config);
        _runnerInstanceId = string.IsNullOrWhiteSpace(config.RunnerInstanceId)
            ? Guid.NewGuid().ToString("N")
            : config.RunnerInstanceId.Trim();

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);

        _httpClient = new HttpClient
        {
            BaseAddress = new Uri(_config.BaseUrl),
            Timeout = _config.RequestTimeout
        };
        ApplyAuthHeaders(_httpClient);

        var grpcBase = string.IsNullOrWhiteSpace(_config.GrpcBaseUrl) ? _config.BaseUrl : _config.GrpcBaseUrl!;
        var grpcHttpClient = new HttpClient
        {
            BaseAddress = new Uri(grpcBase),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        ApplyAuthHeaders(grpcHttpClient);

        _grpcChannel = GrpcChannel.ForAddress(grpcBase, new GrpcChannelOptions { HttpClient = grpcHttpClient });
        _grpcClient = new Croniq.Rpc.Runner.RunnerClient(_grpcChannel);

        _logger = config.Logger is null ? new ConsoleRunnerLogger() : new LoggerAdapter(config.Logger);

        _inflightLimiter = new SemaphoreSlim(_config.MaxInflight, _config.MaxInflight);
        _outbox = new OutboxStore(
            _config.OutboxPath ?? Path.Combine(Environment.CurrentDirectory, ".croniq", "runner-outbox.jsonl"),
            _config.OutboxMaxEntries,
            _config.OutboxMaxBytes);

        _pollPath = $"/tenants/{Uri.EscapeDataString(_config.TenantId)}/work/poll";
        _renewPath = $"/tenants/{Uri.EscapeDataString(_config.TenantId)}/work/renew";
        _ackPath = $"/tenants/{Uri.EscapeDataString(_config.TenantId)}/work/ack";
        _eventsPath = $"/tenants/{Uri.EscapeDataString(_config.TenantId)}/work";
        _heartbeatPath = $"/tenants/{Uri.EscapeDataString(_config.TenantId)}/runners/heartbeat";
        _jobRegisterPath = $"/tenants/{Uri.EscapeDataString(_config.TenantId)}/jobs:register";
    }

    public void OnExecute(string jobKey, RunnerExecuteHandler handler)
        => OnExecute(jobKey, handler, registration: null);

    public void OnExecute(string jobKey, RunnerExecuteHandler handler, RunnerJobRegistration? registration)
    {
        if (string.IsNullOrWhiteSpace(jobKey))
        {
            throw new ArgumentException("jobKey is required", nameof(jobKey));
        }
        if (handler is null)
        {
            throw new ArgumentNullException(nameof(handler));
        }

        if (!_handlers.TryAdd(jobKey.Trim(), new HandlerRegistration(handler, registration)))
        {
            throw new InvalidOperationException($"Handler already registered for jobKey '{jobKey}'.");
        }
    }

    public async Task StartAsync(CancellationToken cancellationToken = default)
    {
        if (_handlers.IsEmpty)
        {
            throw new InvalidOperationException("At least one onExecute handler must be registered.");
        }
        if (_running)
        {
            throw new InvalidOperationException("Runner is already running.");
        }

        _running = true;
        _acceptingWork = true;
        _draining = false;
        _fatal = null;

        _runCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        var token = _runCts.Token;

        await _outbox.LoadAsync(token).ConfigureAwait(false);
        await RegisterJobsAsync(token).ConfigureAwait(false);

        var tasks = new List<Task>
        {
            Task.Run(() => ProcessLoopAsync(token), token),
            Task.Run(() => ReplayOutboxLoopAsync(token), token)
        };

        if (_config.TransportMode != TransportMode.Polling)
        {
            tasks.Add(Task.Run(() => GrpcLoopAsync(token), token));
        }

        if (_config.TransportMode != TransportMode.Grpc)
        {
            tasks.Add(Task.Run(() => PollLoopAsync(token), token));
        }

        if (_config.HeartbeatInterval > TimeSpan.Zero)
        {
            tasks.Add(Task.Run(() => HeartbeatLoopAsync(token), token));
        }

        await Task.WhenAll(tasks).ConfigureAwait(false);
        if (_fatal is not null)
        {
            throw _fatal;
        }
    }

    public async Task DrainAsync(TimeSpan timeout)
    {
        if (!_running)
        {
            return;
        }

        _draining = true;
        await TrySendDrainHeartbeatAsync().ConfigureAwait(false);
        _acceptingWork = false;
        StopGrpc();

        var deadline = DateTimeOffset.UtcNow + (timeout <= TimeSpan.Zero ? TimeSpan.FromSeconds(30) : timeout);
        while (!_inflight.IsEmpty && DateTimeOffset.UtcNow < deadline)
        {
            await Task.Delay(100).ConfigureAwait(false);
        }

        if (!_inflight.IsEmpty || !_queue.IsEmpty)
        {
            await AbandonPendingLeasesAsync().ConfigureAwait(false);
        }

        _running = false;
        _runCts?.Cancel();
    }

    public Task StopAsync()
    {
        _acceptingWork = false;
        _running = false;
        StopGrpc();
        _runCts?.Cancel();
        return Task.CompletedTask;
    }

    private static RunnerConfig Normalize(RunnerConfig config)
    {
        if (config is null) throw new ArgumentNullException(nameof(config));
        if (string.IsNullOrWhiteSpace(config.BaseUrl)) throw new ArgumentException("BaseUrl is required", nameof(config));
        if (string.IsNullOrWhiteSpace(config.TenantId)) throw new ArgumentException("TenantId is required", nameof(config));
        if (string.IsNullOrWhiteSpace(config.Environment)) throw new ArgumentException("Environment is required", nameof(config));
        if (string.IsNullOrWhiteSpace(config.RunnerId)) throw new ArgumentException("RunnerId is required", nameof(config));

        var hasApiKey = !string.IsNullOrWhiteSpace(config.ApiKey);
        var hasBearer = !string.IsNullOrWhiteSpace(config.BearerToken);
        if (hasApiKey == hasBearer)
        {
            throw new ArgumentException("Set exactly one of ApiKey or BearerToken.", nameof(config));
        }

        return config with
        {
            MaxInflight = Math.Max(1, config.MaxInflight),
            PollBatchSize = Math.Max(1, config.PollBatchSize),
            PollWait = config.PollWait < TimeSpan.Zero ? TimeSpan.Zero : config.PollWait,
            RenewLead = config.RenewLead <= TimeSpan.Zero ? TimeSpan.FromSeconds(10) : config.RenewLead,
            RetryBase = config.RetryBase <= TimeSpan.Zero ? TimeSpan.FromMilliseconds(500) : config.RetryBase,
            RetryMax = config.RetryMax <= TimeSpan.Zero ? TimeSpan.FromSeconds(10) : config.RetryMax,
            OutboxMaxEntries = config.OutboxMaxEntries <= 0 ? 500 : config.OutboxMaxEntries,
            OutboxMaxBytes = config.OutboxMaxBytes <= 0 ? 1_000_000 : config.OutboxMaxBytes
        };
    }

    private void Enqueue(Lease lease)
    {
        _queue.Enqueue(lease);
        _queueSignal.Release();
    }

    private async Task ProcessLoopAsync(CancellationToken token)
    {
        try
        {
            while (!token.IsCancellationRequested)
            {
                await _queueSignal.WaitAsync(token).ConfigureAwait(false);
                if (token.IsCancellationRequested)
                {
                    break;
                }

                if (!_queue.TryDequeue(out var lease))
                {
                    continue;
                }

                await _inflightLimiter.WaitAsync(token).ConfigureAwait(false);
                _ = Task.Run(async () =>
                {
                    try
                    {
                        await ExecuteLeaseAsync(lease, token).ConfigureAwait(false);
                    }
                    finally
                    {
                        _inflightLimiter.Release();
                    }
                }, token);
            }
        }
        catch (OperationCanceledException)
        {
            // expected on shutdown
        }
    }

    private async Task ExecuteLeaseAsync(Lease lease, CancellationToken runnerToken)
    {
        var leaseCts = CancellationTokenSource.CreateLinkedTokenSource(runnerToken);
        var state = new LeaseState(lease, leaseCts);
        _inflight[lease.LeaseId] = state;

        var renewTask = ScheduleRenewAsync(state, runnerToken);

        try
        {
            if (!_handlers.TryGetValue(lease.JobKey, out var registration))
            {
                _logger.Warn("No handler for jobKey", new Dictionary<string, object?> { ["jobKey"] = lease.JobKey });
                await AckFailureInternalAsync(lease, "handler-not-found", "handler not registered", "handler-not-found", allowOutbox: true, runnerToken).ConfigureAwait(false);
                return;
            }

            if (!_config.AllowTestExecutions && string.Equals(lease.ExecutionMode, "test", StringComparison.OrdinalIgnoreCase))
            {
                await AckFailureInternalAsync(lease, "test-not-allowed", "runner policy disallows test executions", "test-not-allowed", allowOutbox: true, runnerToken).ConfigureAwait(false);
                return;
            }

            var context = new RunnerExecutionContext(
                lease.ExecutionId,
                lease.LeaseId,
                lease.TriggerId,
                lease.JobKey,
                lease.FireAtUtc,
                lease.LeaseExpiresAtUtc,
                lease.ExecutionMode,
                lease.InvocationSource,
                leaseCts.Token,
                evt => SendEventsAsync(lease, new[] { evt }, allowOutbox: true, runnerToken));

            var payload = _config.ParsePayloadJson ? TryParsePayload(lease.Payload) : lease.Payload;
            await registration.Handler(context, payload, _logger, leaseCts.Token).ConfigureAwait(false);

            if (!state.IsAbandoned)
            {
                await AckSuccessAsync(lease, allowOutbox: true, runnerToken).ConfigureAwait(false);
            }
        }
        catch (OperationCanceledException) when (state.IsAbandoned)
        {
            // ignored
        }
        catch (Exception ex)
        {
            if (!state.IsAbandoned)
            {
                await AckFailureAsync(lease, ex, allowOutbox: true, runnerToken).ConfigureAwait(false);
            }
        }
        finally
        {
            state.Complete();
            if (renewTask is not null)
            {
                try { await renewTask.ConfigureAwait(false); } catch { }
            }
            _inflight.TryRemove(lease.LeaseId, out _);
        }
    }

    private Task? ScheduleRenewAsync(LeaseState state, CancellationToken runnerToken)
    {
        if (state.IsAbandoned)
        {
            return null;
        }

        var delay = state.Lease.LeaseExpiresAtUtc - DateTimeOffset.UtcNow - _config.RenewLead;
        if (delay < TimeSpan.FromSeconds(1))
        {
            delay = TimeSpan.FromSeconds(1);
        }

        return Task.Run(async () =>
        {
            try
            {
                await Task.Delay(delay, state.Token).ConfigureAwait(false);
                if (state.IsAbandoned || state.Token.IsCancellationRequested)
                {
                    return;
                }

                var renewed = await RenewAsync(state.Lease, runnerToken).ConfigureAwait(false);
                if (renewed is not null)
                {
                    state.UpdateLease(renewed);
                    var next = ScheduleRenewAsync(state, runnerToken);
                    if (next is not null)
                    {
                        await next.ConfigureAwait(false);
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // ignored
            }
            catch (Exception ex)
            {
                if (HandleFatal(ex))
                {
                    return;
                }
                _logger.Warn("lease renew failed", new Dictionary<string, object?> { ["leaseId"] = state.Lease.LeaseId, ["error"] = ex.Message });
            }
        }, runnerToken);
    }

    private async Task PollLoopAsync(CancellationToken token)
    {
        var attempt = 0;
        while (!token.IsCancellationRequested)
        {
            if (!_acceptingWork)
            {
                await Task.Delay(TimeSpan.FromMilliseconds(100), token).ConfigureAwait(false);
                continue;
            }

            if (_config.TransportMode == TransportMode.Auto && _grpcConnected)
            {
                await Task.Delay(TimeSpan.FromMilliseconds(250), token).ConfigureAwait(false);
                continue;
            }

            try
            {
                var leases = await PollAsync(token).ConfigureAwait(false);
                attempt = 0;
                foreach (var lease in leases)
                {
                    if (!_acceptingWork)
                    {
                        break;
                    }
                    Enqueue(lease);
                }
            }
            catch (Exception ex)
            {
                if (HandleFatal(ex))
                {
                    return;
                }
                attempt++;
                var delay = NextDelay(attempt);
                _logger.Warn("poll failed", new Dictionary<string, object?> { ["error"] = ex.Message, ["delayMs"] = (int)delay.TotalMilliseconds });
                await Task.Delay(delay, token).ConfigureAwait(false);
            }
        }
    }

    private async Task GrpcLoopAsync(CancellationToken token)
    {
        var attempt = 0;
        while (!token.IsCancellationRequested)
        {
            try
            {
                await RunGrpcSessionAsync(token).ConfigureAwait(false);
                attempt = 0;
            }
            catch (Exception ex)
            {
                _grpcConnected = false;
                if (HandleFatal(ex))
                {
                    return;
                }
                attempt++;
                var delay = NextDelay(attempt);
                _logger.Warn("grpc connection failed", new Dictionary<string, object?> { ["error"] = ex.Message, ["delayMs"] = (int)delay.TotalMilliseconds });
                await Task.Delay(delay, token).ConfigureAwait(false);
            }
        }
    }

    private async Task RunGrpcSessionAsync(CancellationToken token)
    {
        using var call = _grpcClient.Connect(cancellationToken: token);
        var outbound = Channel.CreateUnbounded<RunnerMessage>();
        _grpcOutbound = outbound;

        var writerTask = Task.Run(async () =>
        {
            try
            {
                await foreach (var message in outbound.Reader.ReadAllAsync(token).ConfigureAwait(false))
                {
                    await call.RequestStream.WriteAsync(message).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException)
            {
                // ignore
            }
        }, token);

        var hello = new RunnerHello
        {
            RunnerId = _config.RunnerId,
            RunnerInstanceId = _runnerInstanceId,
            AllowTestExecutions = _config.AllowTestExecutions,
            MaxInflight = _config.MaxInflight
        };
        if (_config.Capabilities is { Length: > 0 })
        {
            foreach (var capability in _config.Capabilities)
            {
                if (!string.IsNullOrWhiteSpace(capability))
                {
                    hello.Capabilities[capability.Trim()] = "true";
                }
            }
        }

        await call.RequestStream.WriteAsync(new RunnerMessage { Hello = hello }).ConfigureAwait(false);

        try
        {
            while (await call.ResponseStream.MoveNext(token).ConfigureAwait(false))
            {
                var message = call.ResponseStream.Current;
                if (message is null)
                {
                    continue;
                }

                if (message.Hello is not null)
                {
                    _grpcConnected = true;
                    continue;
                }

                if (message.Assigned is null)
                {
                    continue;
                }

                if (!_acceptingWork)
                {
                    continue;
                }

                var assigned = message.Assigned;
                var lease = new Lease(
                    assigned.ExecutionId,
                    assigned.LeaseId,
                    assigned.TriggerId,
                    assigned.JobKey,
                    assigned.FireAtUtc > 0 ? DateTimeOffset.FromUnixTimeMilliseconds(assigned.FireAtUtc) : DateTimeOffset.UtcNow,
                    assigned.LeaseExpiresAtUtc > 0 ? DateTimeOffset.FromUnixTimeMilliseconds(assigned.LeaseExpiresAtUtc) : DateTimeOffset.UtcNow,
                    string.IsNullOrWhiteSpace(assigned.Payload) ? null : assigned.Payload,
                    string.IsNullOrWhiteSpace(assigned.ExecutionMode) ? null : assigned.ExecutionMode,
                    string.IsNullOrWhiteSpace(assigned.InvocationSource) ? null : assigned.InvocationSource);
                Enqueue(lease);
            }
        }
        catch (RpcException ex) when (IsGrpcMismatch(ex))
        {
            throw new RunnerMismatchException("runner mismatch", ex);
        }
        catch (RpcException ex) when (IsGrpcRunnerIdInUse(ex))
        {
            throw new RunnerIdInUseException("runner id already in use", ex);
        }
        finally
        {
            _grpcConnected = false;
            _grpcOutbound = null;
            outbound.Writer.TryComplete();
            try { await writerTask.ConfigureAwait(false); } catch { }
            try { await call.RequestStream.CompleteAsync().ConfigureAwait(false); } catch { }
        }
    }

    private async Task HeartbeatLoopAsync(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            try
            {
                await HeartbeatAsync(token).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                if (HandleFatal(ex))
                {
                    return;
                }
                _logger.Warn("heartbeat failed", new Dictionary<string, object?> { ["error"] = ex.Message });
            }

            try
            {
                await Task.Delay(_config.HeartbeatInterval, token).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                return;
            }
        }
    }

    private async Task ReplayOutboxLoopAsync(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            var entries = _outbox.Snapshot();
            foreach (var entry in entries)
            {
                if (token.IsCancellationRequested)
                {
                    return;
                }

                try
                {
                    await ReplayEntryAsync(entry, token).ConfigureAwait(false);
                    await _outbox.RemoveAsync(entry.Id, token).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                catch (Exception ex)
                {
                    if (HandleFatal(ex))
                    {
                        return;
                    }
                    await _outbox.MarkFailedAsync(entry.Id, token).ConfigureAwait(false);
                }
            }

            try
            {
                await Task.Delay(TimeSpan.FromSeconds(2), token).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                return;
            }
        }
    }

    private async Task ReplayEntryAsync(OutboxEntry entry, CancellationToken token)
    {
        switch (entry.Type)
        {
            case "ack_success":
                var ackSuccess = entry.Payload.Deserialize<OutboxAckSuccessPayload>(SerializerOptions);
                if (ackSuccess is not null)
                {
                    await AckSuccessAsync(ackSuccess.Lease, allowOutbox: false, token).ConfigureAwait(false);
                }
                break;
            case "ack_failure":
                var ackFailure = entry.Payload.Deserialize<OutboxAckFailurePayload>(SerializerOptions);
                if (ackFailure is not null)
                {
                    await AckFailureInternalAsync(
                        ackFailure.Lease,
                        ackFailure.ErrorType,
                        ackFailure.ErrorMessage,
                        ackFailure.DeadLetterReason,
                        allowOutbox: false,
                        token).ConfigureAwait(false);
                }
                break;
            case "events":
                var events = entry.Payload.Deserialize<OutboxEventsPayload>(SerializerOptions);
                if (events is not null)
                {
                    await SendEventsAsync(events.Lease, events.Events, allowOutbox: false, token).ConfigureAwait(false);
                }
                break;
        }
    }

    private async Task AbandonPendingLeasesAsync()
    {
        foreach (var state in _inflight.Values)
        {
            state.Abandon();
        }

        while (_queue.TryDequeue(out var queued))
        {
            await AckFailureInternalAsync(
                queued,
                "runner-shutdown",
                "runner shutting down",
                "runner-shutdown",
                allowOutbox: true,
                CancellationToken.None).ConfigureAwait(false);
        }

        foreach (var state in _inflight.Values)
        {
            await AckFailureInternalAsync(
                state.Lease,
                "runner-shutdown",
                "runner shutting down",
                "runner-shutdown",
                allowOutbox: true,
                CancellationToken.None).ConfigureAwait(false);
        }
    }

    private async Task<IReadOnlyList<Lease>> PollAsync(CancellationToken token)
    {
        var request = new WorkPollRequest
        {
            EnvironmentTag = _config.Environment,
            RunnerId = _config.RunnerId,
            RunnerInstanceId = _runnerInstanceId,
            BatchSize = _config.PollBatchSize,
            WaitForMs = (int)_config.PollWait.TotalMilliseconds,
            AllowTestExecutions = _config.AllowTestExecutions,
            MaxInflight = _config.MaxInflight,
            Capabilities = _config.Capabilities
        };

        var response = await SendJsonAsync(_pollPath, request, token).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            await ThrowForStatusAsync(response).ConfigureAwait(false);
        }

        var body = await response.Content.ReadAsStringAsync(token).ConfigureAwait(false);
        var parsed = JsonSerializer.Deserialize<WorkPollResponse>(body, SerializerOptions);
        if (parsed?.Leases is null)
        {
            return Array.Empty<Lease>();
        }

        return parsed.Leases.Select(MapLease).ToArray();
    }

    private async Task<Lease?> RenewAsync(Lease lease, CancellationToken token)
    {
        var request = new WorkRenewRequest
        {
            EnvironmentTag = _config.Environment,
            RunnerId = _config.RunnerId,
            Lease = MapLeaseToken(lease)
        };

        var response = await SendJsonAsync(_renewPath, request, token).ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.NotFound)
        {
            return null;
        }

        if (!response.IsSuccessStatusCode)
        {
            await ThrowForStatusAsync(response).ConfigureAwait(false);
        }

        var body = await response.Content.ReadAsStringAsync(token).ConfigureAwait(false);
        var parsed = JsonSerializer.Deserialize<WorkRenewResponse>(body, SerializerOptions);
        if (parsed?.Renewed != true || parsed.Lease is null)
        {
            return null;
        }

        return MapLease(parsed.Lease);
    }

    private async Task AckSuccessAsync(Lease lease, bool allowOutbox, CancellationToken token)
    {
        if (TrySendGrpc(new RunnerMessage
        {
            AckSuccess = new WorkAckSuccess
            {
                ExecutionId = lease.ExecutionId,
                LeaseId = lease.LeaseId
            }
        }))
        {
            return;
        }

        var request = new WorkAckRequest
        {
            EnvironmentTag = _config.Environment,
            RunnerId = _config.RunnerId,
            Lease = MapLeaseToken(lease),
            Succeeded = true
        };

        try
        {
            var response = await SendJsonAsync(_ackPath, request, token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                await ThrowForStatusAsync(response).ConfigureAwait(false);
            }
        }
        catch (Exception ex) when (allowOutbox && !HandleFatal(ex))
        {
            await _outbox.EnqueueAsync("ack_success", new OutboxAckSuccessPayload(lease), CancellationToken.None).ConfigureAwait(false);
        }
    }

    private async Task AckFailureAsync(Lease lease, Exception error, bool allowOutbox, CancellationToken token)
    {
        var message = error.Message;
        await AckFailureInternalAsync(lease, "execution-failed", message, "execution-failed", allowOutbox, token).ConfigureAwait(false);
    }

    private async Task AckFailureInternalAsync(
        Lease lease,
        string errorType,
        string errorMessage,
        string deadLetterReason,
        bool allowOutbox,
        CancellationToken token)
    {
        if (TrySendGrpc(new RunnerMessage
        {
            AckFailure = new WorkAckFailure
            {
                ExecutionId = lease.ExecutionId,
                LeaseId = lease.LeaseId,
                ErrorType = errorType,
                ErrorMessage = errorMessage,
                DeadLetterReason = deadLetterReason
            }
        }))
        {
            return;
        }

        var request = new WorkAckRequest
        {
            EnvironmentTag = _config.Environment,
            RunnerId = _config.RunnerId,
            Lease = MapLeaseToken(lease),
            Succeeded = false,
            DeadLetterReason = deadLetterReason
        };

        try
        {
            var response = await SendJsonAsync(_ackPath, request, token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                await ThrowForStatusAsync(response).ConfigureAwait(false);
            }
        }
        catch (Exception ex) when (allowOutbox && !HandleFatal(ex))
        {
            await _outbox.EnqueueAsync(
                "ack_failure",
                new OutboxAckFailurePayload(lease, errorType, errorMessage, deadLetterReason),
                CancellationToken.None).ConfigureAwait(false);
        }
    }

    private async Task SendEventsAsync(Lease lease, IEnumerable<WorkEvent> events, bool allowOutbox, CancellationToken token)
    {
        var payload = events.ToArray();
        if (payload.Length == 0)
        {
            return;
        }

        if (TrySendGrpc(new RunnerMessage
        {
            Events = new WorkEvents
            {
                ExecutionId = lease.ExecutionId,
                LeaseId = lease.LeaseId,
                Events = { payload.Select(MapEvent) }
            }
        }))
        {
            return;
        }

        var request = new WorkEventsRequest
        {
            EnvironmentTag = _config.Environment,
            RunnerId = _config.RunnerId,
            Lease = MapLeaseToken(lease),
            Events = payload.Select(MapEventEntry).ToArray()
        };

        try
        {
            var response = await SendJsonAsync($"{_eventsPath}/{Uri.EscapeDataString(lease.ExecutionId)}:events", request, token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                await ThrowForStatusAsync(response).ConfigureAwait(false);
            }
        }
        catch (Exception ex) when (allowOutbox && !HandleFatal(ex))
        {
            await _outbox.EnqueueAsync("events", new OutboxEventsPayload(lease, payload), CancellationToken.None).ConfigureAwait(false);
        }
    }

    private async Task HeartbeatAsync(CancellationToken token)
    {
        var metadata = BuildHeartbeatMetadata();
        var request = new RunnerHeartbeatRequest
        {
            EnvironmentTag = _config.Environment,
            RunnerId = _config.RunnerId,
            RunnerInstanceId = _runnerInstanceId,
            SeenAtUtc = DateTimeOffset.UtcNow,
            MetadataJson = metadata
        };

        var response = await SendJsonAsync(_heartbeatPath, request, token).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            await ThrowForStatusAsync(response).ConfigureAwait(false);
        }
    }

    private async Task RegisterJobsAsync(CancellationToken token)
    {
        if (!_config.RegisterJobs)
        {
            return;
        }

        foreach (var entry in _handlers)
        {
            var jobKey = entry.Key;
            var registration = entry.Value.Registration;
            var request = new RunnerJobRegistrationRequest
            {
                EnvironmentTag = _config.Environment,
                RunnerId = _config.RunnerId,
                RunnerInstanceId = _runnerInstanceId,
                JobKey = jobKey,
                Description = registration?.Description,
                Metadata = registration?.Metadata
            };

            var response = await SendJsonAsync(_jobRegisterPath, request, token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                await ThrowForStatusAsync(response).ConfigureAwait(false);
            }

            var body = await response.Content.ReadAsStringAsync(token).ConfigureAwait(false);
            var job = string.IsNullOrWhiteSpace(body)
                ? null
                : JsonSerializer.Deserialize<JobResponse>(body, SerializerOptions);

            if (job?.IsActive == false)
            {
                _logger.Warn("job registration pending approval", new Dictionary<string, object?> { ["jobKey"] = jobKey });
            }
            else
            {
                _logger.Info("job registration completed", new Dictionary<string, object?> { ["jobKey"] = jobKey });
            }
        }
    }

    private string BuildHeartbeatMetadata()
    {
        var data = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase)
        {
            ["runnerInstanceId"] = _runnerInstanceId,
            ["transportState"] = ResolveTransportState(),
            ["allowTestExecutions"] = _config.AllowTestExecutions,
            ["maxInflight"] = _config.MaxInflight,
            ["draining"] = _draining
        };

        if (_config.Capabilities is { Length: > 0 })
        {
            data["capabilities"] = _config.Capabilities;
        }

        if (_config.HeartbeatMetadata is not null)
        {
            foreach (var pair in _config.HeartbeatMetadata)
            {
                data[pair.Key] = pair.Value;
            }
        }

        return JsonSerializer.Serialize(data, SerializerOptions);
    }

    private async Task TrySendDrainHeartbeatAsync()
    {
        try
        {
            await HeartbeatAsync(CancellationToken.None).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.Warn("drain heartbeat failed", new Dictionary<string, object?> { ["error"] = ex.Message });
        }
    }

    private string ResolveTransportState()
    {
        return _config.TransportMode switch
        {
            TransportMode.Grpc => "grpc",
            TransportMode.Polling => "polling",
            _ => _grpcConnected ? "grpc" : "polling"
        };
    }

    private static RpcWorkEvent MapEvent(WorkEvent ev)
    {
        var evt = new RpcWorkEvent
        {
            Message = ev.Message,
            Level = ev.Level ?? string.Empty,
            TimestampUtc = ev.TimestampUtc?.ToUnixTimeMilliseconds() ?? 0,
            EventType = ev.EventType ?? string.Empty
        };
        if (ev.Properties is not null)
        {
            foreach (var pair in ev.Properties)
            {
                evt.Properties[pair.Key] = pair.Value;
            }
        }
        return evt;
    }

    private static WorkEventEntry MapEventEntry(WorkEvent ev)
        => new WorkEventEntry(ev.Message, ev.Level, ev.TimestampUtc, ev.Properties, ev.EventType);

    private static Lease MapLease(WorkLeaseToken token)
        => new(
            token.ExecutionId,
            token.LeaseId,
            token.TriggerId,
            token.JobKey,
            token.FireAtUtc,
            token.LeaseExpiresAtUtc,
            token.Payload,
            token.ExecutionMode,
            token.InvocationSource);

    private static WorkLeaseToken MapLeaseToken(Lease lease)
        => new(
            lease.ExecutionId,
            lease.LeaseId,
            lease.TriggerId,
            lease.JobKey,
            lease.FireAtUtc,
            lease.LeaseExpiresAtUtc,
            lease.Payload,
            lease.ExecutionMode ?? string.Empty,
            lease.InvocationSource ?? string.Empty);

    private object? TryParsePayload(string? payload)
    {
        if (string.IsNullOrWhiteSpace(payload))
        {
            return null;
        }

        try
        {
            return JsonSerializer.Deserialize<object>(payload, SerializerOptions);
        }
        catch (JsonException)
        {
            return payload;
        }
    }

    private async Task<HttpResponseMessage> SendJsonAsync(string path, object payload, CancellationToken token)
    {
        var body = JsonSerializer.Serialize(payload, SerializerOptions);
        using var request = new HttpRequestMessage(HttpMethod.Post, path)
        {
            Content = new StringContent(body, Encoding.UTF8, "application/json")
        };
        return await _httpClient.SendAsync(request, token).ConfigureAwait(false);
    }

    private static async Task ThrowForStatusAsync(HttpResponseMessage response)
    {
        var body = await response.Content.ReadAsStringAsync().ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.Forbidden && ContainsProblemTitle(body, "runner-mismatch"))
        {
            throw new RunnerMismatchException("RunnerId must match the authenticated caller identity.");
        }
        if (response.StatusCode == HttpStatusCode.Forbidden && ContainsProblemTitle(body, "runner-registration-denied"))
        {
            throw new RunnerJobRegistrationDeniedException("Runner self-registration is denied by policy.");
        }
        if (response.StatusCode == HttpStatusCode.Conflict && ContainsProblemTitle(body, "runner-id-in-use"))
        {
            throw new RunnerIdInUseException("RunnerId is already in use by another active runner instance.");
        }
        if (response.StatusCode == HttpStatusCode.Conflict && ContainsProblemTitle(body, "lease-conflict"))
        {
            throw new LeaseConflictException("Lease conflict.");
        }

        response.EnsureSuccessStatusCode();
    }

    private static bool ContainsProblemTitle(string body, string expected)
    {
        if (string.IsNullOrWhiteSpace(body))
        {
            return false;
        }

        try
        {
            using var document = JsonDocument.Parse(body);
            if (document.RootElement.ValueKind != JsonValueKind.Object)
            {
                return false;
            }

            if (document.RootElement.TryGetProperty("title", out var title)
                && title.ValueKind == JsonValueKind.String
                && string.Equals(title.GetString(), expected, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }

            if (document.RootElement.TryGetProperty("error", out var error)
                && error.ValueKind == JsonValueKind.String
                && string.Equals(error.GetString(), expected, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }
        catch (JsonException)
        {
            return false;
        }

        return false;
    }

    private static bool IsGrpcMismatch(RpcException ex)
        => ex.StatusCode == StatusCode.PermissionDenied && string.Equals(ex.Status.Detail, "runner-mismatch", StringComparison.OrdinalIgnoreCase);

    private static bool IsGrpcRunnerIdInUse(RpcException ex)
        => ex.StatusCode == StatusCode.AlreadyExists && string.Equals(ex.Status.Detail, "runner-id-in-use", StringComparison.OrdinalIgnoreCase);

    private bool HandleFatal(Exception ex)
    {
        if (ex is RunnerMismatchException or RunnerIdInUseException)
        {
            Fail(ex);
            return true;
        }
        return false;
    }

    private void Fail(Exception ex)
    {
        _fatal ??= ex;
        _runCts?.Cancel();
    }

    private bool TrySendGrpc(RunnerMessage message)
    {
        var outbound = _grpcOutbound;
        if (outbound is null)
        {
            return false;
        }

        return outbound.Writer.TryWrite(message);
    }

    private void StopGrpc()
    {
        _grpcOutbound?.Writer.TryComplete();
        _grpcOutbound = null;
    }

    private static TimeSpan NextDelay(int attempt, TimeSpan? baseDelay = null, TimeSpan? maxDelay = null)
    {
        var baseMs = baseDelay?.TotalMilliseconds ?? 500;
        var maxMs = maxDelay?.TotalMilliseconds ?? 10000;
        var scale = Math.Min(maxMs, baseMs * Math.Pow(2, Math.Min(attempt, 6)));
        var jitter = 0.5 + Random.Shared.NextDouble();
        return TimeSpan.FromMilliseconds(scale * jitter);
    }

    private TimeSpan NextDelay(int attempt)
        => NextDelay(attempt, _config.RetryBase, _config.RetryMax);

    private void ApplyAuthHeaders(HttpClient client)
    {
        if (!string.IsNullOrWhiteSpace(_config.ApiKey))
        {
            client.DefaultRequestHeaders.Add("X-Croniq-Key", _config.ApiKey);
            return;
        }

        if (!string.IsNullOrWhiteSpace(_config.BearerToken))
        {
            client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", _config.BearerToken);
        }
    }

    public async ValueTask DisposeAsync()
    {
        _runCts?.Cancel();
        _httpClient.Dispose();
        _grpcChannel.Dispose();
        await Task.CompletedTask;
    }

    private sealed class LeaseState
    {
        private readonly CancellationTokenSource _cts;
        private bool _abandoned;

        public LeaseState(Lease lease, CancellationTokenSource cts)
        {
            Lease = lease;
            _cts = cts;
        }

        public Lease Lease { get; private set; }
        public CancellationToken Token => _cts.Token;
        public bool IsAbandoned => _abandoned;

        public void UpdateLease(Lease lease) => Lease = lease;

        public void Abandon()
        {
            _abandoned = true;
            _cts.Cancel();
        }

        public void Complete()
        {
            _cts.Dispose();
        }
    }

    private sealed record WorkPollRequest
    {
        public string? EnvironmentTag { get; init; }
        public string RunnerId { get; init; } = string.Empty;
        public string? RunnerInstanceId { get; init; }
        public int? BatchSize { get; init; }
        public int? WaitForMs { get; init; }
        public bool? AllowTestExecutions { get; init; }
        public int? MaxInflight { get; init; }
        public string[]? Capabilities { get; init; }
    }

    private sealed record WorkLeaseToken(
        string ExecutionId,
        string LeaseId,
        string TriggerId,
        string JobKey,
        DateTimeOffset FireAtUtc,
        DateTimeOffset LeaseExpiresAtUtc,
        string? Payload,
        string ExecutionMode,
        string InvocationSource);

    private sealed record WorkPollResponse(WorkLeaseToken[] Leases);

    private sealed record WorkRenewRequest
    {
        public string? EnvironmentTag { get; init; }
        public string RunnerId { get; init; } = string.Empty;
        public WorkLeaseToken Lease { get; init; } = null!;
    }

    private sealed record WorkRenewResponse(bool Renewed, WorkLeaseToken? Lease);

    private sealed record WorkAckRequest
    {
        public string? EnvironmentTag { get; init; }
        public string RunnerId { get; init; } = string.Empty;
        public WorkLeaseToken Lease { get; init; } = null!;
        public bool Succeeded { get; init; }
        public DateTimeOffset? NextFireTimeUtc { get; init; }
        public string? DeadLetterReason { get; init; }
    }

    private sealed record WorkEventsRequest
    {
        public string? EnvironmentTag { get; init; }
        public string RunnerId { get; init; } = string.Empty;
        public WorkLeaseToken Lease { get; init; } = null!;
        public WorkEventEntry[]? Events { get; init; }
    }

    private sealed record WorkEventEntry(
        string Message,
        string? Level = null,
        DateTimeOffset? TimestampUtc = null,
        IReadOnlyDictionary<string, string>? Properties = null,
        string? EventType = null);

    private sealed record RunnerHeartbeatRequest
    {
        public string? EnvironmentTag { get; init; }
        public string RunnerId { get; init; } = string.Empty;
        public string? RunnerInstanceId { get; init; }
        public DateTimeOffset? SeenAtUtc { get; init; }
        public string? MetadataJson { get; init; }
    }

    private sealed record RunnerJobRegistrationRequest
    {
        public string? EnvironmentTag { get; init; }
        public string RunnerId { get; init; } = string.Empty;
        public string? RunnerInstanceId { get; init; }
        public string JobKey { get; init; } = string.Empty;
        public string? Description { get; init; }
        public IReadOnlyDictionary<string, string>? Metadata { get; init; }
    }

    private sealed record JobResponse(string JobKey, bool IsActive);

    private sealed record OutboxAckSuccessPayload(Lease Lease);
    private sealed record OutboxAckFailurePayload(Lease Lease, string ErrorType, string ErrorMessage, string DeadLetterReason);
    private sealed record OutboxEventsPayload(Lease Lease, WorkEvent[] Events);

    private sealed record HandlerRegistration(RunnerExecuteHandler Handler, RunnerJobRegistration? Registration);
}
