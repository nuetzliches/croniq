import { anySignal } from './abort.js';
import type { CroniqClient } from './client.js';
import { ExecutionContextImpl } from './context.js';
import { sleep } from './deferred.js';
import { parseScheduledFor, parseTimeoutMs } from './duration.js';
import { LogEnrichment } from './enrichment.js';
import { type HandlerRegistry, NoHandlerRegisteredError } from './handler.js';
import { scopedLogger, type Logger } from './logger.js';
import type { ResolvedRunnerOptions } from './options.js';
import type { AckRequest, AckStatus, WorkAssignment } from './protocol.js';

export interface DispatcherDeps {
  client: CroniqClient;
  registry: HandlerRegistry;
  options: ResolvedRunnerOptions;
  runnerId: string;
  runnerTags: readonly string[];
  logger: Logger;
}

/**
 * Owns the lifecycle of a single in-flight execution: builds the
 * {@link ExecutionContext}, runs the handler under a per-execution
 * AbortSignal, periodically renews the work-item lease, drains the
 * streaming log writer (if used), and acks the outcome.
 */
export class ExecutionDispatcher {
  readonly #deps: DispatcherDeps;

  constructor(deps: DispatcherDeps) {
    this.#deps = deps;
  }

  /**
   * Dispatch a single work assignment.
   *
   * @param executionAC – AbortController owned by the runner; aborted on
   *   server-initiated cancel or drain-timeout.
   * @param outerSignal – host shutdown signal. Used to distinguish "draining"
   *   from "server-cancelled" when classifying errors.
   */
  async dispatch(
    assignment: WorkAssignment,
    executionAC: AbortController,
    outerSignal: AbortSignal,
  ): Promise<void> {
    const { client, registry, options, runnerId, runnerTags, logger } = this.#deps;
    const executionId = assignment.execution_id;
    const jobKey = assignment.job_key;
    const attempt = assignment.attempt;
    const timeoutMs = parseTimeoutMs(assignment.timeout) ?? 5 * 60_000;
    // Original logical fire time; a missing or unparseable value yields null
    // rather than falling back to fire_at (see ExecutionContext.scheduledFor).
    const scheduledFor = parseScheduledFor(assignment.scheduled_for);

    const handlerLogger = scopedLogger(logger, {
      execution_id: executionId,
      job_key: jobKey,
      runner_id: runnerId,
      attempt,
    });
    const enrichment = new LogEnrichment(jobKey, runnerId, runnerTags);

    const ctx = new ExecutionContextImpl({
      executionId,
      jobKey,
      scheduledFor,
      attempt,
      metadata: assignment.metadata,
      timeoutMs,
      runnerId,
      runnerTags,
      signal: executionAC.signal,
      logger: handlerLogger,
      client,
      enrichment,
      logWriterOptions: options.logWriter,
      sdkLogger: logger,
    });

    // Lease-renewal loop runs alongside the handler. Linked to the
    // execution's abort signal so server-cancel/drain stops it.
    const renewAC = new AbortController();
    const renewSignal = anySignal(executionAC.signal, renewAC.signal);
    const renewTask = this.#renewLoop(executionId, runnerId, renewSignal);

    const startedAt = Date.now();
    let status: AckStatus;
    let error: string | undefined;

    try {
      const handler = registry.resolve(jobKey);
      if (!handler) throw new NoHandlerRegisteredError(jobKey);
      await handler(ctx);
      status = 'success';
    } catch (err) {
      if (executionAC.signal.aborted && outerSignal.aborted) {
        status = 'failure';
        error = 'runner draining';
      } else if (executionAC.signal.aborted) {
        status = 'failure';
        error = 'cancelled by server';
      } else {
        const ex = err instanceof Error ? err : new Error(String(err));
        // Identifiers travel as fields only, never interpolated into the
        // message — see sanitize.ts. The same applies to every log call below.
        logger.warn('job handler threw', {
          error: ex.message,
          job_key: jobKey,
          execution_id: executionId,
        });
        status = 'failure';
        error = ex.message;
      }
    } finally {
      renewAC.abort();
      try {
        await renewTask;
      } catch {
        // expected on cancellation
      }
    }

    const durationMs = Date.now() - startedAt;

    const writer = ctx.logWriterIfCreated;
    if (writer) {
      try {
        await writer.dispose();
      } catch (err) {
        logger.warn('log_writer drain failed', {
          error: String(err),
          execution_id: executionId,
          job_key: jobKey,
        });
      }
    }

    // Ack uses a fresh AbortController so a draining outer signal cannot
    // cancel the final ack POST. Match the .NET SDK's `CancellationToken.None`.
    const ackAC = new AbortController();
    try {
      const payload: AckRequest = {
        runner_id: runnerId,
        execution_id: executionId,
        status,
        attempt,
        duration_ms: durationMs,
      };
      if (error !== undefined) payload.error = error;
      await client.ack(payload, ackAC.signal);
    } catch (err) {
      logger.error('failed to ack execution', {
        error: String(err),
        execution_id: executionId,
        job_key: jobKey,
      });
    }
  }

  async #renewLoop(executionId: string, runnerId: string, signal: AbortSignal): Promise<void> {
    const { client, options, logger } = this.#deps;
    while (!signal.aborted) {
      try {
        await sleep(options.renewIntervalMs, signal);
      } catch {
        return;
      }
      if (signal.aborted) return;
      try {
        await client.renew({ runner_id: runnerId, execution_id: executionId }, signal);
      } catch (err) {
        if (signal.aborted) return;
        logger.debug('lease renew failed', {
          error: String(err),
          execution_id: executionId,
        });
      }
    }
  }
}
