import type { CroniqClient } from './client.js';
import type { LogEnrichment } from './enrichment.js';
import type { Logger } from './logger.js';
import { StreamingLogWriter, type LogWriter } from './log-writer.js';
import type { ResolvedLogWriterOptions } from './options.js';
import type { WorkEvent, WorkEventLevel } from './protocol.js';

export interface ExecutionContext {
  /** Server-assigned execution identifier. */
  readonly executionId: string;
  /** Job key, e.g. `billing:invoice`. */
  readonly jobKey: string;
  /** 1-based attempt counter, incremented on each retry. */
  readonly attempt: number;
  /**
   * Raw metadata payload from the server. The shape is job-specific — cast
   * to your expected type. Field names are the original snake_case sent by
   * the server (no transformation).
   */
  readonly metadata: unknown;
  /** Server-declared timeout for this execution, in milliseconds. */
  readonly timeoutMs: number;
  /** The runner's stable identifier. */
  readonly runnerId: string;
  /** Free-form tags this runner self-declared. */
  readonly runnerTags: readonly string[];
  /**
   * Combined abort signal: aborts when the server cancels the execution via
   * `PollResponse.cancel`, OR when the runner's drain timeout elapses while
   * still in-flight. Handlers should propagate this to downstream awaits.
   */
  readonly signal: AbortSignal;
  /** Logger pre-scoped with `execution_id`, `job_key`, `runner_id`, `attempt`. */
  readonly logger: Logger;
  /**
   * Streaming log writer that POSTs events to the Croniq server's execution
   * log (visible in the UI). Lazily initialised on first access; the runner
   * drains the writer (bounded by `logWriter.shutdownTimeoutMs`) before
   * sending the ack.
   */
  readonly logWriter: LogWriter;

  /**
   * Push a single event inline (awaits the HTTP POST). For high-volume
   * scenarios, prefer {@link logWriter}.
   */
  pushEvent(level: WorkEventLevel, message: string, fields?: Record<string, string>): Promise<void>;
  /** Push pre-built events inline. */
  pushEvents(events: WorkEvent[]): Promise<void>;
}

export interface ExecutionContextInput {
  executionId: string;
  jobKey: string;
  attempt: number;
  metadata: unknown;
  timeoutMs: number;
  runnerId: string;
  runnerTags: readonly string[];
  signal: AbortSignal;
  logger: Logger;
  client: CroniqClient;
  enrichment: LogEnrichment;
  logWriterOptions: ResolvedLogWriterOptions;
  sdkLogger: Logger;
}

export class ExecutionContextImpl implements ExecutionContext {
  readonly executionId: string;
  readonly jobKey: string;
  readonly attempt: number;
  readonly metadata: unknown;
  readonly timeoutMs: number;
  readonly runnerId: string;
  readonly runnerTags: readonly string[];
  readonly signal: AbortSignal;
  readonly logger: Logger;

  readonly #client: CroniqClient;
  readonly #enrichment: LogEnrichment;
  readonly #logWriterOptions: ResolvedLogWriterOptions;
  readonly #sdkLogger: Logger;
  #logWriter: StreamingLogWriter | null = null;

  constructor(input: ExecutionContextInput) {
    this.executionId = input.executionId;
    this.jobKey = input.jobKey;
    this.attempt = input.attempt;
    this.metadata = input.metadata;
    this.timeoutMs = input.timeoutMs;
    this.runnerId = input.runnerId;
    this.runnerTags = input.runnerTags;
    this.signal = input.signal;
    this.logger = input.logger;
    this.#client = input.client;
    this.#enrichment = input.enrichment;
    this.#logWriterOptions = input.logWriterOptions;
    this.#sdkLogger = input.sdkLogger;
  }

  get logWriter(): LogWriter {
    this.#logWriter ??= new StreamingLogWriter(
      this.#client,
      this.executionId,
      this.#enrichment,
      this.#logWriterOptions,
      this.#sdkLogger,
    );
    return this.#logWriter;
  }

  /** Internal accessor — returns the writer only if a handler actually used it. */
  get logWriterIfCreated(): StreamingLogWriter | null {
    return this.#logWriter;
  }

  pushEvent(level: WorkEventLevel, message: string, fields?: Record<string, string>): Promise<void> {
    const event: WorkEvent = fields ? { level, message, fields } : { level, message };
    return this.pushEvents([event]);
  }

  async pushEvents(events: WorkEvent[]): Promise<void> {
    if (events.length === 0) return;
    const enriched = events.map((e) => this.#enrichment.enrich(e));
    const ac = new AbortController();
    await this.#client.pushEvents(this.executionId, enriched, ac.signal);
  }
}
