// Shared HTTP plumbing for the runner client (client.ts) and the producer
// trigger client (trigger.ts). Kept dependency-free beyond `AbortError` so
// both clients compose an outer signal with a per-request timeout the same
// way — and abort classification stays identical across the two.

import { AbortError } from './deferred.js';

export interface ComposedSignal {
  signal: AbortSignal;
  /** Removes the listener we attached to `outer`. Must be called in a finally. */
  dispose: () => void;
}

/**
 * Combines an outer `AbortSignal` with an optional per-request timeout into a
 * single signal. When `timeoutMs` is omitted the outer signal is returned
 * as-is. Node 18 doesn't have `AbortSignal.any`, so this is built by hand; the
 * caller MUST invoke `dispose()` in a finally — otherwise long-lived outer
 * signals accumulate one listener per request.
 */
export function composeSignals(outer: AbortSignal | undefined, timeoutMs?: number): ComposedSignal {
  if (timeoutMs == null) {
    if (outer == null) return { signal: new AbortController().signal, dispose: () => {} };
    return { signal: outer, dispose: () => {} };
  }

  const ac = new AbortController();
  let timer: NodeJS.Timeout | undefined;

  if (outer == null) {
    timer = setTimeout(() => ac.abort(new AbortError('timeout')), timeoutMs);
    return { signal: ac.signal, dispose: () => timer && clearTimeout(timer) };
  }

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

/** True for a native fetch abort, including undici's wrapped-cause variant. */
export function isAbortLikeError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  if (err.name === 'AbortError') return true;
  // undici exposes the original cause for fetch aborts in some Node versions.
  const cause = (err as { cause?: unknown }).cause;
  return cause instanceof Error && cause.name === 'AbortError';
}
