// Public API surface for `@nuetzliches/croniq-runner`.

export { CroniqRunner, createRunner } from './runner.js';
export type {
  CroniqRunnerOptions,
  LogWriterOptions,
  ResolvedRunnerOptions,
  ResolvedLogWriterOptions,
} from './options.js';
export type { ExecutionContext } from './context.js';
export type { JobHandler, JobRegistrationOptions } from './handler.js';
export { NoHandlerRegisteredError } from './handler.js';
export type { Logger, LogLevel } from './logger.js';
export { consoleLogger, noopLogger } from './logger.js';
export type { LogWriter } from './log-writer.js';
export type {
  AckRequest,
  AckStatus,
  PollRequest,
  PollResponse,
  RegisterJobRequest,
  RegisterJobResponse,
  RenewRequest,
  TriggerRequest,
  TriggerResponse,
  WorkAssignment,
  WorkEvent,
  WorkEventLevel,
} from './protocol.js';
export { AuthFailedError, HttpError, PollInstanceConflictError, RunnerOwnershipDeniedError } from './client.js';
export { isLoopbackHostname } from './security.js';

// Producer-side trigger (on-demand) client — parity with the .NET SDK (#277).
export { CroniqTriggerClient, createTriggerClient, QueueOverflowError } from './trigger.js';
export type {
  CroniqTriggerClientOptions,
  TriggerParams,
  TriggerResult,
} from './trigger.js';
