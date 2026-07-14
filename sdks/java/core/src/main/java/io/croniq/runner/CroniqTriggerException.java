package io.croniq.runner;

/**
 * Thrown by {@link CroniqTriggerClient} when a trigger cannot be completed —
 * a non-2xx response, a transport failure, or a serialisation error.
 *
 * <p>{@link #statusCode()} carries the HTTP status when the failure came from a
 * response (0 for transport / serialisation errors), so a producer batching or
 * retrying triggers can observe backpressure rather than silently dropping it:
 * {@link #isQueueOverflow()} is the {@code 429} the server returns once a job's
 * per-job queue-depth cap is reached
 * (<a href="https://github.com/nuetzliches/croniq/issues/299">#299</a>).
 */
public final class CroniqTriggerException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    private final int statusCode;

    /** For a non-2xx response — {@code statusCode} is the HTTP status. */
    public CroniqTriggerException(String message, int statusCode) {
        super(message);
        this.statusCode = statusCode;
    }

    /** For a transport / serialisation failure with no HTTP status. */
    public CroniqTriggerException(String message, Throwable cause) {
        super(message, cause);
        this.statusCode = 0;
    }

    /** HTTP status that triggered the failure, or {@code 0} if none (transport error). */
    public int statusCode() {
        return statusCode;
    }

    /**
     * {@code true} when the trigger was rejected with {@code 429 Too Many
     * Requests} because the job's queued executions are at the per-job cap
     * (issue #299). A producer should back off and retry rather than treat this
     * as a permanent failure.
     */
    public boolean isQueueOverflow() {
        return statusCode == 429;
    }
}
