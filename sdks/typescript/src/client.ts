import type {
  AckRequest,
  PollRequest,
  PollResponse,
  RegisterJobRequest,
  RegisterJobResponse,
  RenewRequest,
  WorkEvent,
} from './protocol.js';
import { AbortError } from './deferred.js';
import type { Logger } from './logger.js';
import { trimTrailingSlashes } from './url.js';

export interface CroniqClientOptions {
  baseUrl: string;
  apiKey?: string | undefined;
  bearerToken?: string | undefined;
  /** Custom `fetch` implementation. Defaults to the global `fetch`. */
  fetchImpl?: typeof fetch;
  logger?: Logger | undefined;
}

export class HttpError extends Error {
  constructor(
    public readonly status: number,
    public readonly statusText: string,
    public readonly path: string,
    public readonly body: string,
  ) {
    super(`${status} ${statusText} on ${path}`);
    this.name = 'HttpError';
  }
}

/**
 * HTTP client for the Croniq runner API. Adds `Authorization` per request
 * (ApiKey takes precedence over Bearer when both are set) and serialises
 * snake_case JSON in/out.
 */
export class CroniqClient {
  readonly #baseUrl: string;
  readonly #apiKey: string | undefined;
  readonly #bearerToken: string | undefined;
  readonly #fetch: typeof fetch;
  readonly #logger: Logger | undefined;

  constructor(opts: CroniqClientOptions) {
    this.#baseUrl = trimTrailingSlashes(opts.baseUrl);
    this.#apiKey = opts.apiKey;
    this.#bearerToken = opts.bearerToken;
    this.#fetch = opts.fetchImpl ?? fetch;
    this.#logger = opts.logger;
  }

  async poll(req: PollRequest, timeoutMs: number, signal: AbortSignal): Promise<PollResponse> {
    const body = await this.#send<PollResponse>('POST', '/v1/work/poll', req, signal, timeoutMs);
    return body ?? { work: [], cancel: [] };
  }

  async ack(req: AckRequest, signal: AbortSignal): Promise<void> {
    await this.#send<void>('POST', '/v1/work/ack', req, signal);
  }

  async renew(req: RenewRequest, signal: AbortSignal): Promise<void> {
    await this.#send<void>('POST', '/v1/work/renew', req, signal);
  }

  async pushEvents(executionId: string, events: WorkEvent[], signal: AbortSignal): Promise<void> {
    if (events.length === 0) return;
    const path = `/v1/work/${encodeURIComponent(executionId)}/events`;
    await this.#send<void>('POST', path, events, signal);
  }

  async registerJob(req: RegisterJobRequest, signal: AbortSignal): Promise<RegisterJobResponse | undefined> {
    const body = await this.#send<RegisterJobResponse>('POST', '/v1/jobs/register', req, signal);
    if (body?.status === 'skipped_dsl_precedence') {
      this.#logger?.info(
        `job ${body.job_key} is managed by the Croniqfile (DSL precedence) — schedule registration skipped`,
        { job_key: body.job_key },
      );
    }
    return body;
  }

  async #send<T>(
    method: string,
    path: string,
    body: unknown,
    outerSignal: AbortSignal,
    timeoutMs?: number,
  ): Promise<T | undefined> {
    const url = `${this.#baseUrl}${path}`;
    const headers: Record<string, string> = {
      'content-type': 'application/json',
      accept: 'application/json',
    };
    if (this.#apiKey) {
      headers.authorization = `ApiKey ${this.#apiKey}`;
    } else if (this.#bearerToken) {
      headers.authorization = `Bearer ${this.#bearerToken}`;
    }

    const { signal, dispose } = composeSignals(outerSignal, timeoutMs);

    let response: Response;
    try {
      response = await this.#fetch(url, {
        method,
        headers,
        body: JSON.stringify(body),
        signal,
      });
    } catch (err) {
      if (isAbortLikeError(err)) {
        throw new AbortError(`request to ${path} aborted`);
      }
      throw err;
    } finally {
      dispose();
    }

    if (!response.ok) {
      const text = await response.text().catch(() => '');
      throw new HttpError(response.status, response.statusText, path, text);
    }

    // 204 No Content / empty body — return undefined so callers see `null`-ish.
    if (response.status === 204) return undefined;
    const text = await response.text();
    if (text.length === 0) return undefined;
    try {
      return JSON.parse(text) as T;
    } catch {
      // Some endpoints respond 200 with a non-JSON body — treat as void.
      return undefined;
    }
  }
}

interface ComposedSignal {
  signal: AbortSignal;
  /** Removes the listener we attached to `outer`. Must be called in a finally. */
  dispose: () => void;
}

function composeSignals(outer: AbortSignal, timeoutMs?: number): ComposedSignal {
  if (timeoutMs == null) {
    return { signal: outer, dispose: () => {} };
  }
  // Node 18 doesn't have AbortSignal.any; build one by hand. The caller MUST
  // invoke dispose() in a finally — otherwise long-lived outer signals
  // (e.g. the runner's loop signal) accumulate one listener per poll.
  const ac = new AbortController();
  let timer: NodeJS.Timeout | undefined;
  const onOuterAbort = (): void => {
    if (timer) clearTimeout(timer);
    ac.abort(outer.reason);
  };
  const dispose = (): void => {
    if (timer) clearTimeout(timer);
    outer.removeEventListener('abort', onOuterAbort);
  };
  if (outer.aborted) {
    ac.abort(outer.reason);
    return { signal: ac.signal, dispose };
  }
  timer = setTimeout(() => {
    outer.removeEventListener('abort', onOuterAbort);
    ac.abort(new AbortError('timeout'));
  }, timeoutMs);
  outer.addEventListener('abort', onOuterAbort, { once: true });
  return { signal: ac.signal, dispose };
}

function isAbortLikeError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  if (err.name === 'AbortError') return true;
  // undici exposes the original cause for fetch aborts in some Node versions.
  const cause = (err as { cause?: unknown }).cause;
  return cause instanceof Error && cause.name === 'AbortError';
}
