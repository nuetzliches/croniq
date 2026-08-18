package io.croniq.runner;

/**
 * Thrown from {@link CroniqRunner#run()} when {@code POST /v1/work/poll} answers
 * {@code 409 Conflict}
 * {@link io.croniq.runner.config.CroniqRunnerOptions#maxConsecutivePollConflicts()}
 * times in a row: another process is already registered under this
 * {@code runner_id} and keeps winning the identity (fencing, server issue #374).
 *
 * <p>A single {@code 409} is transient — the deposed instance may legitimately
 * take its identity back — so the runner backs off and retries. A streak of them
 * is not: it is a duplicate deployment, two processes started with the same fixed
 * {@code runner_id}. Retrying forever there leaves the misconfiguration behind a
 * warning that scrolls past, so the runner bails instead (issue #134 sub-item 1).
 *
 * <p>Distinct from {@link CroniqOwnershipDeniedException}, which is a {@code 403}
 * and permanent from the first response.
 *
 * <p>Unchecked so it propagates out of {@code run()} without widening the declared
 * checked-exception set; hosts should let it reach {@code main} so the process
 * exits non-zero and the misconfiguration reaches monitoring.
 */
public class CroniqPollInstanceConflictException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    private final String runnerId;
    private final int consecutiveCount;

    public CroniqPollInstanceConflictException(String runnerId, int consecutiveCount, Throwable cause) {
        super(
                "poll instance conflict — another runner is already registered with runner_id '"
                        + runnerId
                        + "'. Observed "
                        + consecutiveCount
                        + " consecutive 409 Conflict responses on POST /v1/work/poll. "
                        + "Stop the duplicate process or rotate the runner_id.",
                cause);
        this.runnerId = runnerId;
        this.consecutiveCount = consecutiveCount;
    }

    /**
     * The {@code runner_id} the conflicts were observed for. Operators can grep
     * {@code runner_id=<value>} in the server's audit log to find the duplicate.
     */
    public String runnerId() {
        return runnerId;
    }

    /**
     * The streak length observed before bailing, equal to
     * {@link io.croniq.runner.config.CroniqRunnerOptions#maxConsecutivePollConflicts()}
     * at throw time.
     */
    public int consecutiveCount() {
        return consecutiveCount;
    }
}
