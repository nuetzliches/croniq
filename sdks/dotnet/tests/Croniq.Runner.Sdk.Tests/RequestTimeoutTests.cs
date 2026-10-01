using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Issue #792: the runner's <see cref="HttpClient"/> has an infinite timeout
/// (for the long poll), and the ack was sent with
/// <see cref="CancellationToken.None"/>. An ack the server never answered
/// therefore never returned, and the execution stayed in the in-flight set
/// that every poll reports. Non-poll requests are bounded by
/// <see cref="CroniqRunnerOptions.RequestTimeout"/> now.
/// </summary>
public sealed class RequestTimeoutTests
{
    private static CroniqClient ClientAgainstSilentServer(TimeSpan requestTimeout)
    {
        var http = new HttpClient(new NeverAnswers())
        {
            BaseAddress = new Uri("http://croniq.test"),
            Timeout = Timeout.InfiniteTimeSpan,
        };
        var options = Options.Create(new CroniqRunnerOptions { RequestTimeout = requestTimeout });
        return new CroniqClient(http, options, NullLogger<CroniqClient>.Instance);
    }

    [Fact]
    public async Task Ack_gives_up_after_the_request_timeout()
    {
        var client = ClientAgainstSilentServer(TimeSpan.FromMilliseconds(200));

        var ack = client.AckAsync(
            new AckRequest("r1", "e1", "success", null, 1, 1),
            CancellationToken.None);

        var finished = await Task.WhenAny(ack, Task.Delay(TimeSpan.FromSeconds(5)));
        Assert.Same(ack, finished);
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => ack);
    }

    [Fact]
    public async Task Renew_and_event_push_are_bounded_too()
    {
        var client = ClientAgainstSilentServer(TimeSpan.FromMilliseconds(200));

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => client.RenewAsync(new RenewRequest("r1", "e1"), CancellationToken.None));
        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => client.PushEventsAsync("e1", [new WorkEvent { Level = "info", Message = "line" }], CancellationToken.None));
    }

    [Fact]
    public void Default_request_timeout_is_thirty_seconds()
    {
        Assert.Equal(TimeSpan.FromSeconds(30), new CroniqRunnerOptions().RequestTimeout);
    }

    /// <summary>Accepts the request and never responds, like a half-open socket.</summary>
    private sealed class NeverAnswers : HttpMessageHandler
    {
        protected override async Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request, CancellationToken cancellationToken)
        {
            await Task.Delay(Timeout.Infinite, cancellationToken).ConfigureAwait(false);
            throw new InvalidOperationException("unreachable");
        }
    }
}
