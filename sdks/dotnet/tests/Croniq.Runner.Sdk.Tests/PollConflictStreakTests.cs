using System.Net;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Unit tests for <see cref="CroniqRunner.UpdateConflictStreak"/> —
/// the helper that decides whether to keep polling or bail after a
/// poll outcome. The companion conformance case
/// <c>11a-poll-409-fatal-after-n.yaml</c> describes the contract at
/// the wire level; these tests pin the local decision logic.
/// </summary>
public class PollConflictStreakTests
{
    [Fact]
    public void SuccessfulPoll_ResetsStreak()
    {
        int streak = 2;
        var shouldBail = CroniqRunner.UpdateConflictStreak(
            failureStatus: null,
            consecutive: ref streak,
            maxConsecutive: 3);

        shouldBail.ShouldBeFalse();
        streak.ShouldBe(0);
    }

    [Fact]
    public void NonConflictError_ResetsStreak()
    {
        // A 5xx in the middle of a 409 streak must NOT extend it —
        // service availability is unrelated to instance ownership.
        int streak = 2;
        var shouldBail = CroniqRunner.UpdateConflictStreak(
            failureStatus: HttpStatusCode.ServiceUnavailable,
            consecutive: ref streak,
            maxConsecutive: 3);

        shouldBail.ShouldBeFalse();
        streak.ShouldBe(0);
    }

    [Fact]
    public void Conflict_IncrementsAndBailsAtThreshold()
    {
        int streak = 0;
        for (int expected = 1; expected < 3; expected++)
        {
            var shouldBail = CroniqRunner.UpdateConflictStreak(
                failureStatus: HttpStatusCode.Conflict,
                consecutive: ref streak,
                maxConsecutive: 3);
            shouldBail.ShouldBeFalse();
            streak.ShouldBe(expected);
        }

        // Third conflict trips the threshold.
        var bail = CroniqRunner.UpdateConflictStreak(
            failureStatus: HttpStatusCode.Conflict,
            consecutive: ref streak,
            maxConsecutive: 3);

        bail.ShouldBeTrue();
        streak.ShouldBe(3);
    }

    [Fact]
    public void ConflictThenSuccessThenConflicts_ResetsBetweenStreaks()
    {
        // Real-world flow: brief conflict, recovery, then a fresh
        // streak. The recovery must clear the counter so the new
        // streak gets its full budget.
        int streak = 0;

        // Build up a partial streak.
        CroniqRunner.UpdateConflictStreak(HttpStatusCode.Conflict, ref streak, 3).ShouldBeFalse();
        CroniqRunner.UpdateConflictStreak(HttpStatusCode.Conflict, ref streak, 3).ShouldBeFalse();
        streak.ShouldBe(2);

        // Recovery clears.
        CroniqRunner.UpdateConflictStreak(failureStatus: null, ref streak, 3).ShouldBeFalse();
        streak.ShouldBe(0);

        // Fresh streak starts at 1.
        CroniqRunner.UpdateConflictStreak(HttpStatusCode.Conflict, ref streak, 3).ShouldBeFalse();
        streak.ShouldBe(1);
    }

    [Fact]
    public void MaxOne_BailsOnFirstConflict()
    {
        // Operator setting that refuses to tolerate any conflict.
        int streak = 0;
        var shouldBail = CroniqRunner.UpdateConflictStreak(
            failureStatus: HttpStatusCode.Conflict,
            consecutive: ref streak,
            maxConsecutive: 1);

        shouldBail.ShouldBeTrue();
        streak.ShouldBe(1);
    }

    [Fact]
    public void Forbidden_BailsImmediatelyRegardlessOfThreshold()
    {
        // 403 is the ownership refusal from #436: permanent, so the
        // effective threshold is 1 no matter how tolerant the operator
        // configured the 409 streak.
        int streak = 0;
        CroniqRunner.UpdateConflictStreak(HttpStatusCode.Forbidden, ref streak, 100).ShouldBeTrue();
    }

    [Fact]
    public void Forbidden_LeavesTheConflictStreakAlone()
    {
        // The counter reports how long a duplicate deployment has been
        // fenced out; a 403 says nothing about that and must not inflate it.
        int streak = 2;
        CroniqRunner.UpdateConflictStreak(HttpStatusCode.Forbidden, ref streak, 3).ShouldBeTrue();
        streak.ShouldBe(2);
    }

    [Fact]
    public void RunnerOwnershipDeniedException_CarriesRunnerIdAndRemedy()
    {
        var ex = new RunnerOwnershipDeniedException("runner-42");

        ex.RunnerId.ShouldBe("runner-42");
        ex.Message.ShouldContain("runner-42");
        ex.Message.ShouldContain("DELETE /v1/runners/{id}");
        ex.InnerException.ShouldBeNull();
    }

    [Fact]
    public void PollInstanceConflictException_CarriesRunnerIdAndCount()
    {
        // Spot-check the exception shape since hosts may want to handle
        // it specifically (e.g. distinguish from generic startup errors).
        var ex = new PollInstanceConflictException("runner-42", 3);

        ex.RunnerId.ShouldBe("runner-42");
        ex.ConsecutiveCount.ShouldBe(3);
        ex.Message.ShouldContain("runner-42");
        ex.Message.ShouldContain("3");
        ex.InnerException.ShouldBeNull();
    }
}
