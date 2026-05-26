package io.croniq.runner.handler;

/**
 * Cancellation handle, modelled after .NET's {@code CancellationToken}. When the
 * server delivers a cancel directive via {@code PollResponse.cancel} (or the
 * runner is draining), the SDK flips this token and interrupts the handler
 * thread — so blocking operations like {@link Thread#sleep(long)} or
 * {@link java.net.http.HttpClient#send(java.net.http.HttpRequest,
 * java.net.http.HttpResponse.BodyHandler) HttpClient.send} naturally throw
 * {@link InterruptedException}.
 *
 * <p>Handlers that loop over CPU work should poll {@link #isRequested()} and
 * exit early, or call {@link #throwIfRequested()} at safe checkpoints.
 */
public interface CroniqCancellation {

    /** Returns {@code true} once cancellation has been requested. Idempotent. */
    boolean isRequested();

    /**
     * Throws {@link CancellationException} if cancellation has been requested.
     * Otherwise returns normally.
     */
    default void throwIfRequested() {
        if (isRequested()) {
            throw new CancellationException();
        }
    }

    /** Thrown by {@link #throwIfRequested()} when a cancel is pending. */
    final class CancellationException extends RuntimeException {
        private static final long serialVersionUID = 1L;

        public CancellationException() {
            super("Execution cancelled by Croniq server");
        }
    }
}
