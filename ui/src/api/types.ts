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
  tags: string[]
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
  /** Per-execution monotonic sequence number assigned at insert time;
   * resolves ties when many events share a millisecond. */
  seq: number
}

export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
}

/**
 * Response for `POST /v1/auth/login` when the user has TOTP enabled.
 * Distinct shape via the `requires_totp` flag; the discriminator lets
 * the UI pick the next screen without parsing JWT internals.
 */
export interface MfaRequiredResponse {
  requires_totp: true
  mfa_token: string
  mfa_token_expires_in: number
}

export type LoginResponse = TokenResponse | MfaRequiredResponse

export function isMfaRequired(r: LoginResponse): r is MfaRequiredResponse {
  return (r as MfaRequiredResponse).requires_totp === true
}

// ─── PR-A1+ Users & Multi-User auth ──────────────────────────────

export type Role = 'admin' | 'operator' | 'viewer'

export interface User {
  user_id: string
  username: string
  email: string | null
  display_name: string | null
  role: Role
  is_active: boolean
  created_at: string
  updated_at: string
  last_login_at: string | null
}

export interface Invitation {
  invitation_id: string
  email: string
  role: Role
  invited_by: string
  expires_at: string
  accepted_at: string | null
  revoked_at: string | null
  created_at: string
}

export interface CreateInvitationResponse {
  invitation_id: string
  email: string
  role: Role
  expires_at: string
  token: string
  accept_url: string
}

export interface PersonalAccessToken {
  token_id: string
  name: string
  token_prefix: string
  scopes: string[]
  expires_at: string | null
  revoked_at: string | null
  last_used_at: string | null
  created_at: string
}

export interface CreatePatResponse {
  token_id: string
  name: string
  token: string
  token_prefix: string
  scopes: string[]
  expires_at: string | null
}

// ─── PR-A3 TOTP ──────────────────────────────────────────────────

export interface TotpSetupResponse {
  secret: string
  otpauth_url: string
  recovery_codes: string[]
}

// ─── PR-A5 OIDC ──────────────────────────────────────────────────

export interface OidcConfigResponse {
  enabled: boolean
  provider_name: string | null
  login_url: string | null
}

// ─── PR-B1 Stats & Audit ─────────────────────────────────────────

export interface AuditEvent {
  event_id: string
  actor_type: string
  actor_id: string | null
  action: string
  target_type: string
  target_id: string | null
  diff_json: string | null
  created_at: string
}

export interface JobStatsResponse {
  job_key: string
  window_days: number
  total: number
  completed: number
  failed: number
  dead: number
  success_rate: number
  p50_ms: number | null
  p95_ms: number | null
  p99_ms: number | null
  last_failure_at: string | null
}

export interface ThroughputBucket {
  start: string
  ok: number
  err: number
}

export interface ThroughputResponse {
  window: string
  bucket: 'hour' | 'day'
  buckets: ThroughputBucket[]
}

export interface FailureHeatmap {
  days: number
  rows: number[][]
  hotspots: { hour: number; failures: number }[]
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
