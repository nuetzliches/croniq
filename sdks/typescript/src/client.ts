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
import { composeSignals, isAbortLikeError } from './http.js';
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
 * A work endpoint answered `403 Forbidden`: the authenticated credential is
 * bound to a different `runner_id` than the one this runner names in its
 * requests (server issue #436).
 *
 * Unlike a `409` — where a duplicate deployment may release the identity on
 * its own — this is **permanent**: retrying cannot clear it. `CroniqRunner.run`
 * rejects with this instead of polling forever, so a misconfigured runner
 * exits rather than looking merely idle (issue #437). The fix is an operator
 * action: give the runner its own `runner_id`, or release the existing
 * binding with `DELETE /v1/runners/{id}`.
 */
export class RunnerOwnershipDeniedError extends Error {
  constructor(public readonly runnerId: string) {
    super(
      `work ownership denied — the credential this runner authenticates with does not own ` +
        `runner_id '${runnerId}'. The server answered 403 Forbidden on POST /v1/work/poll ` +
        `and will keep doing so: give this runner its own runner_id, or release the ` +
        `existing binding with DELETE /v1/runners/{id}.`,
    );
    this.name = 'RunnerOwnershipDeniedError';
  }
}

/** True when `err` is a 403 from a work endpoint — the ownership refusal. */
export function isOwnershipDenied(err: unknown): boolean {
  return err instanceof HttpError && err.status === 403;
}

/**
 * `POST /v1/work/poll` answered `409 Conflict` `maxConsecutivePollConflicts`
 * times in a row: another process is already registered under this
 * `runner_id` and keeps winning the identity (fencing, server issue #374).
 *
 * A single 409 is transient — a deposed instance may legitimately take its
 * identity back — so the runner backs off and retries. A streak of them is
 * not: it is a duplicate deployment, two processes started with the same
 * fixed `runnerId`. Retrying forever there leaves the misconfiguration behind
 * a warning that scrolls past, so `CroniqRunner.run` rejects with this
 * instead (issue #134 sub-item 1).
 *
 * Distinct from {@link RunnerOwnershipDeniedError}, which is a 403 and
 * permanent from the first response.
 */
export class PollInstanceConflictError extends Error {
  constructor(
    public readonly runnerId: string,
    public readonly consecutiveCount: number,
  ) {
    super(
      `poll instance conflict — another runner is already registered with runner_id ` +
        `'${runnerId}'. Observed ${consecutiveCount} consecutive 409 Conflict responses ` +
        `on POST /v1/work/poll. Stop the duplicate process or rotate the runner_id.`,
    );
    this.name = 'PollInstanceConflictError';
  }
}

/** True when `err` is a 409 from the poll endpoint — the fencing refusal. */
export function isInstanceConflict(err: unknown): boolean {
  return err instanceof HttpError && err.status === 409;
}

/**
 * A work endpoint answered `401 Unauthorized` `maxConsecutiveAuthFailures`
 * times in a row: the API key was rejected and keeps being rejected.
 *
 * The credential is read once, when the client is built, and never re-read,
 * so retrying presents the same dead key forever. Before this existed a 401
 * fell into the generic transient bucket and the runner retried on the poll
 * interval indefinitely: the process stayed up, looked healthy, did nothing,
 * and never exited non-zero — so no supervisor restarted it, and restarting
 * is exactly what would have picked up the new key (issue #473).
 *
 * Not thrown on the first 401. Key rotation hands over by installing the new
 * key and giving the old one an expiry (server issue #471), and dying on a
 * single 401 would turn a narrow race around that handover into an outage.
 */
export class AuthFailedError extends Error {
  constructor(public readonly consecutiveCount: number) {
    super(
      `unauthorized — the API key was rejected on ${consecutiveCount} consecutive ` +
        `POST /v1/work/poll attempts. It may have been revoked, or its rotation grace ` +
        `window may have elapsed. Restart the runner with the current key.`,
    );
    this.name = 'AuthFailedError';
  }
}

/** True when `err` is a 401 from a work endpoint — the rejected credential. */
export function isUnauthorized(err: unknown): boolean {
  return err instanceof HttpError && err.status === 401;
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
