using System.Collections.Concurrent;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;
using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk;

/// <summary>
/// The Croniq runner: polls the server for work, dispatches handlers
/// concurrently up to <see cref="CroniqRunnerOptions.MaxInflight"/>,
/// renews leases for in-flight executions, and reports completion.
///
/// Hosting-agnostic: call <see cref="RunAsync(CancellationToken)"/> to
/// drive the loop yourself. Inside a Generic Host, prefer
/// <c>services.AddCroniqRunner(...)</c> which registers a
/// <c>BackgroundService</c> adapter that calls <see cref="RunAsync(CancellationToken)"/>
/// on your behalf.
/// </summary>
public sealed class CroniqRunner : IAsyncDisposable
{
    private readonly ICroniqClient _client;
    private readonly CroniqHandlerRegistry _registry;
    private readonly IServiceProvider _serviceProvider;
    private readonly ILoggerFactory _loggerFactory;
    private readonly ILogger<CroniqRunner> _logger;
    private readonly RunnerStateProbe _stateProbe;
    private readonly RunnerIdentityResolver _identityResolver;
    private readonly CroniqRunnerOptions _options;
    private readonly TimeProvider _timeProvider;

    private readonly string _instanceId = Guid.NewGuid().ToString("N");
    private readonly ConcurrentDictionary<string, CancellationTokenSource> _inflight = new();
    private readonly CancellationTokenSource _drainCts = new();

    private string? _resolvedRunnerId;
    private ExecutionDispatcher? _dispatcher;
    private int _runOnce;

    /// <summary>
    /// Construct a runner. Internal: consumers use
    /// <c>services.AddCroniqRunner(...)</c> which wires this via DI.
    /// </summary>
    internal CroniqRunner(
        IOptions<CroniqRunnerOptions> options,
        ICroniqClient client,
        IServiceProvider serviceProvider,
        ILoggerFactory loggerFactory,
        CroniqHandlerRegistry registry,
        RunnerIdentityResolver identityResolver,
        RunnerStateProbe stateProbe,
        TimeProvider? timeProvider = null)
    {
        _options = options.Value;
        _client = client;
        _serviceProvider = serviceProvider;
        _loggerFactory = loggerFactory;
        _logger = loggerFactory.CreateLogger<CroniqRunner>();
        _registry = registry;
        _identityResolver = identityResolver;
        _stateProbe = stateProbe;
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    /// <summary>The stable runner ID resolved during <see cref="RunAsync(CancellationToken)"/>.</summary>
    public string RunnerId => _resolvedRunnerId ?? throw new InvalidOperationException("RunnerId is only available after RunAsync starts.");

    /// <summary>Snapshot of currently in-flight execution IDs (diagnostic only).</summary>
    public IReadOnlyCollection<string> Inflight => _inflight.Keys.ToArray();

    /// <summary>
    /// Run the poll/dispatch/ack loop until cancellation. Returns when the
    /// token is signalled <em>and</em> all in-flight executions have either
    /// finished or the drain timeout elapses.
    /// </summary>
    public async Task RunAsync(CancellationToken cancellationToken)
    {
        if (Interlocked.Exchange(ref _runOnce, 1) != 0)
        {
            throw new InvalidOperationException("CroniqRunner.RunAsync may only be called once per instance.");
        }

        _resolvedRunnerId = _identityResolver.Resolve();
        _stateProbe.MarkStarted();

        // Apply all handler registrations queued via Add…Handler() calls.
        foreach (var registration in _serviceProvider.GetServices<HandlerRegistration>())
        {
            registration.ApplyTo(_registry);
        }

        _dispatcher = new ExecutionDispatcher(
            _client, _registry, _serviceProvider, _loggerFactory, _stateProbe,
            _options, _resolvedRunnerId, _options.Tags.AsReadOnlyList());

        _logger.LogInformation(
            "Croniq runner starting: runner_id={RunnerId}, capabilities={Capabilities}, max_inflight={MaxInflight}",
            _resolvedRunnerId,
            string.Join(",", _options.Capabilities),
            _options.MaxInflight);

        await SelfRegisterSchedulesAsync(cancellationToken).ConfigureAwait(false);

        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _drainCts.Token);

        try
        {
            await PollLoopAsync(linked.Token).ConfigureAwait(false);
        }
        finally
        {
            _stateProbe.MarkDraining();
            await DrainAsync().ConfigureAwait(false);
        }
    }

