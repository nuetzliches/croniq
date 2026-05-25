import type { WorkEventLevel } from './protocol.js';

export type LogLevel = WorkEventLevel;

export interface Logger {
  trace(message: string, fields?: Record<string, unknown>): void;
  debug(message: string, fields?: Record<string, unknown>): void;
  info(message: string, fields?: Record<string, unknown>): void;
  warn(message: string, fields?: Record<string, unknown>): void;
  error(message: string, fields?: Record<string, unknown>): void;
}

const ORDER: Record<LogLevel, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

/**
 * Console-backed logger that emits at or above {@link minLevel}.
 *
 * The runner's internal diagnostics default to `warn` so SDK noise stays
 * out of an application's stdout/stderr unless something is going wrong.
 * Job handlers receive a separate per-execution `Logger` whose minLevel
 * defaults to `info`.
 */
export function consoleLogger(minLevel: LogLevel = 'warn', prefix?: string): Logger {
  const threshold = ORDER[minLevel];
  const tag = prefix ? `[${prefix}] ` : '';

  const emit = (level: LogLevel, message: string, fields?: Record<string, unknown>): void => {
    if (ORDER[level] < threshold) return;
    const line = fields && Object.keys(fields).length > 0
      ? `${tag}${level}: ${message} ${JSON.stringify(fields)}`
      : `${tag}${level}: ${message}`;
    // Map to console method, but funnel everything to stderr for warn/error
    // so application stdout stays clean for actual job output.
    // eslint-disable-next-line no-console
    if (level === 'error' || level === 'warn') console.error(line);
    // eslint-disable-next-line no-console
    else console.log(line);
  };

  return {
    trace: (m, f) => emit('trace', m, f),
    debug: (m, f) => emit('debug', m, f),
    info: (m, f) => emit('info', m, f),
    warn: (m, f) => emit('warn', m, f),
    error: (m, f) => emit('error', m, f),
  };
}

/** Logger that drops every message — useful for tests. */
export const noopLogger: Logger = {
  trace: () => {},
  debug: () => {},
  info: () => {},
  warn: () => {},
  error: () => {},
};

/**
 * Wraps a logger so every emitted record carries the given extra fields,
 * merged with whatever the caller passes in (caller wins). Used to scope a
 * handler-side logger with `execution_id`, `job_key`, `runner_id`, `attempt`.
 */
export function scopedLogger(base: Logger, extra: Record<string, unknown>): Logger {
  const merge = (fields?: Record<string, unknown>): Record<string, unknown> => ({ ...extra, ...(fields ?? {}) });
  return {
    trace: (m, f) => base.trace(m, merge(f)),
    debug: (m, f) => base.debug(m, merge(f)),
    info: (m, f) => base.info(m, merge(f)),
    warn: (m, f) => base.warn(m, merge(f)),
    error: (m, f) => base.error(m, merge(f)),
  };
}
