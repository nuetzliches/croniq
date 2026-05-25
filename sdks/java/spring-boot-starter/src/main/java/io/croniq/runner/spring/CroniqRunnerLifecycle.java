package io.croniq.runner.spring;

import io.croniq.runner.CroniqRunner;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.SmartLifecycle;

/**
 * Bridges {@link CroniqRunner}'s blocking {@code run()} loop to Spring's
 * {@link SmartLifecycle} so the runner starts after the context is fully
 * initialised and stops cleanly during shutdown.
 *
 * <p>Phase: {@link Integer#MAX_VALUE} minus 1000 — late start, early stop.
 * That gives application beans time to finish their own initialisation before
 * the runner accepts work, and lets the runner drain in-flight executions
 * before any data source or message-broker beans are torn down.
 */
final class CroniqRunnerLifecycle implements SmartLifecycle {

    private static final Logger log = LoggerFactory.getLogger(CroniqRunnerLifecycle.class);

    private final CroniqRunner runner;
    private volatile Thread loop;

    CroniqRunnerLifecycle(CroniqRunner runner) {
        this.runner = runner;
    }

    @Override
    public synchronized void start() {
        if (loop != null) {
            return;
        }
        loop = Thread.ofPlatform().name("croniq-runner").daemon(false).start(() -> {
            try {
                runner.run();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            } catch (RuntimeException e) {
                log.error("Croniq runner stopped due to unhandled error", e);
            }
        });
    }

    @Override
    public synchronized void stop() {
        if (loop == null) {
            return;
        }
        runner.close();
        try {
            loop.join();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } finally {
            loop = null;
        }
    }

    @Override
    public boolean isRunning() {
        return loop != null && loop.isAlive();
    }

    /**
     * Spring lifecycle phase. Stop in reverse phase order; later start, earlier
     * stop. We pick a high value to start after data sources and message brokers
     * and stop before they tear down so the drain can still ack final results.
     */
    @Override
    public int getPhase() {
        return Integer.MAX_VALUE - 1000;
    }
}
