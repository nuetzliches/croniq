/**
 * Compose multiple AbortSignals into one that aborts as soon as any of the
 * inputs aborts. A polyfill for `AbortSignal.any()` (Node 20.3+) so this
 * SDK works on Node 18.
 */
export function anySignal(...signals: AbortSignal[]): AbortSignal {
  // Fast path: native AbortSignal.any when available.
  const anyFn = (AbortSignal as unknown as { any?: (s: AbortSignal[]) => AbortSignal }).any;
  if (typeof anyFn === 'function') return anyFn(signals);

  const controller = new AbortController();
  const abort = (reason: unknown): void => {
    if (controller.signal.aborted) return;
    controller.abort(reason);
    for (const s of signals) s.removeEventListener('abort', listeners.get(s)!);
  };

  const listeners = new Map<AbortSignal, () => void>();
  for (const signal of signals) {
    if (signal.aborted) {
      controller.abort(signal.reason);
      return controller.signal;
    }
    const fn = (): void => abort(signal.reason);
    listeners.set(signal, fn);
    signal.addEventListener('abort', fn, { once: true });
  }
  return controller.signal;
}
