using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Review 2026-10-24, K-9: the poll retry delay is spread over ±20 % so a
/// fleet that lost its polls to a server restart does not retry in lockstep.
/// </summary>
public sealed class PollRetryJitterTests
{
    [Theory]
    [InlineData(0.0, 4000)]
    [InlineData(0.5, 5000)]
    [InlineData(0.999999, 6000)]
    public void Jitter_spans_eighty_to_one_hundred_twenty_percent(double sample, int expectedMs)
    {
        var jittered = CroniqRunner.Jitter(TimeSpan.FromSeconds(5), sample);

        jittered.TotalMilliseconds.ShouldBe(expectedMs, tolerance: 1);
    }

    [Fact]
    public void Jitter_stays_inside_the_bounds_for_random_samples()
    {
        for (var i = 0; i < 1000; i++)
        {
            var jittered = CroniqRunner.Jitter(TimeSpan.FromMilliseconds(400), Random.Shared.NextDouble());
            jittered.ShouldBeInRange(TimeSpan.FromMilliseconds(320), TimeSpan.FromMilliseconds(480));
        }
    }
}
