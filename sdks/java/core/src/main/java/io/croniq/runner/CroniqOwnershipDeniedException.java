package io.croniq.runner;

/**
 * Thrown from {@link CroniqRunner#run()} when a work endpoint answers
 * {@code 403 Forbidden}: the authenticated credential is bound to a different
 * {@code runner_id} than the one this runner names in its requests (server
 * issue #436).
 *
 * <p>Unlike a {@code 409} — where a duplicate deployment may release the
 * identity on its own — a {@code 403} is <em>permanent</em>: no number of
 * retries can clear it. The runner therefore bails on the first occurrence
 * instead of polling forever and looking merely idle. An operator has to give
 * this runner its own {@code runner_id}, or release the existing binding with
 * {@code DELETE /v1/runners/{id}}.
 *
 * <p>Unchecked so it propagates out of {@code run()} without widening the
 * declared checked-exception set; hosts should let it reach {@code main} so
 * the process exits non-zero and the misconfiguration reaches monitoring.
 * See issue #437.
 */
public class CroniqOwnershipDeniedException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    private final String runnerId;

    public CroniqOwnershipDeniedException(String runnerId, Throwable cause) {
        super(
                "work ownership denied — the credential this runner authenticates with does not own runner_id '"
                        + runnerId
                        + "'. The server answered 403 Forbidden on POST /v1/work/poll and will keep doing so: "
                        + "give this runner its own runner_id, or release the existing binding with "
                        + "DELETE /v1/runners/{id}.",
                cause);
        this.runnerId = runnerId;
    }

    /**
     * The {@code runner_id} the credential was refused for. Operators can grep
     * {@code runner_id=<value>} in the server's audit log to find the
     * credential that actually owns it.
     */
    public String runnerId() {
        return runnerId;
    }
}
