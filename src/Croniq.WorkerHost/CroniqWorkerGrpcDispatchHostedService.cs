using System;
using System.Diagnostics.Metrics;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Hosting;
using Croniq.Core.Observability;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Hosting;

/// <summary>
/// Background service that uses Worker.Connect for gRPC-first dispatch with polling fallback.
/// </summary>
public sealed class CroniqWorkerGrpcDispatchHostedService : BackgroundService, IWorkerDispatchStatusProvider
{
    private static readonly Meter Meter = new("Croniq.WorkerHost.Dispatch");
    private static readonly Counter<long> GrpcReconnects = Meter.CreateCounter<long>(
        "croniq.worker_dispatch.grpc.reconnects",
        description: "Number of gRPC worker dispatch reconnect attempts.");
    private static readonly Counter<long> FallbackActivations = Meter.CreateCounter<long>(
        "croniq.worker_dispatch.fallback.activations",
        description: "Number of fallback polling activations.");
    private readonly TriggerWorker _worker;
    private readonly Worker.WorkerClient _client;
    private readonly WorkerDispatchOptions _dispatchOptions;
    private readonly WorkerHostOptions _hostOptions;
    private readonly CroniqOptions _coreOptions;
    private readonly CroniqStartupOptions _startupOptions;
    private readonly ILogger<CroniqWorkerGrpcDispatchHostedService> _logger;
    private int _grpcConnected;
    private long _lastConnectedTicks;
    private long _lastFallbackTicks;

