// In-memory representation of one conformance YAML file. Field names
// mirror the YAML/openapi.yaml snake_case convention so the loader can
// pass keys through verbatim.

export interface CaseSpec {
  name: string;
  description?: string;
  runner_config: RunnerConfig;
  handlers: HandlerSpec[];
  server_script: ScriptEntry[];
  shutdown_after_ms?: number;
  expectations: Expectations;
}

export interface RunnerConfig {
  runner_id?: string;
  runner_id_prefix?: string;
  capabilities?: string[];
  tags?: string[];
  max_inflight?: number;
  api_key?: string;
  bearer_token?: string;
  poll_timeout_ms?: number;
  renew_interval_ms?: number;
  drain_timeout_ms?: number;
  poll_retry_delay_ms?: number;
  capacity_backoff_ms?: number;
  max_consecutive_poll_conflicts?: number;
}

export type HandlerBehavior = 'noop' | 'throw' | 'sleep' | 'log' | 'stream_logs';

export interface HandlerSpec {
  job_key: string;
  is_default?: boolean;
  schedule?: string;
  behavior: HandlerBehavior;
  error_message?: string;
  duration_ms?: number;
  level?: 'trace' | 'debug' | 'info' | 'warn' | 'error';
  message?: string;
  count?: number;
  interval_ms?: number;
}

export interface ScriptEntry {
  on: string;
  match_count?: number;
  respond: RespondSpec;
}

export interface RespondSpec {
  status: number;
  body?: unknown;
  delay_ms?: number;
  headers?: Record<string, string>;
}

export interface Expectations {
  duration_max_ms?: number;
  http: HttpExpectation[];
}

export interface HttpExpectation {
  method: string;
  path: string;
  exact_count?: number;
  min_count?: number;
  max_count?: number;
  headers?: Record<string, string>;
  body_match?: unknown;
}
