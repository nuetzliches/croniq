// Producer-side trigger client — an idiomatic wrapper over `POST /v1/trigger`.
//
// Parity with the .NET `ICroniqTriggerClient` (#277): fire a registered job on
// demand so the *same* handler serves both its Croniqfile schedule and
// near-real-time, event-driven execution. Deliberately independent of the
// runner: triggering needs the `jobs:trigger` (or `admin`) scope, which runner
// poll keys typically do not carry, so the trigger client takes ITS OWN
// credentials rather than reusing a runner's.

import { HttpError } from './client.js';
import { AbortError } from './deferred.js';
import { composeSignals, isAbortLikeError } from './http.js';
import type { TriggerRequest, TriggerResponse } from './protocol.js';
import { trimTrailingSlashes } from './url.js';

/** Options for {@link CroniqTriggerClient} / {@link createTriggerClient}. */
export interface CroniqTriggerClientOptions {
  /** Base URL of the Croniq server, e.g. `http://localhost:4000`. */
  serverUrl: string;

  /**
   * API key for `Authorization: ApiKey {key}`. Takes precedence over
   * {@link bearerToken} when both are set. Needs the `jobs:trigger` (or
   * `admin`) scope — distinct from a runner's poll scopes.
   */
  apiKey?: string | undefined;

  /** Bearer token for `Authorization: Bearer {token}`. */
  bearerToken?: string | undefined;

  /** Per-request timeout in milliseconds. Default 30 000 ms. */
  requestTimeoutMs?: number | undefined;

  /** Custom `fetch` implementation. Defaults to the global `fetch`. */
  fetchImpl?: typeof fetch | undefined;
}

/** Optional arguments for a single {@link CroniqTriggerClient.trigger} call. */
export interface TriggerParams {
  /**
   * Arbitrary JSON metadata forwarded to the handler as-is (merged over the
   * job's DSL metadata server-side). Keys starting with `__` are reserved.
   */
  metadata?: Record<string, unknown>;

  /** Capabilities a runner MUST have to be assigned this execution. */
  require?: string[];

  /** Capabilities used to prefer runners when several are eligible. */
  prefer?: string[];

  /**
   * Execution timeout as a server duration string (e.g. `"30s"`, `"5m"`).
   * The server default applies when omitted.
   */
  timeout?: string;

  /**
   * Optional dedup key (≤ 200 chars, scoped per `job_key`). Servers with
   * trigger-idempotency support (#279) coalesce repeat triggers carrying the
   * same key onto the existing execution (see {@link TriggerResult.deduplicated});
   * older servers ignore it.
   */
  idempotencyKey?: string;

  /** Abort signal to cancel the underlying HTTP call. */
  signal?: AbortSignal;
}

/** Result of an on-demand job trigger (`POST /v1/trigger`). */
export interface TriggerResult {
  /**
   * Identifier of the execution the trigger resolved to. On a dedup hit this
   * is the EXISTING execution's id, not a new one.
   */
  executionId: string;

  /** Server work-queue depth after the trigger was processed. */
  queued: number;

  /**
   * `true` when the server coalesced this trigger onto an existing execution
   * because the request carried an `idempotency_key` it had already seen.
   * Always `false` on servers without idempotency-key support (they omit the
   * flag and the client defaults it to `false`).
   */
  deduplicated: boolean;
}

/**
 * Thrown when `POST /v1/trigger` is rejected with `429 Too Many Requests`
 * because the job's per-job queue-depth cap is reached (#299). A producer that
 * batches or retries triggers should treat this as backpressure — back off and
 * retry later rather than dropping the work silently. {@link retryAfterMs}
 * carries the server's `Retry-After` hint when present.
 */
export class QueueOverflowError extends HttpError {
  constructor(
    path: string,
    body: string,
    public readonly retryAfterMs?: number,
  ) {
    super(429, 'Too Many Requests', path, body);
    this.name = 'QueueOverflowError';
  }
}

const TRIGGER_PATH = '/v1/trigger';
const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;

/**
 * Client for firing Croniq jobs on demand. Serialises a snake_case JSON body,
 * attaches `Authorization` per request (ApiKey precedence over Bearer), and
 * parses the `TriggerResponse` (defaulting a missing `deduplicated` to
 * `false`). Non-2xx responses surface as errors — `429` as a
 * {@link QueueOverflowError}, everything else as an {@link HttpError}.
 */
export class CroniqTriggerClient {
  readonly #baseUrl: string;
  readonly #apiKey: string | undefined;
  readonly #bearerToken: string | undefined;
  readonly #requestTimeoutMs: number;
  readonly #fetch: typeof fetch;

