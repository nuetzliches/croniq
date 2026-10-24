// Retry pacing shared by the poll loop and the ack path. Internal: nothing here
// is exported from index.ts.

import type { CroniqClient } from './client.js';
import { HttpError } from './client.js';
import { sleep } from './deferred.js';
import type { Logger } from './logger.js';
import type { AckRequest } from './protocol.js';

/**
 * Scales `ms` by a uniform factor in [0.8, 1.2].
 *
 * Review 2026-10-24, K-9: with a fixed poll retry delay every runner that saw
 * the same server outage retries in lockstep, so a restarting server is hit by
 * all of them at once. ±20 % spreads them out and stays inside what
 * conformance case 12 pins (at most six polls in 1.5 s at a 400 ms delay).
 */
export function jittered(ms: number, random: () => number = Math.random): number {
  return Math.round(ms * (0.8 + 0.4 * random()));
}

/**
 * Pauses before the second and third ack attempt. Mutable so tests can shrink
 * them; not part of the public API.
 */
export const ackRetry: { delaysMs: readonly number[] } = { delaysMs: [1_000, 2_000] };

/**
 * True for an ack failure worth repeating: a transport error (connection
 * refused or reset, DNS, the per-request timeout) or a 5xx. A 4xx is the
 * server's verdict on this ack — 409 already terminal or not ours, 404 gone,
 * 403 not our runner_id — and sending it again cannot change the answer.
 */
export function isRetryableAckError(err: unknown): boolean {
  if (err instanceof HttpError) return err.status >= 500;
  return true;
}

/**
 * Posts an ack, retrying transient failures. Throws the last error once the
 * attempts are spent or the failure is not retryable, so the caller keeps its
 * own final error logging.
 *
 * Review 2026-10-24, K-1: the ack used to be fire-once. A handler that ran for
 * twenty minutes and then lost its ack to a connection reset or a request
 * timeout was dropped from the in-flight set, the stale-claim reaper requeued
 * it, and the job ran a second time. The ack is idempotent on the server
 * (CAS-fenced), so three attempts about 1 s and 2 s apart are safe; each
 * attempt gets its own request timeout from the client. `signal` only stops
 * further attempts — the dispatcher passes one nothing aborts, so a drain
 * cannot cut the final ack short.
 */
export async function ackWithRetry(
  client: CroniqClient,
  payload: AckRequest,
  signal: AbortSignal,
  logger: Logger,
): Promise<void> {
  const delays = ackRetry.delaysMs;
  for (let attempt = 0; ; attempt += 1) {
    try {
      await client.ack(payload, signal);
      return;
    } catch (err) {
      if (signal.aborted || attempt >= delays.length || !isRetryableAckError(err)) throw err;
      const delayMs = delays[attempt]!;
      logger.warn(`ack attempt ${attempt + 1} failed — retrying in ${delayMs}ms`, {
        error: String(err),
        execution_id: payload.execution_id,
      });
      await sleep(delayMs, signal).catch(() => {});
      if (signal.aborted) throw err;
    }
  }
}

/**
 * Upper bound on how long one ack can take including its retries: every
 * attempt runs into its request timeout and every pause is taken.
 */
export function ackBudgetMs(requestTimeoutMs: number): number {
  const pauses = ackRetry.delaysMs.reduce((sum, ms) => sum + ms, 0);
  return (ackRetry.delaysMs.length + 1) * requestTimeoutMs + pauses;
}
