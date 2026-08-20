using System.Net;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Unit tests for <see cref="CroniqRunner.UpdateAuthStreak"/> — the helper that
/// decides whether a run of 401s means the credential is gone. The companion
/// conformance case <c>17-poll-401-auth-ceiling.yaml</c> describes the contract
/// at the wire level; these tests pin the local decision logic.
///
/// A 401 was previously transient, so a runner whose key was revoked retried it
/// every poll interval forever: up, healthy-looking, idle, and never exiting
/// non-zero, so nothing ever restarted it (issue #473).
/// </summary>
public class AuthStreakTests
{
    [Fact]
    public void SuccessfulPoll_ResetsStreak()
    {
        int streak = 2;
        var shouldBail = CroniqRunner.UpdateAuthStreak(
            failureStatus: null,
            consecutive: ref streak,
            maxConsecutive: 3);

        shouldBail.ShouldBeFalse();
        streak.ShouldBe(0);
    }

    [Fact]
    public void NonAuthError_ResetsStreak()
    {
        // A 5xx says nothing about whether the credential is valid. Counting it
        // would make an unwell server look like a revoked key.
        int streak = 2;
        var shouldBail = CroniqRunner.UpdateAuthStreak(
            failureStatus: HttpStatusCode.ServiceUnavailable,
            consecutive: ref streak,
            maxConsecutive: 3);

        shouldBail.ShouldBeFalse();
        streak.ShouldBe(0);
    }

    [Fact]
    public void SingleUnauthorized_IsSurvivable()
    {
        // Rotation hands over by installing the new key and giving the old one
        // an expiry (server issue #471). Bailing on the first 401 would turn a
        // narrow race around that handover into an outage.
        int streak = 0;
        var shouldBail = CroniqRunner.UpdateAuthStreak(
            failureStatus: HttpStatusCode.Unauthorized,
            consecutive: ref streak,
            maxConsecutive: 3);

        shouldBail.ShouldBeFalse();
        streak.ShouldBe(1);
    }

    [Fact]
    public void ConsecutiveUnauthorized_BailsAtThreshold()
    {
        int streak = 0;
        for (var i = 1; i <= 2; i++)
        {
            var shouldBail = CroniqRunner.UpdateAuthStreak(
                failureStatus: HttpStatusCode.Unauthorized,
                consecutive: ref streak,
                maxConsecutive: 2);

            shouldBail.ShouldBe(i == 2);
        }

        streak.ShouldBe(2);
    }

    [Fact]
    public void ConflictDoesNotSpendTheAuthBudget()
    {
        // The two budgets are independent: a duplicate deployment must not be
        // reported as an authentication failure.
        int streak = 1;
        var shouldBail = CroniqRunner.UpdateAuthStreak(
            failureStatus: HttpStatusCode.Conflict,
            consecutive: ref streak,
            maxConsecutive: 2);

        shouldBail.ShouldBeFalse();
        streak.ShouldBe(0);
    }

    [Fact]
    public void AuthFailedException_NamesTheStreakAndTheRemedy()
    {
        var ex = new AuthFailedException(3);

        ex.ConsecutiveCount.ShouldBe(3);
        ex.Message.ShouldContain("revoked");
        ex.Message.ShouldContain("Restart the runner");
    }
}
