// In-memory representation of one trigger (producer) conformance YAML file
// (schema/trigger-case-schema.json). Field names mirror the YAML snake_case
// so the loader passes keys through verbatim. Shares `ScriptEntry` with the
// runner case shape — the mock-server contract is identical.

import type { ScriptEntry } from './case-spec.js';

export interface TriggerCaseSpec {
  name: string;
  description?: string;
  trigger_config: TriggerConfig;
  trigger_calls: TriggerCall[];
  server_script: ScriptEntry[];
  expectations: TriggerExpectations;
}

export interface TriggerConfig {
  api_key?: string;
  bearer_token?: string;
}

export interface TriggerCall {
  request: TriggerRequestSpec;
  expect: TriggerExpect;
}

export interface TriggerRequestSpec {
  job_key: string;
  require?: string[];
  prefer?: string[];
  metadata?: Record<string, unknown>;
  timeout?: string;
  idempotency_key?: string;
}

export interface TriggerExpect {
  /** Success — subset match on the parsed TriggerResult. */
  response?: TriggerResponseExpect;
  /** true → the client MUST surface the call as an error. */
  error?: boolean;
}

export interface TriggerResponseExpect {
  /** `"*"` matches any non-empty execution id. */
  execution_id?: string;
  queued?: number;
  deduplicated?: boolean;
}

export interface TriggerExpectations {
  duration_max_ms?: number;
  http: TriggerHttpExpectation[];
}

export interface TriggerHttpExpectation {
  method: string;
  path: string;
  exact_count?: number;
  min_count?: number;
  max_count?: number;
  headers?: Record<string, string>;
  body_match?: unknown;
  /** Top-level request-body keys that MUST NOT be present. */
  body_absent?: string[];
}