  constructor(opts: CroniqTriggerClientOptions) {
    if (!opts.serverUrl || typeof opts.serverUrl !== 'string') {
      throw new TypeError('CroniqTriggerClientOptions.serverUrl is required');
    }
    try {
      // eslint-disable-next-line no-new
      new URL(opts.serverUrl);
    } catch {
      throw new TypeError(`CroniqTriggerClientOptions.serverUrl is not a valid URL: ${opts.serverUrl}`);
    }
    this.#baseUrl = trimTrailingSlashes(opts.serverUrl);
    this.#apiKey = opts.apiKey;
    this.#bearerToken = opts.bearerToken;
    this.#requestTimeoutMs = opts.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    this.#fetch = opts.fetchImpl ?? fetch;
  }

  /**
   * Fire a job immediately. Its registered handler runs on the next eligible
   * runner, exactly like a scheduled fire.
   *
   * @param jobKey Job key, e.g. `billing:invoice-generate`.
   * @param params Optional metadata / routing hints / timeout / idempotency key.
   * @returns The created (or deduplicated) execution and queue depth.
   * @throws {QueueOverflowError} on `429` per-job queue overflow (#299).
   * @throws {HttpError} on any other non-2xx response.
   */
  async trigger(jobKey: string, params: TriggerParams = {}): Promise<TriggerResult> {
    if (typeof jobKey !== 'string' || jobKey.trim().length === 0) {
      throw new TypeError('trigger(jobKey): jobKey must be a non-empty string');
    }

    // Only assign supplied optionals: JSON.stringify drops `undefined`, so an
    // unset field never reaches the wire.
    const requestBody: TriggerRequest = { job_key: jobKey };
    if (params.metadata !== undefined) requestBody.metadata = params.metadata;
    if (params.require !== undefined) requestBody.require = params.require;
    if (params.prefer !== undefined) requestBody.prefer = params.prefer;
    if (params.timeout !== undefined) requestBody.timeout = params.timeout;
    if (params.idempotencyKey !== undefined) requestBody.idempotency_key = params.idempotencyKey;

    const headers: Record<string, string> = {
      'content-type': 'application/json',
      accept: 'application/json',
    };
    if (this.#apiKey) {
      headers.authorization = `ApiKey ${this.#apiKey}`;
    } else if (this.#bearerToken) {
      headers.authorization = `Bearer ${this.#bearerToken}`;
    }

    const { signal, dispose } = composeSignals(params.signal, this.#requestTimeoutMs);

    let response: Response;
    try {
      response = await this.#fetch(`${this.#baseUrl}${TRIGGER_PATH}`, {
        method: 'POST',
        headers,
        body: JSON.stringify(requestBody),
        signal,
      });
    } catch (err) {
      if (isAbortLikeError(err)) {
        throw new AbortError(`request to ${TRIGGER_PATH} aborted`);
      }
      throw err;
    } finally {
      dispose();
    }

    if (!response.ok) {
      const text = await response.text().catch(() => '');
      if (response.status === 429) {
        throw new QueueOverflowError(TRIGGER_PATH, text, parseRetryAfterMs(response.headers.get('retry-after')));
      }
      throw new HttpError(response.status, response.statusText, TRIGGER_PATH, text);
    }

    const text = await response.text();
    if (text.length === 0) {
      throw new HttpError(response.status, response.statusText, TRIGGER_PATH, 'empty response body');
    }
    let parsed: TriggerResponse;
    try {
      parsed = JSON.parse(text) as TriggerResponse;
    } catch {
      throw new HttpError(response.status, response.statusText, TRIGGER_PATH, `non-JSON response body: ${text}`);
    }

    return {
      executionId: parsed.execution_id,
      queued: parsed.queued,
      deduplicated: parsed.deduplicated ?? false,
    };
  }
}

/** Convenience factory mirroring `createRunner`. */
export function createTriggerClient(opts: CroniqTriggerClientOptions): CroniqTriggerClient {
  return new CroniqTriggerClient(opts);
}

/**
 * Parse a `Retry-After` header value into milliseconds. Supports both the
 * delta-seconds form (`"30"`) and the HTTP-date form. Returns `undefined` when
 * the header is absent or unparseable.
 */
function parseRetryAfterMs(headerValue: string | null): number | undefined {
  if (!headerValue) return undefined;
  const trimmed = headerValue.trim();
  if (trimmed.length === 0) return undefined;
  const seconds = Number(trimmed);
  if (Number.isFinite(seconds) && seconds >= 0) {
    return Math.round(seconds * 1000);
  }
  const dateMs = Date.parse(trimmed);
  if (!Number.isNaN(dateMs)) {
    return Math.max(0, dateMs - Date.now());
  }
  return undefined;
}
