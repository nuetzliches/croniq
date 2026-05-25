package io.croniq.runner.spring;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Marks a Spring bean method as a Croniq job handler. The method must accept
 * a single {@link io.croniq.runner.handler.CroniqExecutionContext} argument
 * and may throw any {@link Exception} — exceptions are reported to the
 * server as {@code status=failure} acks.
 *
 * <p>If {@link #schedule()} is set, the runner self-registers the job at
 * startup via {@code POST /v1/jobs/register}. Schedule format follows the
 * Croniqfile DSL ({@code "5m"}, {@code "*&#47;15 * * * *"}, …).
 *
 * <p>Example:
 *
 * <pre>{@code
 * @Component
 * public class BillingJobs {
 *     @CroniqJob(key = "billing:invoice", schedule = "5m")
 *     public void handleInvoice(CroniqExecutionContext ctx) {
 *         // …
 *     }
 * }
 * }</pre>
 */
@Target(ElementType.METHOD)
@Retention(RetentionPolicy.RUNTIME)
public @interface CroniqJob {

    /** Job key the server uses to dispatch executions (e.g., {@code "billing:invoice"}). */
    String key();

    /** Optional schedule string. When set, the runner self-registers the job at startup. */
    String schedule() default "";
}
