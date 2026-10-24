package io.croniq.runner.internal;

import io.croniq.runner.handler.CroniqCancellation;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Internal cancellation primitive. Tracks the requested flag and the worker
 * thread so a server cancel can both set the flag (for handlers that poll
 * {@link CroniqCancellation#isRequested()}) and interrupt blocking I/O.
 */
public final class CancellationHandle implements CroniqCancellation {

    private final AtomicBoolean requested = new AtomicBoolean(false);
    private volatile Thread worker;

    @Override
    public boolean isRequested() {
        return requested.get();
    }

    /** Bind the worker thread once it begins executing the handler. */
    public synchronized void attach(Thread thread) {
        this.worker = thread;
    }

    /**
     * Unbind the worker thread: no later {@link #cancel()} interrupts it. Called
     * once the handler has returned, before the ack. The interrupt happens under
     * the same lock, so once this returns no cancel can still be about to
     * interrupt the thread, and clearing the thread's interrupt flag afterwards
     * is final (review 2026-10-24, K-5).
     */
    public synchronized void detach() {
        this.worker = null;
    }

    /** Flip to cancelled and interrupt the worker if attached. Idempotent. */
    public void cancel() {
        if (requested.compareAndSet(false, true)) {
            synchronized (this) {
                Thread t = worker;
                if (t != null) {
                    t.interrupt();
                }
            }
        }
    }
}
