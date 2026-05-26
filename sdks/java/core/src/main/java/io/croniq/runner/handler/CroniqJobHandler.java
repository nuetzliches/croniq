package io.croniq.runner.handler;

/**
 * Functional handler interface for a Croniq job. Implementations are invoked
 * once per execution; the SDK acks success on normal return and failure on
 * any thrown exception (including {@link InterruptedException} from a
 * cancelled execution).
 */
@FunctionalInterface
public interface CroniqJobHandler {
    /**
     * Handles one execution.
     *
     * @param ctx execution context — execution id, job key, attempt, metadata,
     *            logger, and a {@link CroniqCancellation} token.
     * @throws Exception any thrown exception is treated as a failed execution;
     *                   its message becomes the {@code error} on the ack.
     */
    void handle(CroniqExecutionContext ctx) throws Exception;
}
