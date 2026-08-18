using System.Net;
using System.Net.Sockets;

using Croniq.Runner.Sdk.HealthChecks;
using Croniq.Runner.Sdk.Internal;

using Microsoft.Extensions.Diagnostics.HealthChecks;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Pins the contract that the health-check description never carries raw
/// exception text. <see cref="HttpRequestException"/> and
/// <see cref="SocketException"/> messages embed the resolved host and port,
/// and a health endpoint is routinely reachable without authentication.
/// </summary>
public class PollFailureDescriptionTests
{
    // Stands in for a real DNS/connect failure message. The host and port are
    // the part that must never reach the description.
    private const string LeakyHost = "croniq.internal";
    private const string LeakyMessage =
        $"No such host is known. ({LeakyHost}:4000)";

    [Fact]
    public void SocketException_ReportsCategoryOnly()
    {
        var ex = new SocketException((int)SocketError.HostNotFound);

        var reason = CroniqRunner.DescribePollFailure(ex);

        reason.ShouldBe("connection failed");
    }

    [Fact]
    public void HttpRequestException_WithoutStatus_ReportsCategoryOnly()
    {
        // Transport-level failure: no response ever arrived, and the message
        // is where the resolved endpoint shows up.
        var ex = new HttpRequestException(LeakyMessage);

        var reason = CroniqRunner.DescribePollFailure(ex);

        reason.ShouldBe("connection failed");
        reason.ShouldNotContain(LeakyHost);
    }

    [Fact]
    public void HttpRequestException_WithStatus_ReportsStatusCode()
    {
        // The status describes the response, not the deployment, so it is safe
        // to publish — and it is the single most useful fact for an operator.
        var ex = new HttpRequestException(LeakyMessage, null, HttpStatusCode.Conflict);

        CroniqRunner.DescribePollFailure(ex).ShouldBe("http status 409");
    }

    [Fact]
    public void TaskCanceled_ReportsTimeoutCategory()
    {
        CroniqRunner.DescribePollFailure(new TaskCanceledException())
            .ShouldBe("poll timed out");
    }

    [Fact]
    public void UnknownException_FallsBackToGenericCategory()
    {
        CroniqRunner.DescribePollFailure(new InvalidOperationException(LeakyMessage))
            .ShouldBe("poll failed");
    }

    [Fact]
    public void DescriptionsAreDrawnFromAClosedSet()
    {
        // Whatever DescribePollFailure returns is published, so the whole
        // range has to be enumerable and free of deployment detail.
        var reasons = new[]
        {
            CroniqRunner.DescribePollFailure(new SocketException(10061)),
            CroniqRunner.DescribePollFailure(new HttpRequestException(LeakyMessage)),
            CroniqRunner.DescribePollFailure(new HttpRequestException(LeakyMessage, null, HttpStatusCode.BadGateway)),
            CroniqRunner.DescribePollFailure(new TaskCanceledException(LeakyMessage)),
            CroniqRunner.DescribePollFailure(new IOException(LeakyMessage)),
        };

        foreach (var reason in reasons)
        {
            reason.ShouldNotContain(LeakyHost);
            reason.ShouldNotContain("4000");
        }
    }

    [Fact]
    public async Task HealthCheckDescription_CarriesTheCategoryNotTheMessage()
    {
        // End-to-end over the probe: what an anonymous reader of /health sees
        // when a custom response writer renders the description.
        var probe = new RunnerStateProbe();
        probe.MarkStarted();
        var start = DateTimeOffset.UtcNow;
        probe.MarkSuccessfulPoll(start);
        probe.MarkPollFailure(
            start,
            CroniqRunner.DescribePollFailure(new HttpRequestException(LeakyMessage)));

        var time = new FakeTimeProvider(start.AddMinutes(10));
        var check = new CroniqRunnerHealthCheck(probe, time);

        var result = await check.CheckHealthAsync(new HealthCheckContext());

        result.Status.ShouldBe(HealthStatus.Unhealthy);
        result.Description.ShouldNotBeNull();
        result.Description.ShouldContain("connection failed");
        result.Description.ShouldNotContain(LeakyHost);
    }

    private sealed class FakeTimeProvider(DateTimeOffset now) : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => now;
    }
}
