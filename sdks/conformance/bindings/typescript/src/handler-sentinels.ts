import type { CroniqRunner, ExecutionContext, WorkEventLevel } from '@nuetzliches/croniq-runner';

import type { HandlerSpec } from './case-spec.js';

/**
 * Translates each handler `behavior` into a registered `JobHandler` on the
 * runner. Every conformance binding must implement the same five sentinels:
 * `noop`, `throw`, `sleep`, `log`, `stream_logs`.
 */
export function applyHandlers(runner: CroniqRunner, handlers: HandlerSpec[]): void {
  for (const spec of handlers) {
    const handler = (ctx: ExecutionContext): void | Promise<void> => behavior(spec, ctx);

    if (spec.is_default) {
      runner.setDefaultHandler(handler);
    } else if (spec.schedule) {
      runner.handle(spec.job_key, handler, { schedule: spec.schedule });
    } else {
      runner.handle(spec.job_key, handler);
    }
  }
}

async function behavior(spec: HandlerSpec, ctx: ExecutionContext): Promise<void> {
  switch (spec.behavior) {
    case 'noop':
      return;
    case 'throw':
      throw new Error(spec.error_message ?? 'thrown by conformance handler');
    case 'sleep': {
      const ms = spec.duration_ms ?? 0;
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => {
          ctx.signal.removeEventListener('abort', onAbort);
          resolve();
        }, ms);
        const onAbort = (): void => {
          clearTimeout(timer);
          // Reject with a real AbortError so the dispatcher classifies the
          // outcome as `cancelled by server` / `runner draining` (matches
          // the .NET binding's behaviour).
          const err = new Error('aborted');
          err.name = 'AbortError';
          reject(err);
        };
        if (ctx.signal.aborted) {
          clearTimeout(timer);
          const err = new Error('aborted');
          err.name = 'AbortError';
          reject(err);
        } else {
          ctx.signal.addEventListener('abort', onAbort, { once: true });
        }
      });
      return;
    }
    case 'log': {
      const level: WorkEventLevel = spec.level ?? 'info';
      const count = spec.count ?? 1;
      for (let i = 0; i < count; i++) {
        ctx.logger[level](spec.message ?? '');
      }
      return;
    }
    case 'stream_logs': {
      const count = spec.count ?? 1;
      const interval = spec.interval_ms ?? 0;
      const level: WorkEventLevel = spec.level ?? 'info';
      const writer = ctx.logWriter;
      for (let i = 0; i < count; i++) {
        await writer.write(level, `line ${i + 1}`);
        if (interval > 0 && i + 1 < count) {
          await new Promise((r) => setTimeout(r, interval));
        }
      }
      return;
    }
    default: {
      const exhaustive: never = spec.behavior;
      throw new Error(`unknown handler behavior '${String(exhaustive)}'`);
    }
  }
}
