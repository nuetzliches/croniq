export interface HealthResponse {
  status: string
  runners_online: number
  runners_stale: number
  runners_dead: number
  queued: number
}

export interface JobDefinition {
  job_key: string
  description: string | null
  assigned_runner_id: string | null
  is_active: boolean
  metadata: Record<string, string>
  created_at: string
  updated_at: string
  timeout: string | null
  max_retries: number | null
  dead_letter_enabled: boolean | null
  tags: string[]
}

export interface TagCount {
  tag: string
  count: number
}

export interface TriggerDefinition {
  trigger_id: string
  job_key: string
  cron_expression: string | null
  timezone: string | null
  calendar: string | null
  window: string | null
  enabled: boolean
  managed_by: string
  created_at: string
  updated_at: string
}

export interface RunnerSummary {
  runner_id: string
  status: string
  capabilities: string[]
  max_inflight: number
  inflight: number
  last_poll_at: string
}

export interface Execution {
  id: string
  job_key: string
  fire_at: string
  attempt: number
  state: string
  runner_id: string | null
  claimed_at: string | null
  completed_at: string | null
  duration_ms: number | null
  error: string | null
  created_at: string
}

export interface DeadLetter {
  id: string
  execution_id: string
  job_key: string
  fire_at: string
  attempt: number
  error: string
  dead_reason: string
  metadata: Record<string, string>
  created_at: string
  expires_at: string | null
}

export interface ExecutionLogEntry {
  id: string
  execution_id: string
  timestamp: string
  level: string
  message: string
  fields: Record<string, string>
}

export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
}

export interface ForecastBucket {
  start: string
  end: string
  count: number
  jobs: string[]
}

export interface ForecastResponse {
  window_minutes: number
  bucket_minutes: number
  buckets: ForecastBucket[]
}

export interface CalendarDefinition {
  calendar_id: string
  name: string
  timezone: string | null
  rules: string
  /** "dsl" — synthesized from the Croniqfile (read-only via API).
   *  "api" — created via API/UI, fully editable. */
  managed_by: 'dsl' | 'api'
  created_at: string
  updated_at: string
}

export interface ApiClient {
  client_id: string
  name: string
  scopes: string[]
  is_active: boolean
  created_at: string
}

export interface CreateClientResponse {
  client_id: string
  name: string
}

export interface CreateApiKeyResponse {
  raw_key: string
  key_id: string
  client_id: string
}

export interface TriggerResponse {
  execution_id: string
  queued: boolean
}

export interface ReplayResponse {
  execution_id: string
  attempt: number
}
