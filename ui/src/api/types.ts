export interface HealthResponse {
  status: string
  runners_online: number
  runners_stale: number
  runners_dead: number
  queued: number
}

export interface VersionResponse {
  version: string
  git_sha: string
  build_time: string
  env: string
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

export type JobLifecycleStatus = 'active' | 'paused' | 'disabled' | 'exhausted'

/// How a job's executions are tracked. `ephemeral` jobs are fire-and-forget:
/// no execution rows are persisted, so an empty execution history is expected
/// rather than a fault (issue #263).
export type ExecutionMode = 'queued' | 'ephemeral'

/// Per-job scheduling liveness from `GET /v1/jobs/states` (issue #250).
/// `overdue` is `true` when an active trigger's next scheduled fire is in the
/// past — the scheduler never advanced it (a missed fire). Lets the dashboard
/// flag a stalled scheduler distinctly from a green success-rate.
export interface JobScheduleState {
  job_key: string
  status: JobLifecycleStatus
  next_fire_at: string | null
  last_fired_at: string | null
  fire_count: number
  overdue: boolean
  /// `queued` (persisted executions) or `ephemeral` (no execution history by
  /// design). Older servers omit this — treat `undefined` as `queued`.
  execution_mode?: ExecutionMode
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

/**
 * Response for `POST /v1/auth/login` when enforced 2FA is on but the account
 * has no confirmed TOTP secret. Instead of a lockout, the client drives inline
 * enrolment via `/v1/auth/login/enroll/totp/{begin,confirm}` with `enroll_token`.
 */
export interface EnrollmentRequiredResponse {
  enrollment_required: true
  enroll_token: string
  enroll_token_expires_in: number
}

export type LoginResponse =
  | TokenResponse
  | MfaRequiredResponse
  | EnrollmentRequiredResponse

export function isMfaRequired(r: LoginResponse): r is MfaRequiredResponse {
  return (r as MfaRequiredResponse).requires_totp === true
}

export function isEnrollmentRequired(
  r: LoginResponse,
): r is EnrollmentRequiredResponse {
  return (r as EnrollmentRequiredResponse).enrollment_required === true
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
  /** Only populated by `GET /v1/users/me`; absent on admin list/get. */
  totp_enabled?: boolean
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

// ─── #138 Sign-in method gates ───────────────────────────────────

export interface PasswordConfigResponse {
  enabled: boolean
}

export interface TotpConfigResponse {
  /** Server enforces 2FA for every password login. When true the login UI
   *  shows the code field up-front and completes login in a single request. */
  required: boolean
}

/// Returned by `GET /v1/auth/config` — combined OIDC + password gate
/// probe that the login UI hits before any auth happens.
export interface AuthConfigResponse {
  oidc: OidcConfigResponse
  password: PasswordConfigResponse
  totp: TotpConfigResponse
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

// ─── #140 Failure alerts ─────────────────────────────────────────

/// One of the channel kinds. Tagged JSON: `{ "type": "shell", "command": "…" }`.
/// `signing_key` is intentionally absent — the server never serialises
/// the HMAC secret. `unknown` is a forward-compat placeholder used when
/// the DSL referenced a channel kind that the running build doesn't
/// implement.
export type AlertChannelKind =
  | { type: 'shell'; command: string }
  | { type: 'webhook'; url: string; timeout_secs: number }
  | { type: 'unknown'; reason: string }

export interface AlertChannelConfig {
  name: string
  kind: AlertChannelKind
}

export type AlertRuleTrigger = 'job_failed' | 'job_sla_missed' | 'job_missed_fire'

export interface AlertRuleConfig {
  name: string
  trigger: AlertRuleTrigger
  job_key_glob: string
  min_attempts: number
  dead_letter_only: boolean
  throttle: string | null
  expected_within: string | null
  channels: string[]
}

/// An operational override layered on top of a DSL-defined alert rule
/// (issue #231). Exactly one intent per row — snooze, disable, or throttle;
/// they are not composable. A row whose `expires_at` is in the past is inert
/// (treated as absent) until the watchdog sweep removes it.
export interface AlertRuleOverride {
  rule_name: string
  /** `false` disables the rule; `null` means the override doesn't touch enablement. */
  enabled: boolean | null
  /** Rule is suppressed until this instant; doubles as the auto-clear deadline. */
  snooze_until: string | null
  /** Replaces the DSL throttle window (seconds) while active. */
  throttle_secs: number | null
  note: string
  set_by_user_id: string
  set_at: string
  /** When the override auto-clears; `null` for open-ended (disable/throttle only). */
  expires_at: string | null
}

/// Shape returned by `GET /v1/alerts/config`. The `channels` field is
/// an object keyed by channel name (server uses `HashMap<String, ...>`).
/// `overrides` surfaces active operational overrides inline (issue #231);
/// it is additive — older servers omit it, hence optional.
export interface AlertsConfig {
  channels: Record<string, AlertChannelConfig>
  rules: AlertRuleConfig[]
  overrides?: AlertRuleOverride[]
}

export type AlertDeliveryState = 'delivered' | 'failed' | 'throttled'

/// One row from `alert_deliveries`. Returned by `GET /v1/alerts/deliveries`
/// (list) and `GET /v1/alerts/deliveries/{id}` (single).
export interface AlertDelivery {
  delivery_id: string
  rule_name: string
  channel_name: string
  job_key: string
  execution_id: string | null
  state: AlertDeliveryState
  error: string | null
  fired_at: string
  delivered_at: string | null
}

export interface AlertDeliveryListQuery {
  job_key?: string
  rule_name?: string
  state?: AlertDeliveryState
  since?: string
  limit?: number
}
