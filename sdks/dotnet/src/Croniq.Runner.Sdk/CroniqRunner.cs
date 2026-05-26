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

    /// <summary>
    /// Pure helper that updates the consecutive-409 streak based on the
    /// latest poll outcome and decides whether to keep polling or bail.
    /// Extracted from <see cref="PollLoopAsync"/> for unit-testing; the
    /// run-loop calls it on every poll outcome.
    /// </summary>
    /// <remarks>
    /// <para>Reset cases (counter → 0):</para>
    /// <list type="bullet">
    /// <item>Successful poll — the other process must have died or released its slot.</item>
    /// <item>Non-409 transient error (5xx, network, timeout) — unrelated to instance ownership.</item>
    /// </list>
    /// <para>Increment case: 409 Conflict. Returns <c>true</c> (bail) when the
    /// counter reaches <paramref name="maxConsecutive"/>.</para>
    /// </remarks>
    internal static bool UpdateConflictStreak(
        System.Net.HttpStatusCode? failureStatus,
        ref int consecutive,
        int maxConsecutive)
    {
        if (failureStatus == null)
        {
            // Success
            consecutive = 0;
            return false;
        }
        if (failureStatus == System.Net.HttpStatusCode.Conflict)
        {
            consecutive++;
            return consecutive >= maxConsecutive;
        }
        // Transient non-409 — reset the streak so a recovered 5xx doesn't
        // accumulate with later 409s.
        consecutive = 0;
        return false;
    }

    private async Task PollLoopAsync(CancellationToken ct)
    {
        // Tracks consecutive `409 Conflict` responses on poll. See
        // UpdateConflictStreak for the reset/increment rules.
        int consecutiveConflicts = 0;

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
                UpdateConflictStreak(null, ref consecutiveConflicts, _options.MaxConsecutivePollConflicts);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                return;
            }
            catch (Exception ex)
            {
                _stateProbe.MarkPollFailure(_timeProvider.GetUtcNow(), ex.Message);

                // Detect 409 Conflict — server says another runner is
                // already registered with this runner_id. After enough
                // consecutive 409s we propagate fatally so the host can
                // exit non-zero instead of retrying forever (see #134).
                var status = (ex as HttpRequestException)?.StatusCode;
                var shouldBail = UpdateConflictStreak(
                    status, ref consecutiveConflicts, _options.MaxConsecutivePollConflicts);

                if (shouldBail)
                {
                    _logger.LogError(
                        ex,
                        "fatal: server returned 409 Conflict on poll {Count} times in a row — " +
                        "another runner is registered with runner_id={RunnerId}. " +
                        "Stop the duplicate process or rotate the runner_id.",
                        consecutiveConflicts, _resolvedRunnerId);
                    throw new PollInstanceConflictException(_resolvedRunnerId!, consecutiveConflicts, ex);
                }

                if (status == System.Net.HttpStatusCode.Conflict)
                {
                    _logger.LogWarning(
                        "poll returned 409 Conflict ({Consecutive}/{Max}) — another runner instance may be active; retrying after {Delay}",
                        consecutiveConflicts, _options.MaxConsecutivePollConflicts, _options.PollRetryDelay);
                }
                else
                {
                    _logger.LogWarning(ex, "poll failed — backing off {Delay}", _options.PollRetryDelay);
                }

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
                // Standalone CTS — intentionally NOT linked to the outer poll
                // token. Host-shutdown stops new polls but in-flight handlers
                // run to natural completion (matching the Rust SDK's drain
                // semantics, and what most graceful-shutdown stories expect).
                // Server-initiated cancel (PollResponse.cancel) and the
                // drain-timeout fallback hard-cancel via this CTS.
                var executionCts = new CancellationTokenSource();
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
            // Drain budget exhausted — handlers had their grace period.
            // Hard-cancel via the per-execution CTS so any well-behaved
            // handler that respects ctx.CancellationToken aborts cleanly;
            // the dispatcher then acks with status=failure. Handlers that
            // ignore the token block until they finish naturally; the
            // runner returns from RunAsync anyway.
            _logger.LogWarning("drain timed out with {Count} execution(s) still in-flight — hard-cancelling", _inflight.Count);
            foreach (var cts in _inflight.Values)
            {
                try
                {
                    cts.Cancel();
                }
                catch (ObjectDisposedException)
                {
                    // race with handler completion — harmless
                }
            }
        }
    }
}

internal static class CollectionExtensions
{
    public static IReadOnlyList<T> AsReadOnlyList<T>(this IList<T> source)
        => source as IReadOnlyList<T> ?? source.ToArray();
}
