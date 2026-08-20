package io.croniq.runner;

/**
 * Thrown from {@link CroniqRunner#run()} when a work endpoint answers
 * {@code 401 Unauthorized}
 * {@link io.croniq.runner.config.CroniqRunnerOptions#maxConsecutiveAuthFailures()}
 * times in a row: the API key was rejected and keeps being rejected.
 *
 * <p>The credential is read once, when the client is built, and never re-read, so
 * retrying presents the same dead key forever. Before this existed a {@code 401}
 * fell into the generic transient bucket and the runner retried on the poll
 * interval indefinitely: the process stayed up, looked healthy, did nothing, and
 * never exited non-zero — so no supervisor restarted it, and restarting is
 * exactly what would have picked up the new key (issue #473).
 *
 * <p>Not thrown on the first {@code 401}. Key rotation hands over by installing the
 * new key and giving the old one an expiry (server issue #471), and dying on a
 * single {@code 401} would turn a narrow race around that handover into an outage.
 *
 * <p>Distinct from {@link CroniqOwnershipDeniedException}, which is a {@code 403}
 * and permanent from the first response.
 *
 * <p>Unchecked so it propagates out of {@code run()} without widening the declared
 * checked-exception set; hosts should let it reach {@code main} so the process
 * exits non-zero and the dead credential reaches monitoring.
 */
public class CroniqAuthFailedException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    private final int consecutiveCount;

    public CroniqAuthFailedException(int consecutiveCount, Throwable cause) {
        super(
                "unauthorized — the API key was rejected on "
                        + consecutiveCount
                        + " consecutive POST /v1/work/poll attempts. It may have been revoked, or its"
                        + " rotation grace window may have elapsed. Restart the runner with the current"
                        + " key.",
                cause);
        this.consecutiveCount = consecutiveCount;
    }

    /**
     * The streak length observed before bailing, equal to
     * {@link io.croniq.runner.config.CroniqRunnerOptions#maxConsecutiveAuthFailures()}
     * at throw time.
     */
    public int consecutiveCount() {
        return consecutiveCount;
    }
}
