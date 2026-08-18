package io.croniq.runner;

import java.io.IOException;

/**
 * A Croniq API call returned a non-2xx status.
 *
 * <p>Carries the status code as a field rather than only in the message text,
 * so callers can branch on it — the poll loop distinguishes a permanent
 * {@code 403} from a transient {@code 503} this way, and no caller has to
 * parse an exception message to do it (issue #437).
 *
 * <p>Extends {@link IOException} so it stays source-compatible with the
 * {@code throws IOException} signatures the wire layer already declares.
 */
public class CroniqHttpException extends IOException {

    private static final long serialVersionUID = 1L;

    private final int statusCode;
    private final String operation;
    private final String body;

    public CroniqHttpException(String operation, int statusCode, String body) {
        super("Croniq " + operation + " returned HTTP " + statusCode + ": " + body);
        this.operation = operation;
        this.statusCode = statusCode;
        this.body = body;
    }

    /** The HTTP status the server returned. */
    public int statusCode() {
        return statusCode;
    }

    /** The client operation that failed — {@code poll}, {@code ack}, {@code renew}, … */
    public String operation() {
        return operation;
    }

    /** The response body, truncated to a log-safe snippet. May be empty, never null. */
    public String body() {
        return body;
    }

    /**
     * True when the server refused this runner's ownership of the
     * {@code runner_id} it named — {@code 403} on a work endpoint (issue
     * #436). Permanent: retrying cannot clear it.
     */
    public boolean isOwnershipDenied() {
        return statusCode == 403;
    }

    /**
     * True when the server fenced this runner instance out — {@code 409} on the poll
     * endpoint, meaning a newer instance registered under the same {@code runner_id}
     * (issue #374). Transient on its own; only a streak of them is fatal, see
     * {@link CroniqPollInstanceConflictException}.
     */
    public boolean isInstanceConflict() {
        return statusCode == 409;
    }
}
