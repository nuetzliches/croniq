// Wire-protocol types for the Croniq runner API.
//
// Field names are snake_case to match the JSON shape in `openapi.yaml`.
// These types are sent and received verbatim over HTTP — no field
// transformation happens at the client boundary, so consumer code that
// touches `metadata` will see the original snake_case keys the server sent.

export interface PollRequest {
  runner_id: string;
  capabilities: string[];
  max_inflight: number;
  inflight: string[];
  instance_id?: string;
  tags: string[];
}

export interface WorkAssignment {
  execution_id: string;
  job_key: string;
  fire_at: string;
  /**
   * Original logical fire time (RFC 3339). Absent when the server predates
   * the field — consumers must not fall back to `fire_at`.
   */
  scheduled_for?: string;
  attempt: number;
  metadata: unknown;
  timeout?: string;
}

export interface PollResponse {
  work: WorkAssignment[];
  cancel: string[];
}

export type AckStatus = 'success' | 'failure';

export interface AckRequest {
  runner_id: string;
  execution_id: string;
  status: AckStatus;
  error?: string;
  duration_ms?: number;
  attempt: number;
}

export interface RenewRequest {
  runner_id: string;
  execution_id: string;
}

export interface RegisterJobRequest {
  job_key: string;
  schedule: string;
  timezone?: string;
  timeout?: string;
  runner_id?: string;
  capabilities: string[];
  description?: string;
}

export interface RegisterJobResponse {
  job_key: string;
  trigger_id?: string;
  status?: 'registered' | 'skipped_dsl_precedence';
}

export type WorkEventLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';

export interface WorkEvent {
  level?: WorkEventLevel;
  message: string;
  fields?: Record<string, string>;
}

/**
 * Wire request for `POST /v1/trigger` (producer side). Optional fields are
 * omitted from the JSON body when unset — a producer never emits a key the
 * caller didn't supply (`JSON.stringify` drops `undefined` values).
 */
export interface TriggerRequest {
  job_key: string;
  metadata?: Record<string, unknown>;
  require?: string[];
  prefer?: string[];
  timeout?: string;
  idempotency_key?: string;
}

/**
 * Wire response of `POST /v1/trigger`. `deduplicated` is sent by servers that
 * support trigger idempotency keys (#279); older servers omit it and the
 * client defaults it to `false`.
 */
export interface TriggerResponse {
  execution_id: string;
  queued: number;
  deduplicated?: boolean;
}
