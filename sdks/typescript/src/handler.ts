import type { ExecutionContext } from './context.js';

export type JobHandler = (ctx: ExecutionContext) => void | Promise<void>;

export interface JobRegistrationOptions {
  /**
   * If set, the runner calls `POST /v1/jobs/register` with this schedule
   * string at startup. Format follows the Croniqfile interval/cron syntax,
   * e.g. `"5m"`, `"1h"`, or a 5-field crontab expression.
   */
  schedule?: string;
  /** Server-side timeout to register with the schedule (e.g. `"10m"`). */
  timeout?: string;
  /** Human-readable description for the registered job. */
  description?: string;
}

export interface SelfRegisterEntry {
  jobKey: string;
  schedule: string;
  timeout: string | undefined;
  description: string | undefined;
}

/** Map of `job_key` → handler, plus an optional default handler. */
export class HandlerRegistry {
  readonly #handlers = new Map<string, JobHandler>();
  readonly #selfRegister: SelfRegisterEntry[] = [];
  #default: JobHandler | undefined;

  register(jobKey: string, handler: JobHandler, opts?: JobRegistrationOptions): void {
    if (!jobKey) throw new TypeError('jobKey is required');
    if (typeof handler !== 'function') throw new TypeError('handler must be a function');
    this.#handlers.set(jobKey, handler);
    if (opts?.schedule) {
      this.#selfRegister.push({
        jobKey,
        schedule: opts.schedule,
        timeout: opts.timeout,
        description: opts.description,
      });
    }
  }

  registerDefault(handler: JobHandler): void {
    if (typeof handler !== 'function') throw new TypeError('handler must be a function');
    this.#default = handler;
  }

  resolve(jobKey: string): JobHandler | undefined {
    return this.#handlers.get(jobKey) ?? this.#default;
  }

  get selfRegister(): readonly SelfRegisterEntry[] {
    return this.#selfRegister;
  }

  get hasDefault(): boolean {
    return this.#default !== undefined;
  }

  get keys(): readonly string[] {
    return [...this.#handlers.keys()];
  }
}

export class NoHandlerRegisteredError extends Error {
  constructor(public readonly jobKey: string) {
    super(`No handler registered for job '${jobKey}' and no default handler configured`);
    this.name = 'NoHandlerRegisteredError';
  }
}
