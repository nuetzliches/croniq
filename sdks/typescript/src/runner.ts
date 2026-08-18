import { randomBytes } from 'node:crypto';
import { setMaxListeners } from 'node:events';

import { anySignal } from './abort.js';
import { CroniqClient, HttpError } from './client.js';
import { sleep } from './deferred.js';
import { ExecutionDispatcher } from './dispatcher.js';
import {
  HandlerRegistry,
  type JobHandler,
  type JobRegistrationOptions,
} from './handler.js';
import { resolveRunnerId } from './identity.js';
import { consoleLogger, type Logger } from './logger.js';
import { resolveOptions, type CroniqRunnerOptions, type ResolvedRunnerOptions } from './options.js';
import type {
  PollRequest,
  PollResponse,
  RegisterJobRequest,
  WorkAssignment,
} from './protocol.js';
import {
  isSafeExecutionId,
  previewForLog,
  type RejectedField,
  rejectAssignmentReason,
  rejectionAckError,
} from './sanitize.js';

/**
 * The Croniq runner: polls the server for work, dispatches handlers
 * concurrently up to {@link CroniqRunnerOptions.maxInflight}, renews leases
 * for in-flight executions, and reports completion.
 *
 * Call {@link run} to drive the loop yourself. Wire `SIGTERM`/`SIGINT` to
 * the AbortController you pass in for graceful shutdown.
 */
export class CroniqRunner {
  readonly #options: ResolvedRunnerOptions;
  readonly #client: CroniqClient;
  readonly #registry = new HandlerRegistry();
  readonly #logger: Logger;
  readonly #instanceId = randomBytes(16).toString('hex');
  readonly #inflight = new Map<string, AbortController>();
  readonly #drainAC = new AbortController();

  #runnerId: string | undefined;
  #dispatcher: ExecutionDispatcher | undefined;
  #ran = false;

