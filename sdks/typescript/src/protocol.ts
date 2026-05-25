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
