using System.Threading.Channels;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Logging;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Time.Testing;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests.Logging;

/// <summary>
/// Deterministic version of conformance case 10
/// (<c>10-streaming-logs-time-threshold.yaml</c>) — the YAML can only
/// assert <c>min_count: 1</c> because the flusher's
/// <see cref="Task.WhenAny(System.Threading.Tasks.Task[])"/> biases
/// toward channel-reads over timer ticks under real-time scheduling.
/// With a <see cref="FakeTimeProvider"/> we drive the timer ourselves
/// and can prove the partial-batch flush path reliably.
/// </summary>
public class LogWriterTests
{
    /// <summary>
    /// Records every <see cref="PushEventsAsync"/> call and signals a
    /// channel so the test can <c>await</c> until <em>N</em> POSTs have
    /// landed, without sleeping past pessimistic real-time guesses.
    /// </summary>
    private sealed class RecordingClient : ICroniqClient
    {
        private readonly Channel<int> _posted =
            Channel.CreateUnbounded<int>(new UnboundedChannelOptions
            {
                SingleReader = true,
                SingleWriter = false,
            });

        private readonly List<IReadOnlyList<WorkEvent>> _posts = [];

        public IReadOnlyList<WorkEvent>[] Posts
        {
            get { lock (_posts) { return [.. _posts]; } }
        }

        public int PostCount
        {
            get { lock (_posts) { return _posts.Count; } }
        }

        public Task PushEventsAsync(string executionId, IReadOnlyList<WorkEvent> events, CancellationToken ct)
        {
            lock (_posts)
            {
                _posts.Add([.. events]);
            }
            _posted.Writer.TryWrite(0);
            return Task.CompletedTask;
        }

        public async Task WaitForPostsAsync(int expected, TimeSpan timeout)
        {
            using var cts = new CancellationTokenSource(timeout);
            while (PostCount < expected)
            {
                try
                {
                    await _posted.Reader.ReadAsync(cts.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    throw new TimeoutException(
                        $"Expected at least {expected} POSTs within {timeout}, observed {PostCount}.");
                }
            }
        }

        // The remaining methods are never called in these tests; throwing
        // is correct so a regression that starts hitting them is loud.
        public Task<PollResponse> PollAsync(PollRequest request, TimeSpan timeout, CancellationToken ct) =>
            throw new NotSupportedException();
        public Task AckAsync(AckRequest request, CancellationToken ct) =>
            throw new NotSupportedException();
        public Task RenewAsync(RenewRequest request, CancellationToken ct) =>
            throw new NotSupportedException();
        public Task RegisterJobAsync(RegisterJobRequest request, CancellationToken ct) =>
            throw new NotSupportedException();
    }

    private static LogWriter NewWriter(
        RecordingClient client,
        FakeTimeProvider time,
        TimeSpan? batchTimeThreshold = null,
        int batchSizeThreshold = 32,
        FlusherParkWatcher? parks = null)
    {
        var options = new LogWriterOptions
        {
            BatchTimeThreshold = batchTimeThreshold ?? TimeSpan.FromMilliseconds(200),
            BatchSizeThreshold = batchSizeThreshold,
        };
        return new LogWriter(
            client,
            executionId: "exec-test",
            enrichment: new LogEnrichment("test:job", "runner-1", []),
            options: options,
            logger: NullLogger.Instance,
            timeProvider: time,
            onFlusherParked: parks is null ? null : parks.Record);
    }

    /// <summary>
    /// Records the flusher loop's park signals so a test can await the
    /// precondition <see cref="FakeTimeProvider.Advance"/> needs: the loop has
    /// drained into its batch buffer and is parked on a
    /// <see cref="PeriodicTimer"/> wait registered against the fake clock.
    /// </summary>
    /// <remarks>
    /// This replaces a <c>Task.Delay(50)</c>, which was a bet that the
    /// background loop had got that far within 50 ms of real time. On a loaded
    /// CI runner the bet loses, and losing it is unrecoverable rather than
    /// slow: the timer is created inside the loop, so an <c>Advance</c> that
    /// lands first schedules the next tick past a clock nothing will move
    /// again, and the test then waits two real seconds for a POST that can no
    /// longer happen (issue #570).
    ///
    /// Parks are buffered in a channel and the recorder is wired through the
    /// writer's constructor, so no signal can be missed — including the loop's
    /// very first park, which in a test that writes nothing is the only one
    /// that will ever arrive unprompted.
    /// </remarks>
    private sealed class FlusherParkWatcher
    {
        private static readonly TimeSpan DefaultTimeout = TimeSpan.FromSeconds(10);

        private readonly Channel<int> _parks =
            Channel.CreateUnbounded<int>(new UnboundedChannelOptions
            {
                SingleReader = true,
                SingleWriter = true,
            });

        public void Record(int buffered) => _parks.Writer.TryWrite(buffered);

        /// <summary>Await the next park, whatever the buffer holds.</summary>
        public Task<int> NextParkAsync(TimeSpan? timeout = null) =>
            _parks.Reader.ReadAsync().AsTask().WaitAsync(timeout ?? DefaultTimeout);

