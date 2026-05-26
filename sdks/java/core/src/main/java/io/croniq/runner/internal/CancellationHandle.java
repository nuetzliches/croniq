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
    public void attach(Thread thread) {
        this.worker = thread;
    }

    /** Flip to cancelled and interrupt the worker if attached. Idempotent. */
    public void cancel() {
        if (requested.compareAndSet(false, true)) {
            Thread t = worker;
            if (t != null) {
                t.interrupt();
            }
        }
    }
}