  constructor(options: CroniqRunnerOptions) {
    const defaultLogger = consoleLogger('warn', 'croniq');
    this.#options = resolveOptions(options, defaultLogger);
    this.#logger = this.#options.logger;
    this.#client = new CroniqClient({
      baseUrl: this.#options.serverUrl,
      apiKey: this.#options.apiKey,
      bearerToken: this.#options.bearerToken,
      logger: this.#logger,
    });
  }

  /**
   * Register a handler for a specific `job_key`. If {@link JobRegistrationOptions.schedule}
   * is set, the runner calls `POST /v1/jobs/register` for the job at startup
   * (DSL precedence still applies server-side).
   */
  handle(jobKey: string, handler: JobHandler, opts?: JobRegistrationOptions): this {
    this.#registry.register(jobKey, handler, opts);
    return this;
  }

  /** Register a fallback handler for any job key that has no specific handler. */
  setDefaultHandler(handler: JobHandler): this {
    this.#registry.registerDefault(handler);
    return this;
  }

  /** The stable runner ID, available after {@link run} has started. */
  get runnerId(): string {
    if (!this.#runnerId) throw new Error('runnerId is only available after run() starts');
    return this.#runnerId;
  }

  /** Snapshot of currently in-flight execution IDs. Diagnostic only. */
  get inflight(): readonly string[] {
    return [...this.#inflight.keys()];
  }

  /**
   * Run the poll/dispatch/ack loop until {@link signal} aborts. Returns when
   * the token signals AND all in-flight executions have either finished or
   * the drain timeout elapses.
   */
  async run(signal: AbortSignal = new AbortController().signal): Promise<void> {
    if (this.#ran) throw new Error('CroniqRunner.run() may only be called once per instance');
    this.#ran = true;

    this.#runnerId = resolveRunnerId(
      {
        runnerId: this.#options.runnerId,
        runnerIdPrefix: this.#options.runnerIdPrefix,
        runnerDataDir: this.#options.runnerDataDir,
      },
      this.#logger,
    );

    this.#dispatcher = new ExecutionDispatcher({
      client: this.#client,
      registry: this.#registry,
      options: this.#options,
      runnerId: this.#runnerId,
      runnerTags: this.#options.tags,
      logger: this.#logger,
    });

    this.#logger.info('Croniq runner starting', {
      runner_id: this.#runnerId,
      capabilities: this.#options.capabilities.join(',') || '<none>',
      max_inflight: this.#options.maxInflight,
    });

    await this.#selfRegisterSchedules(signal);

    const loopSignal = anySignal(signal, this.#drainAC.signal);
    // The runner's loop signal is observed by every poll, every sleep, and
    // every in-flight dispatch — well above Node's default ceiling of 10
    // abort listeners. Lift the cap rather than letting the runtime emit a
    // misleading "memory leak" warning. (Each listener is cleaned up by its
    // owner; the count just legitimately grows with `maxInflight`.)
    setMaxListeners(0, loopSignal);
    try {
      await this.#pollLoop(loopSignal);
    } finally {
      await this.#drain();
    }
  }

  /**
   * Signal graceful shutdown without waiting. Cancels new polls; in-flight
   * handlers keep running until they complete or the drain-timeout elapses.
   */
  requestDrain(): void {
    if (!this.#drainAC.signal.aborted) this.#drainAC.abort();
  }

  async #selfRegisterSchedules(signal: AbortSignal): Promise<void> {
    for (const entry of this.#registry.selfRegister) {
      try {
        const payload: RegisterJobRequest = {
          job_key: entry.jobKey,
          schedule: entry.schedule,
          runner_id: this.#runnerId!,
          capabilities: this.#options.capabilities,
        };
        if (entry.timeout !== undefined) payload.timeout = entry.timeout;
        if (entry.description !== undefined) payload.description = entry.description;
        await this.#client.registerJob(payload, signal);
      } catch (err) {
        this.#logger.warn(
          `self-register for job ${entry.jobKey} failed — runner will still poll, ` +
            'but the server may not have a schedule',
          { job_key: entry.jobKey, error: String(err) },
        );
      }
    }
  }

  async #pollLoop(signal: AbortSignal): Promise<void> {
    while (!signal.aborted) {
      // Control-slot polling (issue #176): even at capacity we still poll
      // so the server can deliver cancels via PollResponse.cancel. The
      // server returns immediately on capacity=0 (no long-poll), so
      // capacityBackoffMs paces the loop and prevents a stampede.
      const atCapacity = this.#inflight.size >= this.#options.maxInflight;

      const request: PollRequest = {
        runner_id: this.#runnerId!,
        capabilities: this.#options.capabilities,
        max_inflight: this.#options.maxInflight,
        inflight: [...this.#inflight.keys()],
        instance_id: this.#instanceId,
        tags: this.#options.tags,
      };

      let response: PollResponse;
      try {
        response = await this.#client.poll(request, this.#options.pollTimeoutMs, signal);
      } catch (err) {
        if (signal.aborted) return;
        const detail = err instanceof HttpError
          ? { status: err.status, status_text: err.statusText }
          : { error: String(err) };
        this.#logger.warn(`poll failed — backing off ${this.#options.pollRetryDelayMs}ms`, detail);
        try {
          await sleep(this.#options.pollRetryDelayMs, signal);
        } catch {
          return;
        }
        continue;
      }

      this.#handleCancellations(response.cancel);

      if (atCapacity) {
        // Work is always empty in this branch (server-side capacity
        // check); cancels above are already processed. Pace the loop.
        try {
          await sleep(this.#options.capacityBackoffMs, signal);
        } catch {
          return;
        }
        continue;
      }

      for (const assignment of response.work) {
        // Ingest guard: an assignment carrying a control character in either
        // identifier never reaches a handler, a log record or a telemetry
        // attribute. See sanitize.ts for the rule and why it is a denylist.
        const rejected = rejectAssignmentReason(assignment.execution_id, assignment.job_key);
        if (rejected !== undefined) {
          await this.#rejectAssignment(assignment, rejected, signal);
          continue;
        }
        if (this.#inflight.has(assignment.execution_id)) continue;
        const ac = new AbortController();
        this.#inflight.set(assignment.execution_id, ac);

        // Fire-and-forget; the inflight map cleans up in the .finally().
        void this.#dispatcher!
          .dispatch(assignment, ac, signal)
          .finally(() => {
            this.#inflight.delete(assignment.execution_id);
          });
      }
    }
  }

  /**
   * Handle a work assignment refused by the ingest guard.
   *
   * The two cases differ in what the runner can still tell the server:
   *
   * - **Unsafe `execution_id`** — nothing. That value is what addresses an ack
   *   or renew, so there is no way to report anything about this execution.
   *   The assignment is dropped and the server's lease expires.
   * - **Unsafe `job_key`, valid `execution_id`** — a failure ack. The handler
   *   never runs, but the execution completes with an error naming the
   *   offending field, so the operator sees a dead-lettered execution instead
   *   of an execution that is silently requeued by the stale-claim reaper and
   *   refused again on every later poll.
   *
   * Awaited rather than fire-and-forget: this path only triggers on malformed
   * input, so pausing the loop for one small POST costs nothing and keeps the
   * ordering observable.
   */
  async #rejectAssignment(
    assignment: WorkAssignment,
    field: RejectedField,
    signal: AbortSignal,
  ): Promise<void> {
    // Escaped and truncated explicitly: this is the one place a refused value
    // is rendered, and it is hostile by definition.
    const preview = previewForLog(assignment[field]);
    const ackable = field === 'job_key';
    this.#logger.warn('rejected work assignment with unsafe identifier', {
      field,
      value: preview,
      acked: ackable,
    });
    if (!ackable) return;
    try {
      await this.#client.ack(
        {
          runner_id: this.#runnerId!,
          execution_id: assignment.execution_id,
          status: 'failure',
          attempt: assignment.attempt,
          duration_ms: 0,
          error: rejectionAckError(field, assignment[field]),
        },
        signal,
      );
    } catch (err) {
      this.#logger.warn('failed to ack a rejected work assignment', {
        execution_id: assignment.execution_id,
        error: String(err),
      });
    }
  }

  #handleCancellations(cancelIds: string[]): void {
    if (cancelIds.length === 0) return;
    for (const id of cancelIds) {
      // Cancel ids are server-supplied too. An unsafe one can never match an
      // in-flight key (those were validated on ingest), but checking here
      // keeps the value out of the log line below on any code path.
      if (!isSafeExecutionId(id)) continue;
      const ac = this.#inflight.get(id);
      if (ac && !ac.signal.aborted) {
        this.#logger.info('server requested cancellation', { execution_id: id });
        ac.abort();
      }
    }
  }

  async #drain(): Promise<void> {
    if (this.#inflight.size === 0) return;
    this.#logger.info(
      `draining ${this.#inflight.size} in-flight execution(s) (timeout ${this.#options.drainTimeoutMs}ms)`,
      { count: this.#inflight.size, timeout_ms: this.#options.drainTimeoutMs },
    );
    const deadline = Date.now() + this.#options.drainTimeoutMs;
    while (this.#inflight.size > 0 && Date.now() < deadline) {
      await sleep(50);
    }
    if (this.#inflight.size > 0) {
      this.#logger.warn(
        `drain timed out with ${this.#inflight.size} execution(s) still in-flight — hard-cancelling`,
        { count: this.#inflight.size },
      );
      for (const ac of this.#inflight.values()) {
        if (!ac.signal.aborted) ac.abort();
      }
      // Give the dispatcher one tick to process the abort and ack.
      const hardDeadline = Date.now() + 1_000;
      while (this.#inflight.size > 0 && Date.now() < hardDeadline) {
        await sleep(25);
      }
    }
  }
}

/** Convenience factory. Equivalent to `new CroniqRunner(options)`. */
export function createRunner(options: CroniqRunnerOptions): CroniqRunner {
  return new CroniqRunner(options);
}
