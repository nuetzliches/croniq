using System.Collections.Concurrent;
using System.Net;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Review 2026-10-24, K-1 and K-4. A failed ack used to be logged and dropped,
/// so the server requeued an execution that had already succeeded. Acks are now
/// retried on transport errors and 5xx (three attempts), never on 4xx, and
/// the drain waits for the failure ack of a hard-cancelled execution.
/// </summary>
public sealed class AckRetryTests
{
    private static readonly TimeSpan[] FastDelays = [TimeSpan.FromMilliseconds(10), TimeSpan.FromMilliseconds(20)];

    private static readonly AckRequest Request = new("r1", "e1", "success", null, 1, 1);

    [Fact]
    public async Task A_5xx_is_retried_until_the_server_accepts()
    {
        var client = new ScriptedClient(Status(HttpStatusCode.ServiceUnavailable), null);

        var ok = await AckSender.SendAsync(client, Request, NullLogger.Instance, FastDelays);

        ok.ShouldBeTrue();
        client.Acks.Count.ShouldBe(2);
    }

    [Fact]
    public async Task A_transport_error_is_retried()
    {
        var client = new ScriptedClient(new HttpRequestException("connection reset"), null);

        (await AckSender.SendAsync(client, Request, NullLogger.Instance, FastDelays)).ShouldBeTrue();
        client.Acks.Count.ShouldBe(2);
    }

    [Fact]
    public async Task A_request_timeout_is_retried()
    {
        var client = new ScriptedClient(new TaskCanceledException("request timed out"), null);

        (await AckSender.SendAsync(client, Request, NullLogger.Instance, FastDelays)).ShouldBeTrue();
        client.Acks.Count.ShouldBe(2);
    }

    [Theory]
    [InlineData(HttpStatusCode.Conflict)]
    [InlineData(HttpStatusCode.NotFound)]
    [InlineData(HttpStatusCode.BadRequest)]
    [InlineData(HttpStatusCode.Forbidden)]
    public async Task A_4xx_is_not_retried(HttpStatusCode status)
    {
        var client = new ScriptedClient(Status(status), null);

        (await AckSender.SendAsync(client, Request, NullLogger.Instance, FastDelays)).ShouldBeFalse();
        client.Acks.Count.ShouldBe(1);
    }

    [Fact]
    public async Task Retries_stop_after_three_attempts()
    {
        var client = new ScriptedClient(
            Status(HttpStatusCode.InternalServerError),
            Status(HttpStatusCode.BadGateway),
            Status(HttpStatusCode.ServiceUnavailable),
            null);

        (await AckSender.SendAsync(client, Request, NullLogger.Instance, FastDelays)).ShouldBeFalse();
        client.Acks.Count.ShouldBe(3);
    }

    [Fact]
    public void Default_delays_are_one_then_two_seconds()
    {
        AckSender.DefaultRetryDelays.ShouldBe([TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2)]);
    }

    [Fact]
    public async Task Drain_timeout_still_delivers_the_failure_ack_with_its_retry()
    {
        // The handler ignores nothing — it honours its token — but the first
        // failure ack hits a 503. Before K-4, DrainAsync returned right after
        // the hard cancel, so RunAsync completed while the ack (and certainly
        // its retry) was still pending.
        var client = new ScriptedClient(Status(HttpStatusCode.ServiceUnavailable), null)
        {
            Work = [new WorkAssignment("exec-drain", "stuck", "2026-10-24T10:00:00Z", 1, System.Text.Json.JsonDocument.Parse("{}").RootElement, "5m")],
        };
        var options = new CroniqRunnerOptions
        {
            ServerUrl = "http://croniq.test",
            RunnerId = "r1",
            DrainTimeout = TimeSpan.FromMilliseconds(100),
            RequestTimeout = TimeSpan.FromSeconds(1),
            PollRetryDelay = TimeSpan.FromMilliseconds(50),
            CapacityBackoff = TimeSpan.FromMilliseconds(20),
        };
        var registry = new CroniqHandlerRegistry();
        var started = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        registry.RegisterDelegate("stuck", async (_, ct) =>
        {
            started.TrySetResult();
            await Task.Delay(Timeout.Infinite, ct);
        });
        using var services = new ServiceCollection().BuildServiceProvider();
        var runner = new CroniqRunner(
            Options.Create(options),
            client,
            services,
            NullLoggerFactory.Instance,
            registry,
            new RunnerIdentityResolver(Options.Create(options), NullLogger<RunnerIdentityResolver>.Instance),
            new RunnerStateProbe())
        {
            AckRetryDelays = [TimeSpan.FromMilliseconds(300), TimeSpan.FromMilliseconds(300)],
        };

        using var cts = new CancellationTokenSource();
        var run = runner.RunAsync(cts.Token);
        await started.Task.WaitAsync(TimeSpan.FromSeconds(5));
        await cts.CancelAsync();
        await run.WaitAsync(TimeSpan.FromSeconds(10));

        client.Acks.Count.ShouldBe(2);
        client.Acks.ShouldAllBe(a => a.ExecutionId == "exec-drain" && a.Status == "failure");
    }

    private static HttpRequestException Status(HttpStatusCode code) =>
        new($"status {(int)code}", inner: null, code);

    /// <summary>
    /// Answers each ack with the next scripted outcome (an exception to
    /// throw, or <c>null</c> for success); once the script runs out every
    /// ack succeeds. Hands out <see cref="Work"/> on the first poll only.
    /// </summary>
    private sealed class ScriptedClient(params Exception?[] ackOutcomes) : ICroniqClient
    {
        private readonly ConcurrentQueue<Exception?> _outcomes = new(ackOutcomes);
        private int _polls;

        public ConcurrentQueue<AckRequest> Acks { get; } = new();

        public IReadOnlyList<WorkAssignment> Work { get; init; } = [];

        public async Task<PollResponse> PollAsync(PollRequest request, TimeSpan timeout, CancellationToken ct)
        {
            if (Interlocked.Increment(ref _polls) == 1)
            {
                return new PollResponse(Work, []);
            }
            await Task.Delay(TimeSpan.FromMilliseconds(20), ct);
            return new PollResponse([], []);
        }

        public Task AckAsync(AckRequest request, CancellationToken ct)
        {
            Acks.Enqueue(request);
            if (_outcomes.TryDequeue(out var outcome) && outcome is not null)
            {
                return Task.FromException(outcome);
            }
            return Task.CompletedTask;
        }

        public Task RenewAsync(RenewRequest request, CancellationToken ct) => Task.CompletedTask;

        public Task PushEventsAsync(string executionId, IReadOnlyList<WorkEvent> events, CancellationToken ct) =>
            Task.CompletedTask;

        public Task RegisterJobAsync(RegisterJobRequest request, CancellationToken ct) => Task.CompletedTask;
    }
}