        /// <summary>
        /// Await parks until one reports at least <paramref name="buffered"/>
        /// events. Earlier parks — the loop's first, empty-buffered one — are
        /// skipped rather than mistaken for it.
        /// </summary>
        public async Task ParkedWithBufferedAsync(int buffered, TimeSpan? timeout = null)
        {
            var deadline = timeout ?? DefaultTimeout;
            var elapsed = System.Diagnostics.Stopwatch.StartNew();
            while (true)
            {
                var remaining = deadline - elapsed.Elapsed;
                if (remaining <= TimeSpan.Zero)
                {
                    throw new TimeoutException(
                        $"Flusher never parked with {buffered} event(s) buffered within {deadline}.");
                }
                if (await NextParkAsync(remaining).ConfigureAwait(false) >= buffered)
                {
                    return;
                }
            }
        }
    }

    [Fact]
    public async Task PartialBatchFlushesWhenTimeThresholdExpires()
    {
        var time = new FakeTimeProvider();
        var client = new RecordingClient();
        var parks = new FlusherParkWatcher();
        await using var writer = NewWriter(client, time, parks: parks);

        await writer.WriteAsync(new WorkEvent { Level = "info", Message = "single event" });
        await parks.ParkedWithBufferedAsync(1);

        // Exactly one tick crosses the partial-batch threshold and
        // produces one POST. Without TimeProvider injection this could
        // never be asserted deterministically — see conformance case 10.
        time.Advance(TimeSpan.FromMilliseconds(200));
        await client.WaitForPostsAsync(expected: 1, timeout: TimeSpan.FromSeconds(2));

        client.PostCount.ShouldBe(1);
        client.Posts[0].Count.ShouldBe(1);
        client.Posts[0][0].Message.ShouldBe("single event");
    }

    [Fact]
    public async Task BatchSizeThresholdStillFlushesWithoutTimeAdvance()
    {
        // Regression guard: TimeProvider injection must not break the
        // batch-by-count path, which fires on its own without any timer
        // involvement.
        var time = new FakeTimeProvider();
        var client = new RecordingClient();
        var parks = new FlusherParkWatcher();
        await using var writer = NewWriter(client, time, batchSizeThreshold: 4, parks: parks);

        for (var i = 0; i < 4; i++)
        {
            await writer.WriteAsync(new WorkEvent { Level = "info", Message = $"event-{i}" });
        }

        await client.WaitForPostsAsync(expected: 1, timeout: TimeSpan.FromSeconds(2));

        client.PostCount.ShouldBe(1);
        client.Posts[0].Count.ShouldBe(4);
        // Time was never advanced; no spurious second POST should appear.
        // Waiting for the loop to park again is what the fixed sleep was
        // approximating, and it also proves the loop got back there.
        await parks.NextParkAsync();
        client.PostCount.ShouldBe(1);
    }

    [Fact]
    public async Task SuccessiveTimerFiresProduceMultipleFlushes()
    {
        // The scenario conformance case 10 describes — multiple
        // time-threshold-driven POSTs across the lifetime of a single
        // execution — but with FakeTimeProvider it's now reliable.
        var time = new FakeTimeProvider();
        var client = new RecordingClient();
        var parks = new FlusherParkWatcher();
        await using var writer = NewWriter(client, time, parks: parks);

        await writer.WriteAsync(new WorkEvent { Level = "info", Message = "first" });
        await parks.ParkedWithBufferedAsync(1);
        time.Advance(TimeSpan.FromMilliseconds(200));
        await client.WaitForPostsAsync(expected: 1, timeout: TimeSpan.FromSeconds(2));

        await writer.WriteAsync(new WorkEvent { Level = "info", Message = "second" });
        await parks.ParkedWithBufferedAsync(1);
        time.Advance(TimeSpan.FromMilliseconds(200));
        await client.WaitForPostsAsync(expected: 2, timeout: TimeSpan.FromSeconds(2));

        client.PostCount.ShouldBe(2);
        client.Posts[0][0].Message.ShouldBe("first");
        client.Posts[1][0].Message.ShouldBe("second");
    }

    [Fact]
    public async Task NoTimerFireWhenBufferIsEmpty()
    {
        // The tick-branch of the loop only POSTs when buffer.Count > 0.
        // An idle timer must never produce empty network calls.
        var time = new FakeTimeProvider();
        var client = new RecordingClient();
        var parks = new FlusherParkWatcher();
        await using var writer = NewWriter(client, time, parks: parks);

        // The timer is created inside the loop, so advancing before the first
        // park would move the clock past a timer that does not exist yet and
        // the test would pass without ever exercising a tick.
        await parks.NextParkAsync();

        // Advance multiple ticks worth of time with nothing written.
        time.Advance(TimeSpan.FromSeconds(2));

        // The loop parking again is the tick having been observed and the
        // wait re-armed — a stronger statement than a real-time sleep, which
        // could not tell "no POST" from "no tick".
        await parks.NextParkAsync();

        client.PostCount.ShouldBe(0);
    }
}