    public CroniqWorkerGrpcDispatchHostedService(
        TriggerWorker worker,
        Worker.WorkerClient client,
        IOptions<WorkerDispatchOptions> dispatchOptions,
        IOptions<WorkerHostOptions> hostOptions,
        IOptions<CroniqOptions> coreOptions,
        IOptions<CroniqStartupOptions> startupOptions,
        ILogger<CroniqWorkerGrpcDispatchHostedService> logger)
    {
        _worker = worker ?? throw new ArgumentNullException(nameof(worker));
        _client = client ?? throw new ArgumentNullException(nameof(client));
        _dispatchOptions = dispatchOptions?.Value ?? new WorkerDispatchOptions();
        _hostOptions = hostOptions?.Value ?? throw new ArgumentNullException(nameof(hostOptions));
        _coreOptions = coreOptions?.Value ?? throw new ArgumentNullException(nameof(coreOptions));
        _startupOptions = startupOptions?.Value ?? new CroniqStartupOptions();
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        Meter.CreateObservableGauge(
            "croniq.worker_dispatch.grpc.connected",
            () => _grpcConnected,
            unit: "state",
            description: "1 when the worker dispatch gRPC stream is connected; otherwise 0.");
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        var startupMode = ResolveStartupMode(_startupOptions.Mode);
        if (startupMode == CroniqStartupMode.Validate)
        {
            _logger.LogInformation("Croniq startup mode is Validate; gRPC worker dispatch is disabled.");
            return;
        }

        if (!_dispatchOptions.EnableGrpc)
        {
            _logger.LogInformation("Croniq worker gRPC dispatch is disabled.");
            return;
        }

        var runnerId = ResolveRunnerId();
        var reconnectDelay = NormalizeDelay(_dispatchOptions.ReconnectDelay, TimeSpan.FromSeconds(5));

        while (!stoppingToken.IsCancellationRequested)
        {
            var sessionEnded = false;
            try
            {
                await RunGrpcSessionAsync(runnerId, stoppingToken).ConfigureAwait(false);
                sessionEnded = true;
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Croniq worker gRPC dispatch failed; falling back to polling.");
            }

            if (sessionEnded && !stoppingToken.IsCancellationRequested)
            {
                _logger.LogWarning("Croniq worker gRPC dispatch stream ended; falling back to polling.");
            }

            if (_dispatchOptions.EnablePollingFallback)
            {
                GrpcReconnects.Add(1);
                await RunFallbackPollingAsync(reconnectDelay, stoppingToken).ConfigureAwait(false);
            }

            if (reconnectDelay > TimeSpan.Zero)
            {
                try
                {
                    await Task.Delay(reconnectDelay, stoppingToken).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
                {
                    break;
                }
            }
        }
    }

    private async Task RunGrpcSessionAsync(string runnerId, CancellationToken stoppingToken)
    {
        _grpcConnected = 0;
        var maxInflight = _dispatchOptions.MaxInflight > 0
            ? _dispatchOptions.MaxInflight
            : Math.Max(_hostOptions.BatchSize, 1);

        using var call = _client.Connect(cancellationToken: stoppingToken);

        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = runnerId,
                MaxInflight = maxInflight
            }
        }).ConfigureAwait(false);

        var outbound = Channel.CreateBounded<RunnerMessage>(new BoundedChannelOptions(Math.Max(16, maxInflight * 4))
        {
            SingleReader = true,
            SingleWriter = false,
            FullMode = BoundedChannelFullMode.Wait
        });

        var dispatchSink = new WorkerGrpcDispatchSink(outbound.Writer);
        var writeTask = WriteLoopAsync(call.RequestStream, outbound.Reader, stoppingToken);
        var scope = default(PartitionScope);
        var limiter = new SemaphoreSlim(maxInflight, maxInflight);

        using var sessionCts = CancellationTokenSource.CreateLinkedTokenSource(stoppingToken);
        var sessionToken = sessionCts.Token;

        try
        {
            while (await call.ResponseStream.MoveNext(sessionToken).ConfigureAwait(false))
            {
                var message = call.ResponseStream.Current;
                if (message?.Hello is not null)
                {
                    scope = new PartitionScope(message.Hello.TenantId, message.Hello.EnvironmentTag);
                    _grpcConnected = 1;
                    _lastConnectedTicks = DateTimeOffset.UtcNow.UtcTicks;
                    _logger.LogInformation(
                        "Connected to worker dispatch gRPC (tenant {Tenant}, environment {Environment}).",
                        IdentifierHashing.HashTenantId(message.Hello.TenantId) ?? string.Empty,
                        message.Hello.EnvironmentTag);
                    continue;
                }

                if (message?.Assigned is null)
                {
                    continue;
                }

                if (string.IsNullOrWhiteSpace(scope.TenantId) || string.IsNullOrWhiteSpace(scope.EnvironmentTag))
                {
                    _logger.LogWarning("Worker dispatch received assignment before server hello; ignoring assignment.");
                    continue;
                }

                _ = ProcessAssignmentAsync(message.Assigned, scope, dispatchSink, limiter, sessionToken);
            }
        }
        catch (RpcException ex) when (ex.StatusCode == StatusCode.Cancelled && stoppingToken.IsCancellationRequested)
        {
            // expected during shutdown
        }
        finally
        {
            _grpcConnected = 0;
            sessionCts.Cancel();
            outbound.Writer.TryComplete();
            try
            {
                await writeTask.ConfigureAwait(false);
            }
            catch
            {
                // ignore writer loop failures on shutdown
            }

            try
            {
                await call.RequestStream.CompleteAsync().ConfigureAwait(false);
            }
            catch
            {
                // ignore completion failures
            }
        }
    }

    private async Task RunFallbackPollingAsync(TimeSpan window, CancellationToken stoppingToken)
    {
        if (window <= TimeSpan.Zero)
        {
            return;
        }

        FallbackActivations.Add(1);
        _lastFallbackTicks = DateTimeOffset.UtcNow.UtcTicks;
        _logger.LogInformation(
            "Croniq worker fallback polling active for {WindowSeconds} seconds.",
            window.TotalSeconds);
        var deadline = DateTimeOffset.UtcNow.Add(window);
        while (!stoppingToken.IsCancellationRequested && DateTimeOffset.UtcNow < deadline)
        {
            try
            {
                var processed = await _worker.ProcessBatchAsync(DateTimeOffset.UtcNow, _hostOptions.BatchSize, stoppingToken).ConfigureAwait(false);
                var delay = processed == 0
                    ? _dispatchOptions.FallbackIdleDelay
                    : _dispatchOptions.FallbackBusyDelay;
                if (delay > TimeSpan.Zero)
                {
                    await Task.Delay(delay, stoppingToken).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Croniq worker fallback polling failed; retrying shortly.");
                if (_dispatchOptions.FallbackErrorDelay > TimeSpan.Zero)
                {
                    await Task.Delay(_dispatchOptions.FallbackErrorDelay, stoppingToken).ConfigureAwait(false);
                }
            }
        }

        _logger.LogInformation("Croniq worker fallback polling complete; retrying gRPC dispatch.");
    }

    private async Task ProcessAssignmentAsync(
        WorkAssigned assigned,
        PartitionScope scope,
        ITriggerWorkerDispatchSink dispatchSink,
        SemaphoreSlim limiter,
        CancellationToken stoppingToken)
    {
        try
        {
            await limiter.WaitAsync(stoppingToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return;
        }

        _ = Task.Run(async () =>
        {
            try
            {
                var lease = MapLease(assigned, scope);
                await _worker.ProcessLeaseAsync(lease, DateTimeOffset.UtcNow, dispatchSink, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                // ignore cancellations during shutdown
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Failed to process assigned work {LeaseId}.", assigned.LeaseId);
            }
            finally
            {
                limiter.Release();
            }
        }, stoppingToken);
    }

    private static TriggerLease MapLease(WorkAssigned assigned, PartitionScope scope)
    {
        var fireAt = assigned.FireAtUtc > 0
            ? DateTimeOffset.FromUnixTimeMilliseconds(assigned.FireAtUtc)
            : DateTimeOffset.UtcNow;
        var leaseExpiresAt = assigned.LeaseExpiresAtUtc > 0
            ? DateTimeOffset.FromUnixTimeMilliseconds(assigned.LeaseExpiresAtUtc)
            : DateTimeOffset.UtcNow;

        return new TriggerLease(
            assigned.LeaseId,
            assigned.TriggerId,
            assigned.JobKey,
            scope,
            fireAt,
            leaseExpiresAt,
            string.IsNullOrWhiteSpace(assigned.Payload) ? null : assigned.Payload,
            string.IsNullOrWhiteSpace(assigned.ExecutionId) ? null : assigned.ExecutionId);
    }

    private static TimeSpan NormalizeDelay(TimeSpan value, TimeSpan fallback)
        => value < TimeSpan.Zero ? fallback : value;

    private string ResolveRunnerId()
        => string.IsNullOrWhiteSpace(_dispatchOptions.RunnerId)
            ? _coreOptions.InstanceId
            : _dispatchOptions.RunnerId.Trim();

    private static CroniqStartupMode ResolveStartupMode(string? mode)
    {
        if (string.IsNullOrWhiteSpace(mode))
        {
            return CroniqStartupMode.Run;
        }

        if (Enum.TryParse<CroniqStartupMode>(mode, ignoreCase: true, out var parsed))
        {
            return parsed;
        }

        throw new InvalidOperationException($"Croniq startup mode '{mode}' is invalid. Valid values: Run, Validate.");
    }

    private static async Task WriteLoopAsync(
        IClientStreamWriter<RunnerMessage> requestStream,
        ChannelReader<RunnerMessage> reader,
        CancellationToken cancellationToken)
    {
        while (await reader.WaitToReadAsync(cancellationToken).ConfigureAwait(false))
        {
            while (reader.TryRead(out var message))
            {
                await requestStream.WriteAsync(message, cancellationToken).ConfigureAwait(false);
            }
        }
    }

    private sealed class WorkerGrpcDispatchSink : ITriggerWorkerDispatchSink
    {
        private readonly ChannelWriter<RunnerMessage> _writer;

        public WorkerGrpcDispatchSink(ChannelWriter<RunnerMessage> writer)
        {
            _writer = writer ?? throw new ArgumentNullException(nameof(writer));
        }

        public Task OnAssignmentAsync(TriggerLease lease, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task OnExecutionStartedAsync(
            string executionId,
            TriggerLease lease,
            Croniq.Core.Jobs.JobKey jobKey,
            System.Diagnostics.Activity? activity,
            CancellationToken cancellationToken)
            => Task.CompletedTask;

        public Task OnExecutionCompletedAsync(
            string executionId,
            ExecutionStatus status,
            double? durationMs,
            Exception? error,
            CancellationToken cancellationToken)
            => Task.CompletedTask;

        public async Task OnReleaseAsync(TriggerReleaseRequest release, CancellationToken cancellationToken)
        {
            if (release.Succeeded)
            {
                await _writer.WriteAsync(new RunnerMessage
                {
                    AckSuccess = new WorkAckSuccess
                    {
                        ExecutionId = release.Lease.ExecutionId ?? string.Empty,
                        LeaseId = release.Lease.LeaseId
                    }
                }, cancellationToken).ConfigureAwait(false);
                return;
            }

            await _writer.WriteAsync(new RunnerMessage
            {
                AckFailure = new WorkAckFailure
                {
                    ExecutionId = release.Lease.ExecutionId ?? string.Empty,
                    LeaseId = release.Lease.LeaseId,
                    DeadLetterReason = release.DeadLetterReason ?? string.Empty,
                    NextFireTimeUtc = release.NextFireTimeUtc?.ToUnixTimeMilliseconds() ?? 0
                }
            }, cancellationToken).ConfigureAwait(false);
        }
    }

    public WorkerDispatchStatus GetStatus()
    {
        return new WorkerDispatchStatus(
            _grpcConnected == 1,
            _lastConnectedTicks > 0 ? new DateTimeOffset(_lastConnectedTicks, TimeSpan.Zero) : null,
            _lastFallbackTicks > 0 ? new DateTimeOffset(_lastFallbackTicks, TimeSpan.Zero) : null);
    }
}