    /// <summary>
    /// Signal graceful shutdown without waiting. Cancels new polls; in-flight
    /// handlers keep running until they complete or the host shutdown token
    /// reaches them.
    /// </summary>
    public void RequestDrain()
    {
        if (!_drainCts.IsCancellationRequested)
        {
            _drainCts.Cancel();
        }
    }

    public async ValueTask DisposeAsync()
    {
        RequestDrain();
        await DrainAsync().ConfigureAwait(false);
        _drainCts.Dispose();
    }

    private async Task SelfRegisterSchedulesAsync(CancellationToken ct)
    {
        foreach (var entry in _registry.SelfRegisterSchedules)
        {
            try
            {
                await _client.RegisterJobAsync(
                    new RegisterJobRequest(
                        entry.JobKey,
                        entry.Schedule,
                        Timezone: null,
                        Timeout: entry.Timeout,
                        RunnerId: _resolvedRunnerId,
                        Capabilities: _options.Capabilities.AsReadOnlyList(),
                        Description: entry.Description),
                    ct).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                _logger.LogWarning(
                    ex,
                    "self-register for job {JobKey} failed — runner will still poll, but the server may not have a schedule",
                    entry.JobKey);
            }
        }
    }

    private async Task PollLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            if (_inflight.Count >= _options.MaxInflight)
            {
                try
                {
                    await Task.Delay(_options.CapacityBackoff, ct).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                continue;
            }

            var inflightIds = _inflight.Keys.ToArray();
            var request = new PollRequest(
                _resolvedRunnerId!,
                _options.Capabilities.AsReadOnlyList(),
                _options.MaxInflight,
                inflightIds,
                _instanceId,
                _options.Tags.AsReadOnlyList());

            PollResponse response;
            try
            {
                response = await _client.PollAsync(request, _options.PollTimeout, ct).ConfigureAwait(false);
                _stateProbe.MarkSuccessfulPoll(_timeProvider.GetUtcNow());
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                return;
            }
            catch (Exception ex)
            {
                _stateProbe.MarkPollFailure(_timeProvider.GetUtcNow(), ex.Message);
                _logger.LogWarning(ex, "poll failed — backing off {Delay}", _options.PollRetryDelay);
                try
                {
                    await Task.Delay(_options.PollRetryDelay, ct).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                continue;
            }

            HandleCancellations(response.Cancel);

            foreach (var assignment in response.Work)
            {
                var executionCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
                if (!_inflight.TryAdd(assignment.ExecutionId, executionCts))
                {
                    // Already tracking — server sent a duplicate; ignore.
                    executionCts.Dispose();
                    continue;
                }

                var dispatchCt = ct;
                _ = _dispatcher!
                    .DispatchAsync(assignment, executionCts, dispatchCt)
                    .ContinueWith(
                        _ =>
                        {
                            if (_inflight.TryRemove(assignment.ExecutionId, out var cts))
                            {
                                cts.Dispose();
                            }
                        },
                        TaskScheduler.Default);
            }
        }
    }

    private void HandleCancellations(IReadOnlyList<string> cancelIds)
    {
        if (cancelIds.Count == 0)
        {
            return;
        }
        foreach (var id in cancelIds)
        {
            if (_inflight.TryGetValue(id, out var cts))
            {
                try
                {
                    cts.Cancel();
                    _logger.LogInformation("server requested cancellation of execution {ExecutionId}", id);
                }
                catch (ObjectDisposedException)
                {
                    // race with completion
                }
            }
        }
    }

    private async Task DrainAsync()
    {
        if (_inflight.IsEmpty)
        {
            return;
        }
        _logger.LogInformation("draining {Count} in-flight execution(s) (timeout {Timeout})", _inflight.Count, _options.DrainTimeout);
        var deadline = _timeProvider.GetUtcNow().Add(_options.DrainTimeout);
        while (!_inflight.IsEmpty && _timeProvider.GetUtcNow() < deadline)
        {
            await Task.Delay(TimeSpan.FromMilliseconds(50)).ConfigureAwait(false);
        }
        if (!_inflight.IsEmpty)
        {
            _logger.LogWarning("drain timed out with {Count} execution(s) still in-flight", _inflight.Count);
        }
    }
}

internal static class CollectionExtensions
{
    public static IReadOnlyList<T> AsReadOnlyList<T>(this IList<T> source)
        => source as IReadOnlyList<T> ?? source.ToArray();
}
