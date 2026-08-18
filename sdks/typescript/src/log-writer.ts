import { type CroniqClient, isOwnershipDenied } from './client.js';
import { deferred, sleep, type Deferred } from './deferred.js';
import type { LogEnrichment } from './enrichment.js';
import type { Logger } from './logger.js';
import type { ResolvedLogWriterOptions } from './options.js';
import type { WorkEvent, WorkEventLevel } from './protocol.js';

export interface LogWriter {
  /** Append an event. Awaits a slot if the bounded buffer is full. */
  write(level: WorkEventLevel, message: string, fields?: Record<string, string>): Promise<void>;
  /** Append a pre-built event. */
  writeEvent(event: WorkEvent): Promise<void>;
  /** Block until every queued event has been POSTed. */
  flush(): Promise<void>;
  /**
   * Close the writer and wait for the background flusher to drain, bounded
   * by `shutdownTimeoutMs`. Safe to call multiple times.
   */
  dispose(): Promise<void>;
}

/**
 * Streaming log writer. Backed by a bounded in-memory buffer and a single
 * background flusher coroutine; mirrors the .NET / Rust SDKs:
 *
 *   - Batch by count: flush when {@link ResolvedLogWriterOptions.batchSizeThreshold} events accumulate.
 *   - Batch by time: flush at least every {@link ResolvedLogWriterOptions.batchTimeThresholdMs} ms.
 *   - Drain-before-ack: {@link dispose} waits up to `shutdownTimeoutMs` for the buffer to flush.
 *   - Backpressure: {@link write} awaits when the buffer is at capacity.
 *
 * Enrichment runs at POST time (`job_key`, `runner_id`, `runner_tags`).
 */
export class StreamingLogWriter implements LogWriter {
  readonly #client: CroniqClient;
  readonly #executionId: string;
  readonly #enrichment: LogEnrichment;
  readonly #opts: ResolvedLogWriterOptions;
  readonly #logger: Logger;

  #buffer: WorkEvent[] = [];
  #pendingFlushes: Array<Deferred<void>> = [];
  /** Resolved when a slot frees up in the bounded buffer. */
  #spaceAvailable: Deferred<void> | null = null;
  /** Resolved when new events arrive (wakes the flusher early). */
  #eventsAvailable: Deferred<void> | null = null;
  #closed = false;
  #disposed = false;
  readonly #flusherTask: Promise<void>;

  constructor(
    client: CroniqClient,
    executionId: string,
    enrichment: LogEnrichment,
    opts: ResolvedLogWriterOptions,
    logger: Logger,
  ) {
    this.#client = client;
    this.#executionId = executionId;
    this.#enrichment = enrichment;
    this.#opts = opts;
    this.#logger = logger;
    this.#flusherTask = this.#runFlusher();
  }

  async write(level: WorkEventLevel, message: string, fields?: Record<string, string>): Promise<void> {
    const event: WorkEvent = fields ? { level, message, fields } : { level, message };
    return this.writeEvent(event);
  }

  async writeEvent(event: WorkEvent): Promise<void> {
    while (!this.#closed && this.#buffer.length >= this.#opts.channelCapacity) {
      this.#spaceAvailable ??= deferred();
      await this.#spaceAvailable.promise;
    }
    if (this.#closed) return;
    this.#buffer.push(event);
    this.#wake();
  }

  flush(): Promise<void> {
    if (this.#closed && this.#buffer.length === 0) return Promise.resolve();
    const d = deferred<void>();
    this.#pendingFlushes.push(d);
    this.#wake();
    return d.promise;
  }

  async dispose(): Promise<void> {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#closed = true;
    this.#wake();
    // Release any writers parked on a full buffer so they unblock and observe `closed`.
    this.#releaseWriters();

    let timedOut = false;
    const timeoutController = new AbortController();
    const timeout = sleep(this.#opts.shutdownTimeoutMs, timeoutController.signal).then(
      () => { timedOut = true; },
      () => {},
    );
    await Promise.race([this.#flusherTask, timeout]);
    timeoutController.abort();

    if (timedOut) {
      this.#logger.warn(
        'log_writer drain timed out',
        { execution_id: this.#executionId, timeout_ms: this.#opts.shutdownTimeoutMs },
      );
    }
  }

  #wake(): void {
    if (this.#eventsAvailable) {
      const d = this.#eventsAvailable;
      this.#eventsAvailable = null;
      d.resolve();
    }
  }

  #releaseWriters(): void {
    if (this.#spaceAvailable) {
      const d = this.#spaceAvailable;
      this.#spaceAvailable = null;
      d.resolve();
    }
  }

  async #runFlusher(): Promise<void> {
    try {
      while (!this.#closed || this.#buffer.length > 0 || this.#pendingFlushes.length > 0) {
        // Wait for one of: new events arrived, the time-threshold tick fires,
        // or the writer was closed. The deferred is re-created each iteration
        // so callers awakening it have a fresh promise to settle.
        if (this.#buffer.length === 0 && this.#pendingFlushes.length === 0 && !this.#closed) {
          this.#eventsAvailable = deferred();
          const tickController = new AbortController();
          const tick = sleep(this.#opts.batchTimeThresholdMs, tickController.signal).catch(() => undefined);
          await Promise.race([this.#eventsAvailable.promise, tick]);
          tickController.abort();
          this.#eventsAvailable = null;
        }

        // Drain whatever has accumulated. We pull *everything* — the
        // batch-size threshold is honoured per-POST inside flushAll().
        if (this.#buffer.length > 0) {
          const batch = this.#buffer;
          this.#buffer = [];
          this.#releaseWriters();
          await this.#flushAll(batch);
        }

        if (this.#pendingFlushes.length > 0) {
          // Drain any events that arrived while we were posting the prior batch.
          if (this.#buffer.length > 0) {
            const more = this.#buffer;
            this.#buffer = [];
            this.#releaseWriters();
            await this.#flushAll(more);
          }
          const resolvers = this.#pendingFlushes;
          this.#pendingFlushes = [];
          for (const r of resolvers) r.resolve();
        }
      }
    } catch (err) {
      this.#logger.error('log_writer flusher crashed', {
        error: String(err),
        execution_id: this.#executionId,
      });
    } finally {
      // Reject any remaining flushers so callers don't hang.
      for (const r of this.#pendingFlushes) r.resolve();
      this.#pendingFlushes = [];
      this.#releaseWriters();
    }
  }

  async #flushAll(events: WorkEvent[]): Promise<void> {
    const max = this.#opts.maxBatchPerPost;
    for (let i = 0; i < events.length; i += max) {
      const chunk = events.slice(i, i + max).map((e) => this.#enrichment.enrich(e));
      try {
        // Use a fresh AbortController per POST so client cancellation is
        // localised — a slow batch should not bring down later batches.
        const ac = new AbortController();
        await this.#client.pushEvents(this.#executionId, chunk, ac.signal);
      } catch (err) {
        if (isOwnershipDenied(err)) {
          // Permanent (#436/#437) — every later batch is lost too, so the
          // operator must see this rather than wonder why the execution
          // produced no output.
          this.#logger.error(
            'log_writer: batch POST refused with 403 Forbidden — this runner\'s credential ' +
              'does not own its runner_id, so no log event will reach the server. Give the ' +
              'runner its own runner_id, or release the existing binding with ' +
              'DELETE /v1/runners/{id}',
            { error: String(err), execution_id: this.#executionId, dropped: chunk.length },
          );
          continue;
        }
        this.#logger.warn(
          'log_writer: batch POST failed — events dropped',
          { error: String(err), execution_id: this.#executionId, dropped: chunk.length },
        );
      }
    }
  }
}
