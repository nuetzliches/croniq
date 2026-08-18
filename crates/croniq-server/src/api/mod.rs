//! Extended HTTP API: runner Pull-API, auth, and management endpoints.

pub mod admin;
pub mod alerts;
pub mod audit;
pub mod auth_endpoints;
pub mod auth_middleware;
pub mod calendars;
pub mod dashboard;
pub mod dead_letters;
pub mod events_sse;
pub mod execution_logs;
pub mod executions;
pub mod hardening;
pub mod invitations;
pub mod jobs;
pub mod maintenance;
pub mod oidc;
pub mod password_reset;
pub mod pat;
pub mod runner_identity;
pub mod runners_sse;
pub mod schedules;
pub mod stats;
pub mod system;
pub mod tags;
pub mod totp;
pub mod users;
pub mod work;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::api::auth_middleware::require_scope;
use axum::{
    Extension, Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header::RETRY_AFTER},
    middleware,
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_auth::jwt::JwtConfig;
use croniq_runner::{
    AppState, CompleteResponse, RegisterOutcome, RunnerStatus, RunnerSummary, TriggerRequest,
    TriggerResponse, WorkAssignment, WorkItem,
    types::{CompleteRequest, HealthResponse, PollRequest, PollResponse},
};
use croniq_scheduler::trigger::Trigger;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::completion::CompletionEvent;
use crate::email::EmailSender;
use crate::oidc::SharedOidcProvider;
use crate::reload::ReloadCounters;
use crate::scheduler::SchedulerCommand;
use crate::store::DynStore;
use crate::watchdog::WatchdogCounters;
use croniq_config::compile::{CalendarConfig, JobConfig};
use croniq_store::models::{Execution, ExecutionFilter, ExecutionState, MaintenanceState};

/// Default maximum time a poll request will block waiting for work.
const DEFAULT_LONG_POLL_TIMEOUT: Duration = Duration::from_secs(30);

/// Default dedup window for `POST /v1/trigger` idempotency keys (issue
/// #279): 10 minutes. Overridable via the Croniqfile
/// `pull_api { trigger_dedup_window … }` directive.
pub const DEFAULT_TRIGGER_DEDUP_WINDOW_SECS: u64 = 600;

/// Maximum accepted length (in characters) of a trigger `idempotency_key`.
/// Longer keys are rejected with `400 Bad Request`.
const MAX_IDEMPOTENCY_KEY_CHARS: usize = 200;

/// Backpressure hint (seconds) sent as the `Retry-After` header when a
/// `POST /v1/trigger` is rejected with `429` for per-job queue overflow
/// (#299/#312). A short, fixed floor: the per-job queue drains as runners
/// claim work, so a producer that waits this long before retrying gives the
/// queue room without stalling throughput. The SDKs surface it as
/// `retryAfterMs`.
const TRIGGER_OVERFLOW_RETRY_AFTER_SECS: u64 = 1;

// ─── Server state ─────────────────────────────────────────────────────────────

/// Full server state: runner sub-state + completion channel.
pub struct ServerState {
    /// Shared runner state (registry + queue).
    pub runner: Arc<AppState>,
    /// Channel for forwarding completion events to the processor task.
    pub completion_tx: mpsc::UnboundedSender<CompletionEvent>,
    /// How long a poll request may block waiting for work.
    /// Defaults to 30 s; can be reduced in tests.
    pub long_poll_timeout: Duration,
    /// JWT configuration for token-based auth. None = auth disabled.
    pub jwt_config: Option<JwtConfig>,
    /// Persistent store for querying jobs and executions.
    pub store: Option<DynStore>,
    /// Channel to send commands to the live scheduler (add/remove jobs).
    pub scheduler_tx: Option<mpsc::UnboundedSender<SchedulerCommand>>,
    /// Shared trigger map for dashboard forecast (read-only snapshot).
    pub triggers: Option<Arc<tokio::sync::RwLock<HashMap<String, Trigger>>>>,
    /// DSL-defined jobs (from the Croniqfile). Shared with the scheduler task,
    /// which replaces its contents on Croniqfile hot-reload. The REST API
    /// unions this with the persisted store so DSL jobs appear in `/v1/jobs`
    /// and `/v1/schedules` alongside API/runner-registered ones.
    pub dsl_jobs: Option<Arc<tokio::sync::RwLock<Vec<JobConfig>>>>,
    /// DSL-defined calendars (from the Croniqfile). Same hot-reload semantics
    /// as `dsl_jobs`. The REST API synthesizes them with `managed_by="dsl"`
    /// in `/v1/calendars` so the UI can reference them in schedule editors.
    pub dsl_calendars: Option<Arc<tokio::sync::RwLock<Vec<CalendarConfig>>>>,
    /// Server-wide policy flag from the Croniqfile `policy { dsl_adopt_on_mutate ... }`
    /// block. When `true`, the explicit `/adopt` endpoint copies a DSL
    /// resource into the API store; when `false` (default), `/adopt`
    /// returns 409 and PUT/DELETE on DSL resources stay blocked.
    pub policy_dsl_adopt_on_mutate: Arc<std::sync::atomic::AtomicBool>,
    /// `policy { strict_calendars }` from the Croniqfile (default `true`).
    /// API handlers resolve schedule calendar references with this policy:
    /// under strict, an unresolvable reference fails closed (trigger paused +
    /// `config_faults` entry) instead of running un-gated (issues #361/#393).
    /// Set at boot and updated on every hot-reload.
    pub policy_strict_calendars: Arc<std::sync::atomic::AtomicBool>,
    /// Path to the Croniqfile, needed by the admin reload endpoint.
    pub config_path: Option<std::path::PathBuf>,
    /// The boot-only settings this process actually started with (issue #406).
    /// `None` on servers that never loaded a Croniqfile (tests, storeless
    /// setups) — the reload endpoint then skips the pending-restart check
    /// instead of reporting every setting as changed.
    pub boot_only_settings: Option<crate::reload::BootOnlySettings>,
    /// Counters for `croniq_config_reload_total`, incremented by both the
    /// file-watcher reload path and the admin reload endpoint.
    pub reload_counters: Arc<ReloadCounters>,
    /// Cumulative counters for the watchdog's recovery actions
    /// (`croniq_watchdog_*` metrics), incremented by the watchdog sweep task
    /// in main.rs and by the inline-takeover requeue in the poll handler.
    pub watchdog_counters: Arc<WatchdogCounters>,
    /// Outbound email sender — used for invitations and password resets.
    /// Defaults to `NoopSender` (logs but doesn't deliver); SMTP backend
    /// lands in PR-A6 behind the `smtp` cargo feature.
    pub email_sender: Arc<dyn EmailSender>,
    /// Operator-configured public base URL for invite, password-reset, and
    /// OIDC login links, from `CRONIQ_APP_URL`. `None` when unset — the base
    /// URL is then derived per-request from `X-Forwarded-*` / `Host` headers
    /// (see `resolve_link_base`). An explicit value is authoritative and
    /// immune to `Host`-header spoofing, which is why it stays configurable.
    pub app_base_url: Option<String>,
    /// OIDC provider for SSO login. `None` disables the OIDC routes.
    /// Discovered once at startup (see `oidc::OidcProvider::discover`).
    pub oidc: SharedOidcProvider,
    /// Whether password login is enabled (issue #138).
    ///
    /// Resolved at boot from DSL `auth { password { enabled bool } }` + env
    /// `CRONIQ_PASSWORD_LOGIN_ENABLED`; defaults to `true`. When `false`,
    /// the public password-login + password-reset endpoints return
    /// `403 password login disabled` and the UI hides the password form.
    pub password_login_enabled: bool,
    /// Whether every password login must present a valid TOTP/recovery
    /// code (enforced 2FA). Resolved at boot from DSL
    /// `auth { totp { required bool } }` + env `CRONIQ_REQUIRE_TOTP`;
    /// defaults to `false`. When `true`, the login UI shows the code field
    /// up-front, and an account without a confirmed TOTP secret is *not*
    /// refused: login answers `enrollment_required` with a short-lived enrol
    /// token so the user can set up TOTP inline (issue #409).
    pub require_totp: bool,
    /// Whether the running config caps run history — `execution_retention` or
    /// a `keep_last` (issue #405). Snapshotted at boot because both knobs are
    /// boot-only (a reload parses them but cannot apply them), so this reflects
    /// what the watchdog actually prunes, not what the file says right now.
    pub retention_configured: bool,
    /// Effective failure-alert configuration after
    /// `alerts::merge_legacy_env_hook` (issue #140 PR-5). Backs the
    /// read-only `GET /v1/alerts/config` endpoint. The
    /// `ChannelKind::Webhook::signing_key` field is
    /// `#[serde(skip_serializing)]` so the HMAC secret cannot leak
    /// via this endpoint — verified by an integration test.
    pub alerts: croniq_config::compile::AlertsConfig,
    /// In-memory fan-out for the Live Console (issue #141). `None` in
    /// tests and unconfigured servers — the SSE endpoint returns 503
    /// in that case. Populated by `main.rs` from the telemetry init's
    /// returned hub.
    pub console_hub: Option<Arc<crate::live_console::ConsoleHub>>,
    /// Scheduler liveness signal (issue #248). The scheduler task records a
    /// timestamp after each successful tick; the `/metrics` endpoint exposes
    /// it as `croniq_scheduler_last_tick_timestamp` so external monitoring can
    /// alert on a wedged scheduler even while HTTP keeps serving. `None` in
    /// tests / when no scheduler runs.
    pub scheduler_heartbeat: Option<Arc<crate::scheduler::SchedulerHeartbeat>>,
    /// Dedup window in seconds for `POST /v1/trigger` idempotency keys
    /// (issue #279). A repeat trigger with the same `(job_key,
    /// idempotency_key)` coalesces to the existing execution while that
    /// execution is still in-flight OR was created within this window.
    /// Resolved at boot from the Croniqfile `pull_api {
    /// trigger_dedup_window … }` directive; default 10 minutes.
    pub trigger_dedup_window_secs: u64,
    /// Global maintenance switch. Cached in-memory (the store is the source of
    /// truth) so the scheduler tick and the work-poll can check it cheaply
    /// every cycle; the PUT handler updates both the store and this cache.
    pub maintenance: Arc<std::sync::RwLock<MaintenanceState>>,
    /// Jobs paused at load time because their `calendar` reference did not
    /// resolve (issue #361), keyed by job key with a human-readable reason.
    /// Populated on boot and on every hot-reload from
    /// `loader::LoadedConfig::calendar_faults`; surfaced as `config_error` on
    /// `GET /v1/jobs/states` and counted by `croniq_config_calendar_faults`.
    /// Empty under `policy { strict_calendars false }`.
    pub config_faults: Arc<std::sync::RwLock<HashMap<String, String>>>,
    /// Whether a work-protocol `runner_id` is bound to the credential that
    /// first claimed it, so one runner's credential cannot act as another
    /// runner. `pull_api { runner_identity_binding "off" }` clears it. See
    /// [`runner_identity`] for the binding semantics; note that binding is
    /// additionally inert without auth or without a store, since neither
    /// case can tell callers apart or record a decision.
    pub runner_identity_binding: bool,
}

impl ServerState {
    pub fn new(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            completion_tx,
            long_poll_timeout: DEFAULT_LONG_POLL_TIMEOUT,
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        })
    }

    /// Construct with JWT auth and optional store.
    pub fn with_auth(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
        jwt_config: Option<JwtConfig>,
        store: Option<DynStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            completion_tx,
            long_poll_timeout: DEFAULT_LONG_POLL_TIMEOUT,
            jwt_config,
            store,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        })
    }

    /// Construct with a custom long-poll timeout (useful in tests).
    pub fn with_timeout(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
        long_poll_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            completion_tx,
            long_poll_timeout,
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        })
    }

    /// Compile the effective calendar set (DSL ∪ store, DSL wins) for
    /// attaching gates to API-managed triggers (issue #393). A missing store
    /// only degrades to the DSL-only set — the triggers being built here are
    /// store-backed anyway, so that combination can't lose a stored calendar.
    pub async fn resolved_calendars(&self) -> crate::loader::ResolvedCalendars {
        let dsl: Vec<CalendarConfig> = match self.dsl_calendars.as_ref() {
            Some(shared) => shared.read().await.clone(),
            None => Vec::new(),
        };
        let stored = match self.store.as_ref() {
            Some(store) => store.list_calendars().unwrap_or_else(|e| {
                tracing::warn!(error = %e, "could not list stored calendars — resolving against DSL only");
                Vec::new()
            }),
            None => Vec::new(),
        };
        let strict = self
            .policy_strict_calendars
            .load(std::sync::atomic::Ordering::Relaxed);
        crate::loader::resolve_calendars(&dsl, &stored, strict)
    }

    /// Insert (`Some`) or clear (`None`) the `config_error` fault for a job.
    /// Clearing on successful calendar resolution is what heals a previously
    /// faulted API schedule without waiting for a config reload.
    pub fn set_config_fault(&self, job_key: &str, fault: Option<String>) {
        let mut faults = self.config_faults.write().unwrap();
        match fault {
            Some(reason) => {
                faults.insert(job_key.to_string(), reason);
            }
            None => {
                faults.remove(job_key);
            }
        }
    }
}

/// Resolve the public origin (`scheme://host[:port]`, no trailing slash) used
/// to build user-facing links (invite, password-reset, OIDC login).
///
/// Precedence:
/// 1. `configured` — the operator's `CRONIQ_APP_URL`. Authoritative, returned
///    verbatim. Immune to `Host`-header spoofing.
/// 2. Reverse-proxy headers `X-Forwarded-Proto` + `X-Forwarded-Host`.
/// 3. The raw `Host` header — only when `trust_request_host` is `true`.
/// 4. `http://localhost:4000` as a last resort.
///
/// `trust_request_host` MUST be `false` for links generated on public,
/// unauthenticated endpoints (password-reset): there the `Host` header is
/// attacker-controlled, so trusting it would let an attacker poison the
/// emailed reset link and capture the token (reset poisoning). Authenticated
/// or same-origin callers (invite creation, OIDC config) pass `true`.
pub(crate) fn resolve_link_base(
    configured: &Option<String>,
    headers: &HeaderMap,
    trust_request_host: bool,
) -> String {
    if let Some(url) = configured {
        return url.trim_end_matches('/').to_string();
    }
    // Headers may carry a comma-separated list when several proxies append;
    // the first value is the outermost (client-facing) hop.
    let header_first = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|raw| raw.split(',').next().unwrap_or(raw).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let proto = header_first("x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    let host = header_first("x-forwarded-host").or_else(|| {
        if trust_request_host {
            header_first("host")
        } else {
            None
        }
    });
    match host {
        Some(h) => format!("{proto}://{h}"),
        None => "http://localhost:4000".to_string(),
    }
}

#[cfg(test)]
mod link_base_tests {
    use super::resolve_link_base;
    use axum::http::{HeaderMap, HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn configured_value_wins_and_trims_slash() {
        let base = resolve_link_base(
            &Some("https://app.example.com/".to_string()),
            &headers(&[("x-forwarded-host", "evil.example")]),
            true,
        );
        assert_eq!(base, "https://app.example.com");
    }

    #[test]
    fn derives_from_forwarded_headers() {
        let base = resolve_link_base(
            &None,
            &headers(&[
                ("x-forwarded-proto", "https"),
                ("x-forwarded-host", "cron.nuts.internal"),
            ]),
            false,
        );
        assert_eq!(base, "https://cron.nuts.internal");
    }

    #[test]
    fn uses_host_header_when_trusted() {
        let base = resolve_link_base(&None, &headers(&[("host", "cron.nuts.internal")]), true);
        assert_eq!(base, "http://cron.nuts.internal");
    }

    #[test]
    fn ignores_raw_host_when_untrusted() {
        // The password-reset case: a spoofed Host must NOT poison the link.
        let base = resolve_link_base(&None, &headers(&[("host", "attacker.example")]), false);
        assert_eq!(base, "http://localhost:4000");
    }

    #[test]
    fn forwarded_host_trusted_even_when_raw_host_is_not() {
        // Behind a reverse proxy the reset link still resolves correctly.
        let base = resolve_link_base(
            &None,
            &headers(&[
                ("x-forwarded-host", "cron.nuts.internal"),
                ("host", "internal:4000"),
            ]),
            false,
        );
        assert_eq!(base, "http://cron.nuts.internal");
    }

    #[test]
    fn falls_back_to_localhost_without_headers() {
        let base = resolve_link_base(&None, &HeaderMap::new(), true);
        assert_eq!(base, "http://localhost:4000");
    }
}

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn server_router(state: Arc<ServerState>) -> Router {
    // Authenticated routes
    let authenticated = Router::new()
        // Work protocol
        .route("/v1/poll", post(handle_poll)) // legacy compat
        .route("/v1/complete", post(handle_complete)) // legacy compat
        .route("/v1/work/poll", post(handle_poll))
        .route("/v1/work/ack", post(handle_complete))
        .route("/v1/work/renew", post(work::handle_renew))
        .route("/v1/work/{execution_id}/events", post(work::handle_events))
        // Runners
        .route("/v1/runners", get(handle_list_runners))
        .route("/v1/runners/{id}", delete(handle_delete_runner))
        .route("/v1/runners/stream", get(runners_sse::handle_runner_stream))
        .route("/v1/events/stream", get(events_sse::handle_events_stream))
        .route("/v1/trigger", post(handle_trigger))
        // Jobs CRUD
        .route("/v1/jobs", get(jobs::handle_list).post(jobs::handle_create))
        .route(
            "/v1/jobs/{job_key}",
            get(jobs::handle_get)
                .put(jobs::handle_update)
                .delete(jobs::handle_delete),
        )
        .route("/v1/jobs/{job_key}/activate", post(jobs::handle_activate))
        .route(
            "/v1/jobs/{job_key}/deactivate",
            post(jobs::handle_deactivate),
        )
        .route("/v1/jobs/register", post(jobs::handle_register))
        // Per-job scheduling liveness (issue #250). Static sibling of
        // `/v1/jobs/{job_key}`; matchit routes the static segment first.
        .route("/v1/jobs/states", get(jobs::handle_list_states))
        // Schedules CRUD
        .route(
            "/v1/schedules",
            get(schedules::handle_list).post(schedules::handle_create),
        )
        .route(
            "/v1/schedules/{trigger_id}",
            get(schedules::handle_get)
                .put(schedules::handle_update)
                .delete(schedules::handle_delete),
        )
        // Calendars CRUD
        .route(
            "/v1/calendars",
            get(calendars::handle_list).post(calendars::handle_create),
        )
        .route(
            "/v1/calendars/{id}",
            get(calendars::handle_get)
                .put(calendars::handle_update)
                .delete(calendars::handle_delete),
        )
        // Calendar adoption (Phase 2 — opt-in via Croniqfile policy block).
        .route("/v1/calendars/{id}/adopt", post(calendars::handle_adopt))
        .route(
            "/v1/calendars/{id}/unadopt",
            post(calendars::handle_unadopt),
        )
        // Job adoption (Phase 2.5 — same opt-in policy applies).
        .route("/v1/jobs/{job_key}/adopt", post(jobs::handle_adopt))
        .route("/v1/jobs/{job_key}/unadopt", post(jobs::handle_unadopt))
        // Dead letters
        .route("/v1/dead-letters", get(dead_letters::handle_list))
        // Static segment — matchit routes it ahead of the `{id}` param below.
        .route(
            "/v1/dead-letters/bulk-delete",
            post(dead_letters::handle_bulk_delete),
        )
        .route(
            "/v1/dead-letters/{id}",
            get(dead_letters::handle_get).delete(dead_letters::handle_delete),
        )
        .route(
            "/v1/dead-letters/{id}/replay",
            post(dead_letters::handle_replay),
        )
        // Failure alerts (issue #140 PR-5): read-only view of the
        // effective config + the per-fire delivery log. Rules + channels
        // are DSL-managed; `/config` surfaces operational overrides inline.
        .route("/v1/alerts/config", get(alerts::handle_get_config))
        .route("/v1/alerts/deliveries", get(alerts::handle_list_deliveries))
        .route(
            "/v1/alerts/deliveries/{id}",
            get(alerts::handle_get_delivery),
        )
        // Operational overrides (issue #231): temporary, audit-logged
        // runtime-state tweaks on DSL rules. `alerts:write` (admin) for
        // mutations; the read view is `alerts:read`.
        .route(
            "/v1/alerts/rules/{name}/snooze",
            post(alerts::handle_snooze_rule),
        )
        .route(
            "/v1/alerts/rules/{name}/disable",
            post(alerts::handle_disable_rule),
        )
        .route(
            "/v1/alerts/rules/{name}/throttle",
            post(alerts::handle_throttle_rule),
        )
        .route(
            "/v1/alerts/rules/{name}/override",
            get(alerts::handle_get_override).delete(alerts::handle_clear_override),
        )
        // Dashboard
        .route("/v1/dashboard/forecast", get(dashboard::handle_forecast))
        // Job stats + insights + audit (PR-B1)
        .route("/v1/jobs/{job_key}/stats", get(stats::handle_job_stats))
        .route("/v1/executions/throughput", get(stats::handle_throughput))
        .route("/v1/insights/failures", get(stats::handle_failure_heatmap))
        .route("/v1/audit", get(audit::handle_list))
        // Tags
        .route("/v1/tags", get(tags::handle_list_tags))
        // Executions + logs
        .route("/v1/executions", get(handle_list_executions))
        .route(
            "/v1/executions/{id}/cancel",
            post(executions::handle_cancel),
        )
        .route(
            "/v1/executions/{id}/logs",
            get(execution_logs::handle_get_logs),
        )
        // Admin
        .route("/v1/admin/reload-config", post(admin::handle_reload_config))
        .route(
            "/v1/maintenance",
            get(maintenance::handle_get_maintenance).put(maintenance::handle_set_maintenance),
        )
        // System diagnostics (config health) — admin-only
        .route("/v1/system/diagnostics", get(system::handle_diagnostics))
        // Auth management
        .route(
            "/v1/api-clients",
            get(auth_endpoints::handle_list_clients).post(auth_endpoints::handle_create_client),
        )
        .route(
            "/v1/api-clients/{id}",
            put(auth_endpoints::handle_update_client).delete(auth_endpoints::handle_delete_client),
        )
        .route(
            "/v1/api-clients/{id}/tokens",
            post(auth_endpoints::handle_issue_client_token),
        )
        .route("/v1/api-keys", post(auth_endpoints::handle_create_api_key))
        .route(
            "/v1/api-keys/{id}",
            delete(auth_endpoints::handle_revoke_api_key),
        )
        // Users
        .route(
            "/v1/users",
            get(users::handle_list).post(users::handle_create),
        )
        .route(
            "/v1/users/me",
            get(users::handle_get_me).patch(users::handle_update_me),
        )
        .route(
            "/v1/users/me/change-password",
            post(users::handle_change_password),
        )
        .route(
            "/v1/users/{id}",
            get(users::handle_get)
                .patch(users::handle_update)
                .delete(users::handle_delete),
        )
        // Invitations
        .route(
            "/v1/invitations",
            get(invitations::handle_list).post(invitations::handle_create),
        )
        .route("/v1/invitations/{id}", delete(invitations::handle_revoke))
        // TOTP / 2FA (self-service)
        .route("/v1/users/me/totp/setup", post(totp::handle_setup))
        .route("/v1/users/me/totp/confirm", post(totp::handle_confirm))
        .route("/v1/users/me/totp/disable", post(totp::handle_disable))
        .route(
            "/v1/users/me/totp/recovery-codes/regenerate",
            post(totp::handle_regenerate),
        )
        // Personal Access Tokens (self-service)
        .route(
            "/v1/users/me/tokens",
            get(pat::handle_list).post(pat::handle_create),
        )
        .route("/v1/users/me/tokens/{id}", delete(pat::handle_revoke))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware::require_auth,
        ));

    // Public routes — no auth required.
    //
    // Health + version metadata (anonymous service discovery), auth
    // login / refresh / logout, password-reset request/confirm, invite
    // acceptance, and OIDC discovery/login/callback. Pre-login UI hits
    // /health, /version and /v1/auth/oidc/config from this set.
    let public = Router::new()
        .route("/health", get(handle_health))
        .route("/version", get(handle_version))
        .route("/v1/auth/login", post(auth_endpoints::handle_login))
        .route(
            "/v1/auth/login/totp",
            post(auth_endpoints::handle_totp_login),
        )
        .route(
            "/v1/auth/login/enroll/totp/begin",
            post(auth_endpoints::handle_enroll_totp_begin),
        )
        .route(
            "/v1/auth/login/enroll/totp/confirm",
            post(auth_endpoints::handle_enroll_totp_confirm),
        )
        .route("/v1/auth/refresh", post(auth_endpoints::handle_refresh))
        .route("/v1/auth/logout", post(auth_endpoints::handle_logout))
        .route(
            "/v1/auth/password-reset/request",
            post(password_reset::handle_request),
        )
        .route(
            "/v1/auth/password-reset/confirm",
            post(password_reset::handle_confirm),
        )
        .route("/v1/invitations/accept", post(invitations::handle_accept))
        // OIDC/SSO
        .route("/v1/auth/oidc/login", get(oidc::handle_login))
        .route("/v1/auth/oidc/callback", get(oidc::handle_callback))
        .route("/v1/auth/oidc/config", get(oidc::handle_config))
        // Combined sign-in-method probe (issue #138). Canonical replacement
        // for the OIDC-only `/v1/auth/oidc/config`; both are kept for now.
        .route("/v1/auth/config", get(oidc::handle_auth_config));

    // Explicit CORS allowlist (issue #429). The only browser origin that
    // legitimately calls this API cross-origin is a dashboard served from the
    // operator-configured public app URL — when it is unset, the SPA is
    // served same-origin by this very server and needs no CORS at all, so no
    // CORS headers are emitted. No wildcard, no `Allow-Credentials`.
    let cors = hardening::cors_layer(state.app_base_url.as_deref());

    let mut router = authenticated.merge(public).with_state(state);
    if let Some(cors) = cors {
        router = router.layer(cors);
    }
    // Security headers on every API response. main.rs applies the same layer
    // again over the fully assembled app so the SPA fallback and /mcp
    // (mounted after this router is built) are covered too; `if_not_present`
    // keeps the double application idempotent.
    hardening::apply_security_headers(router)
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/poll` — heartbeat + work dispatch with long-poll support.
///
/// If the queue is empty and the runner has capacity, the handler waits up to
/// `LONG_POLL_TIMEOUT` for a `work_notify` signal before returning an empty
/// response. This eliminates the need for runners to busy-poll.
async fn handle_poll(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<PollRequest>,
) -> (StatusCode, Json<PollResponse>) {
    if let Err(s) = require_scope(&ctx, Scope::WORK_POLL) {
        return (
            s,
            Json(PollResponse {
                work: vec![],
                cancel: vec![],
            }),
        );
    }
    // Bind (or verify) this `runner_id` against the authenticated caller
    // BEFORE touching the registry: registering is what triggers the takeover
    // path below, which requeues the incumbent's in-flight executions and
    // fences it out with 409. A caller that does not own the id must not get
    // that far.
    if let Err(s) = runner_identity::authorize_runner(&state, &ctx, &req.runner_id) {
        return (
            s,
            Json(PollResponse {
                work: vec![],
                cancel: vec![],
            }),
        );
    }
    // Update registry heartbeat. A new instance polling under an existing
    // `runner_id` takes the identity over inline (issues #190, #374) — the
    // old session's claims are requeued below. Only the most recently
    // deposed instance is rejected with 409 (fencing), so a duplicate
    // deployment converges to one winner instead of thrashing.
    let (outcome, flapping) = {
        let mut reg = state.runner.registry.write().await;
        match reg.register_or_update(
            &req.runner_id,
            req.capabilities.clone(),
            req.max_inflight,
            req.inflight.clone(),
            req.instance_id.clone(),
            req.tags.clone(),
        ) {
            Ok(outcome) => {
                // Feed the flapping detector under the same lock so two
                // racing takeover polls can't both miss the threshold.
                let flapping = matches!(outcome, RegisterOutcome::TookOver { .. })
                    && reg.record_takeover(&req.runner_id, Utc::now());
                (outcome, flapping)
            }
            Err(conflict) => {
                tracing::warn!(
                    runner_id = %req.runner_id,
                    conflicting_instance = %conflict,
                    "runner instance conflict — this instance was deposed by a newer instance registering under the same runner_id"
                );
                return (
                    StatusCode::CONFLICT,
                    Json(PollResponse {
                        work: vec![],
                        cancel: vec![],
                    }),
                );
            }
        }
    };

    if let RegisterOutcome::TookOver {
        previous_instance_id,
    } = &outcome
    {
        tracing::warn!(
            runner_id = %req.runner_id,
            previous_instance = %previous_instance_id,
            new_instance = ?req.instance_id,
            "new runner instance registered under existing id — taking over previous session"
        );
        if let Some(ref store) = state.store {
            audit::record_event(
                store,
                "system",
                None,
                "runner.takeover",
                "runner",
                Some(&req.runner_id),
            );
        }
        if flapping {
            // ≥3 takeovers of this runner_id within 10 min: two live
            // processes are almost certainly sharing the id (e.g. an
            // accidental duplicate deployment whose fenced loser is
            // restarted by a container restart policy with a fresh
            // instance_id, re-taking the identity in a loop). Jobs keep
            // running, but every switch requeues the loser's claims —
            // warn once per window instead of per takeover.
            tracing::warn!(
                runner_id = %req.runner_id,
                "runner identity flapping — repeated takeovers suggest multiple live processes share this runner_id; give each replica its own runner_id (see docs/operations.md, issue #374)"
            );
            if let Some(ref store) = state.store {
                audit::record_event(
                    store,
                    "system",
                    None,
                    "runner.identity_flapping",
                    "runner",
                    Some(&req.runner_id),
                );
            }
        }
        // Requeue any executions still claimed-in-store by this runner_id
        // on a background task so the poll request returns promptly. The
        // runner won't see the requeued items in *this* response (we polled
        // with `inflight: []` which capped capacity at zero); they appear
        // on the next poll, by which time the spawned task has finished
        // even on a slow disk. Without spawning, a stale runner with many
        // orphaned executions could stall the takeover poll past the
        // long-poll deadline.
        if let (Some(store), Some(dsl_jobs)) = (state.store.clone(), state.dsl_jobs.clone()) {
            let runner_state = Arc::clone(&state.runner);
            let runner_id = req.runner_id.clone();
            let watchdog_counters = Arc::clone(&state.watchdog_counters);
            tokio::spawn(async move {
                let now = Utc::now();
                let store_clone = store.clone();
                let dsl_jobs_handle = dsl_jobs.clone();
                let requeued = crate::watchdog::requeue_abandoned_for_runner(
                    &store,
                    &runner_state,
                    &runner_id,
                    now,
                    |job_key| {
                        // Acquire the lock once per lookup so we don't
                        // clone the entire DSL job list up-front (which
                        // can be expensive on deployments with hundreds
                        // of DSL-managed jobs).
                        if let Ok(jobs) = dsl_jobs_handle.try_read()
                            && let Some(c) = jobs.iter().find(|j| j.key == job_key)
                        {
                            return Some(c.clone());
                        }
                        match store_clone.get_job_definition(job_key) {
                            Ok(Some(def)) => Some(crate::loader::job_config_from_job_def(&def)),
                            _ => None,
                        }
                    },
                )
                .await;
                if !requeued.is_empty() {
                    watchdog_counters.add_dead_runner_requeued(requeued.len() as u64);
                    tracing::info!(
                        runner_id = %runner_id,
                        count = requeued.len(),
                        "inline takeover: requeued abandoned executions"
                    );
                }
            });
        } else if let Some(ref store) = state.store {
            // No DSL jobs map (test mode); fall back to the synchronous
            // path so behaviour stays consistent with pre-fix tests.
            let now = Utc::now();
            let store_clone = Arc::clone(store);
            let requeued = crate::watchdog::requeue_abandoned_for_runner(
                store,
                &state.runner,
                &req.runner_id,
                now,
                |job_key| match store_clone.get_job_definition(job_key) {
                    Ok(Some(def)) => Some(crate::loader::job_config_from_job_def(&def)),
                    _ => None,
                },
            )
            .await;
            if !requeued.is_empty() {
                state
                    .watchdog_counters
                    .add_dead_runner_requeued(requeued.len() as u64);
                tracing::info!(
                    runner_id = %req.runner_id,
                    count = requeued.len(),
                    "inline takeover: requeued abandoned executions"
                );
            }
        }
    }

    let capacity = (req.max_inflight as usize).saturating_sub(req.inflight.len());

    if capacity == 0 {
        // Runner is at capacity — no work to hand out, but still deliver
        // any pending cancels so an operator's cancel of a long-running job
        // reaches the runner without waiting for the next slot to free up
        // (issue #176).
        let cancel = state.runner.drain_cancels(&req.runner_id).await;
        return (
            StatusCode::OK,
            Json(PollResponse {
                work: vec![],
                cancel,
            }),
        );
    }

    // Try to dequeue immediately; if nothing available, long-poll for up to
    // LONG_POLL_TIMEOUT waiting for a work_notify signal. The same
    // `work_notify` channel is also pinged by `AppState::push_cancel` so a
    // long-poll wakes up when a cancel arrives, not only when work does.
    loop {
        // Set up the notification listener BEFORE checking the queue so we
        // cannot miss an enqueue that races with our check.
        let notified = state.runner.work_notify.notified();

        // Global maintenance freezes dispatch: hand out no new work, but keep
        // the long-poll alive (so runners don't hot-loop) and still deliver
        // cancels. Queued work resumes when maintenance clears — the PUT
        // handler pings `work_notify` so a waiting poll re-evaluates promptly.
        let frozen = state
            .maintenance
            .read()
            .map(|m| m.is_active(Utc::now()))
            .unwrap_or(false);
        let work = if frozen {
            Vec::new()
        } else {
            try_dequeue_for(&state, &req.runner_id, &req.capabilities, capacity).await
        };
        let cancel = state.runner.drain_cancels(&req.runner_id).await;

        if !work.is_empty() || !cancel.is_empty() {
            return (StatusCode::OK, Json(PollResponse { work, cancel }));
        }

        // Queue empty AND no pending cancels — wait for a notification or timeout
        tokio::select! {
            _ = notified => {
                // A new item was enqueued OR a cancel was pushed — loop and try again
            }
            _ = tokio::time::sleep(state.long_poll_timeout) => {
                // Timeout: return empty response, runner will poll again
                return (StatusCode::OK, Json(PollResponse { work: vec![], cancel: vec![] }));
            }
        }
    }
}

/// Attempt to dequeue items for a runner without blocking.
///
/// Enforces the per-job concurrency guard (issue #278): items whose job
/// carries a `__max_concurrent` metadata limit are only dequeued while the
/// number of currently claimed (in-flight) executions of that job — counted
/// authoritatively from the store — is below the limit. Blocked items stay
/// in the queue at their FIFO position (they are skipped in place, so they
/// neither get dropped nor starve other jobs' items behind them) and are
/// picked up by a later poll once a slot frees.
///
/// The guard check, the dequeue, and the store-side claim all happen under
/// the queue write lock, so two concurrent polls cannot both observe a free
/// slot and double-claim a singleton job. The registry lock is only taken
/// after the queue lock is released, preserving the registry→queue lock
/// order used elsewhere (e.g. `handle_health`).
async fn try_dequeue_for(
    state: &Arc<ServerState>,
    runner_id: &str,
    capabilities: &[String],
    capacity: usize,
) -> Vec<WorkAssignment> {
    let mut q = state.runner.queue.write().await;

    // In-flight counts fetched from the store during this call, so a queue
    // full of items from one guarded job costs one count query, not one per
    // item. Store state cannot change concurrently in a way that matters:
    // claims serialize on the queue lock we hold.
    let mut inflight_cache: HashMap<String, u64> = HashMap::new();
    // Executions assigned in THIS batch per job_key — a capacity-2 poll must
    // not hand out two executions of the same singleton job in one response.
    let mut batch_claims: HashMap<String, u64> = HashMap::new();

    let mut items: Vec<WorkItem> = Vec::new();
    while items.len() < capacity {
        let next = q.dequeue_for_where(capabilities, |item| {
            has_free_concurrency_slot(state, item, &batch_claims, &mut inflight_cache)
        });
        let Some(item) = next else { break };
        *batch_claims.entry(item.job_key.clone()).or_insert(0) += 1;
        items.push(item);
    }

    if items.is_empty() {
        return vec![];
    }

    // Mark claimed executions in the persistent store so we track runner_id.
    // This happens before the queue lock is released so the next poll's
    // guard check already sees these rows as claimed.
    if let Some(ref store) = state.store {
        let now = Utc::now();
        items.retain(|item| {
            let Ok(id) = uuid::Uuid::parse_str(&item.execution_id) else {
                return true;
            };
            match store.claim_execution(id, runner_id, now) {
                Ok(_) => true,
                // The row is no longer `queued` — cancelled, completed, or
                // claimed via another path. Handing the item out anyway
                // would start a run whose completion the store's CAS guard
                // then rejects (issue #374), so drop it instead.
                Err(croniq_store::traits::StoreError::Conflict(_))
                | Err(croniq_store::traits::StoreError::NotFound(_)) => {
                    tracing::warn!(
                        execution_id = %item.execution_id,
                        job_key = %item.job_key,
                        "work item dropped — execution is no longer queued in the store"
                    );
                    false
                }
                // Fail-open on infrastructure errors, matching the
                // concurrency guard above: blocking all dispatch on a
                // transient store error is worse than a rare extra run.
                Err(e) => {
                    tracing::warn!(
                        execution_id = %item.execution_id,
                        error = %e,
                        "claim persist failed — dispatching anyway (fail-open)"
                    );
                    true
                }
            }
        });
    }
    drop(q);

    let mut reg = state.runner.registry.write().await;
    items
        .into_iter()
        .filter(|item| reg.claim(runner_id, &item.execution_id))
        .map(WorkAssignment::from)
        .collect()
}

/// Read the `__max_concurrent` limit from a work item's metadata.
/// `None` = unguarded job. Zero or malformed values are treated as
/// unguarded — the DSL validator rejects them at config time.
fn concurrency_limit(item: &WorkItem) -> Option<u64> {
    item.metadata
        .get(croniq_config::compile::MAX_CONCURRENT_METADATA_KEY)?
        .as_str()?
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
}

/// Decide whether `item` may be dispatched given its job's concurrency
/// limit. Counts `Claimed` executions of the job in the store (memoised in
/// `inflight_cache` for this poll) plus what the current batch already
/// assigned.
///
/// Fail-open by design: with no store configured (bare test/dev servers)
/// in-flight executions are not tracked anywhere authoritative, and on a
/// store error blocking all dispatch would be worse than a rare extra
/// concurrent run — both cases dispatch and log at debug/warn.
fn has_free_concurrency_slot(
    state: &ServerState,
    item: &WorkItem,
    batch_claims: &HashMap<String, u64>,
    inflight_cache: &mut HashMap<String, u64>,
) -> bool {
    let Some(limit) = concurrency_limit(item) else {
        return true;
    };
    let batch = batch_claims.get(&item.job_key).copied().unwrap_or(0);
    if batch >= limit {
        return false;
    }
    let Some(ref store) = state.store else {
        tracing::debug!(
            job_key = %item.job_key,
            "concurrency guard: no store configured — cannot count in-flight executions, dispatching"
        );
        return true;
    };
    let inflight = match inflight_cache.get(&item.job_key) {
        Some(n) => *n,
        None => match store.count_executions_in_states(&item.job_key, &[ExecutionState::Claimed]) {
            Ok(n) => {
                inflight_cache.insert(item.job_key.clone(), n);
                n
            }
            Err(e) => {
                tracing::warn!(
                    job_key = %item.job_key,
                    error = %e,
                    "concurrency guard: store count failed — dispatching without guard"
                );
                return true;
            }
        },
    };
    inflight + batch < limit
}

/// `POST /v1/complete` — release inflight + forward to processor.
async fn handle_complete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CompleteRequest>,
) -> (StatusCode, Json<CompleteResponse>) {
    if let Err(s) = require_scope(&ctx, Scope::WORK_ACK) {
        return (s, Json(CompleteResponse { received: false }));
    }
    // The store's completion CAS fences on `runner_id` (see
    // `complete_execution`), but the value used to come straight from the
    // request body — naming the victim's runner made the CAS *match*. Bind the
    // body's `runner_id` to the caller first, so the fence value is one the
    // caller is entitled to use.
    if let Err(s) = runner_identity::authorize_runner(&state, &ctx, &req.runner_id) {
        return (s, Json(CompleteResponse { received: false }));
    }
    // Ownership of the *execution* still matters: owning `runner_id` does not
    // mean this execution was dispatched to it. The store CAS enforces that
    // (state must be `claimed` with a matching `runner_id`); the release below
    // only touches this runner's own in-flight list.
    {
        let mut reg = state.runner.registry.write().await;
        reg.release(&req.runner_id, &req.execution_id);
    }

    let event = CompletionEvent {
        runner_id: req.runner_id.clone(),
        execution_id: req.execution_id.clone(),
        status: req.status,
        error: req.error.clone(),
        duration_ms: req.duration_ms,
        attempt: req.attempt,
    };

    if let Err(e) = state.completion_tx.send(event) {
        tracing::error!(error = %e, "completion channel closed");
    }

    (StatusCode::OK, Json(CompleteResponse { received: true }))
}

/// `GET /v1/runners` — list all known runners with liveness status.
async fn handle_list_runners(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<RunnerSummary>>, StatusCode> {
    require_scope(&ctx, Scope::RUNNERS_READ)?;
    let now = Utc::now();
    let reg = state.runner.registry.read().await;

    let summaries = reg
        .all()
        .map(|r| RunnerSummary {
            runner_id: r.runner_id.clone(),
            status: r.status_at_with_ttl(now, state.runner.lease_ttl_secs),
            capabilities: r.capabilities.clone(),
            max_inflight: r.max_inflight,
            inflight: r.inflight.len(),
            last_poll_at: r.last_poll_at,
            tags: r.tags.clone(),
        })
        .collect();

    Ok(Json(summaries))
}

/// `DELETE /v1/runners/{id}` — deregister a runner.
async fn handle_delete_runner(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(runner_id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_scope(&ctx, Scope::RUNNERS_WRITE) {
        return s;
    }
    {
        let mut reg = state.runner.registry.write().await;
        reg.remove(&runner_id);
    }
    // Deregistering is the operator-driven handover: it frees the id so a
    // different credential can claim it on its next poll.
    runner_identity::release_runner(&state, &runner_id);
    StatusCode::NO_CONTENT
}

/// `POST /v1/trigger` — immediately enqueue a job execution.
///
/// Persists the execution to the store first (just like the scheduler does)
/// so that the CompletionProcessor can find it for retries and dead-lettering.
///
/// Supports optional caller-supplied idempotency keys (issue #279): a
/// repeat trigger carrying the same `(job_key, idempotency_key)` coalesces
/// to the existing execution — while that execution is still
/// queued/claimed, or for `state.trigger_dedup_window_secs` after it was
/// created — and responds with `deduplicated: true` instead of enqueuing a
/// duplicate. The check-then-insert is best-effort (two truly concurrent
/// identical triggers may both pass the lookup): the endpoint dedups
/// at-least-once producers, it is NOT a strict exactly-once guarantee.
async fn handle_trigger(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<TriggerRequest>,
) -> (StatusCode, HeaderMap, Json<TriggerResponse>) {
    // Follows this handler's established error style: status code + an
    // empty TriggerResponse body (same as the scope-rejection arm below).
    // The queue-overflow arm below sets a `Retry-After` header; every other
    // arm carries an empty one.
    let error_response = |status: StatusCode| {
        (
            status,
            HeaderMap::new(),
            Json(TriggerResponse {
                execution_id: String::new(),
                queued: 0,
                deduplicated: false,
            }),
        )
    };

    if let Err(s) = require_scope(&ctx, Scope::JOBS_TRIGGER) {
        return error_response(s);
    }

    // An empty idempotency_key is treated as absent; an oversized one is a
    // caller bug and gets rejected rather than silently truncated.
    let idempotency_key = req
        .idempotency_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(str::to_string);
    if let Some(ref key) = idempotency_key
        && key.chars().count() > MAX_IDEMPOTENCY_KEY_CHARS
    {
        tracing::warn!(
            job_key = %req.job_key,
            key_chars = key.chars().count(),
            "trigger rejected: idempotency_key exceeds {MAX_IDEMPOTENCY_KEY_CHARS} characters"
        );
        return error_response(StatusCode::BAD_REQUEST);
    }

    let now = Utc::now();

    // Dedup lookup. Requires the store: without one (possible in tests and
    // store-less dev servers — `state.store` is an Option) there is nothing
    // to look executions up in, so dedup silently degrades to a no-op and
    // every trigger enqueues normally. Store errors also fail open: an
    // occasional duplicate execution beats refusing the trigger outright.
    if let (Some(key), Some(store)) = (idempotency_key.as_deref(), state.store.as_ref()) {
        let window_start = now - chrono::Duration::seconds(state.trigger_dedup_window_secs as i64);
        match store.find_execution_by_idempotency_key(&req.job_key, key, window_start) {
            Ok(Some(existing)) => {
                let queued = state.runner.queue.read().await.len();
                tracing::info!(
                    job_key = %req.job_key,
                    execution_id = %existing.id,
                    "trigger deduplicated via idempotency_key — returning existing execution"
                );
                return (
                    StatusCode::OK,
                    HeaderMap::new(),
                    Json(TriggerResponse {
                        execution_id: existing.id.to_string(),
                        queued,
                        deduplicated: true,
                    }),
                );
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(
                    job_key = %req.job_key,
                    error = %e,
                    "idempotency_key dedup lookup failed — proceeding with a fresh trigger"
                );
            }
        }
    }

    // Per-job queue-overflow cap (#299). The scheduler bounds *scheduled*
    // fires at `max_queue_depth` (per-job override, default 10 — see
    // `scheduler::tick`); a manual `POST /v1/trigger` must honour the same cap
    // or a burst of triggers (event storms, client retries, a hot producer)
    // can pile queued executions up unbounded for one job — the exact overflow
    // the scheduler guard prevents. Checked after dedup (a coalesced trigger
    // adds nothing to the queue) and before persisting the execution row, so a
    // rejected trigger leaves no orphan row behind. `dsl_jobs` unset (store-less
    // dev servers, tests) or an unknown `job_key` falls back to the default 10.
    let max_queue_depth = if let Some(ref dsl_jobs) = state.dsl_jobs {
        let jobs = dsl_jobs.read().await;
        jobs.iter()
            .find(|j| j.key == req.job_key)
            .and_then(|j| j.max_queue_depth)
            .unwrap_or(10)
    } else {
        10
    } as usize;
    let queued_for_job = state.runner.queue.read().await.count_for_job(&req.job_key);
    if queued_for_job >= max_queue_depth {
        tracing::warn!(
            job_key = %req.job_key,
            queued = queued_for_job,
            max = max_queue_depth,
            "trigger rejected — per-job queue overflow (#299)"
        );
        // Backpressure: tell producers (and the SDKs, which surface it as
        // `retryAfterMs`) how long to wait before retrying instead of
        // hammering a full queue.
        let mut headers = HeaderMap::new();
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from(TRIGGER_OVERFLOW_RETRY_AFTER_SECS),
        );
        return (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            Json(TriggerResponse {
                execution_id: String::new(),
                queued: 0,
                deduplicated: false,
            }),
        );
    }

    let exec_uuid = uuid::Uuid::new_v4();
    let execution_id = exec_uuid.to_string();

    // Build metadata: start from the DSL job's compiled metadata so that
    // __runner_exec (and other DSL-stamped keys) survive into the WorkItem
    // and the DB execution row. The caller's req.metadata values are overlaid
    // on top so they can still override or extend individual entries.
    let mut metadata: HashMap<String, String> = if let Some(ref dsl_jobs) = state.dsl_jobs {
        let jobs = dsl_jobs.read().await;
        jobs.iter()
            .find(|j| j.key == req.job_key)
            .map(|j| j.metadata.clone())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    if let serde_json::Value::Object(ref map) = req.metadata {
        for (k, v) in map {
            // The `__`-prefixed namespace is reserved for keys the
            // scheduler / DSL compiler stamp (`__runner_exec`, `__require`,
            // `__prefer`, `__max_concurrent`, …) and that runners act on
            // directly. Caller-supplied metadata must not reach into it —
            // overriding `__require`/`__max_concurrent` would subvert
            // routing and the concurrency guard, and `__runner_exec` is
            // consumed verbatim by the shell runner. Drop such keys; the
            // caller's own `require`/`prefer` request fields (applied
            // below) are the supported way to influence those.
            if croniq_config::compile::is_reserved_metadata_key(k) {
                tracing::debug!(
                    job_key = %req.job_key,
                    key = %k,
                    "trigger: ignoring caller metadata key in reserved `__` namespace"
                );
                continue;
            }
            metadata.insert(k.clone(), v.as_str().unwrap_or(&v.to_string()).to_string());
        }
    }
    if !req.require.is_empty() {
        metadata.insert(
            "__require".into(),
            serde_json::to_string(&req.require).unwrap_or_default(),
        );
    }
    if !req.prefer.is_empty() {
        metadata.insert(
            "__prefer".into(),
            serde_json::to_string(&req.prefer).unwrap_or_default(),
        );
    }

    // Persist the execution record to the store so that the CompletionProcessor
    // can find it when the runner reports success/failure.
    if let Some(ref store) = state.store {
        let execution = Execution {
            id: exec_uuid,
            job_key: req.job_key.clone(),
            fire_at: now,
            // Manual trigger: the logical fire time is the moment of the
            // trigger call itself.
            scheduled_for: now,
            attempt: 1,
            state: ExecutionState::Queued,
            runner_id: None,
            claimed_at: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            dead_reason: None,
            idempotency_key: idempotency_key.clone(),
            metadata: metadata.clone(),
            created_at: now,
        };
        if let Err(e) = store.create_execution(&execution) {
            // Enqueueing anyway would hand a runner an execution_id with no
            // backing row: the CompletionProcessor could never record its
            // outcome and the run would vanish without history. Reject the
            // trigger instead; the caller can retry.
            tracing::error!(job_key = %req.job_key, error = %e, "failed to persist triggered execution — trigger rejected");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let item = WorkItem {
        execution_id: execution_id.clone(),
        job_key: req.job_key,
        fire_at: now,
        scheduled_for: now,
        attempt: 1,
        require: req.require,
        prefer: req.prefer,
        metadata: serde_json::to_value(&metadata).unwrap_or_default(),
        timeout: req.timeout,
    };

    let queued = {
        let mut q = state.runner.queue.write().await;
        q.enqueue(item);
        q.len()
    };
    state.runner.work_notify.notify_waiters();

    (
        StatusCode::OK,
        HeaderMap::new(),
        Json(TriggerResponse {
            execution_id,
            queued,
            deduplicated: false,
        }),
    )
}

/// `GET /v1/executions` — list recent executions from the store.
async fn handle_list_executions(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = ExecutionFilter {
        job_key: params.get("job_key").cloned(),
        runner_id: params.get("runner_id").cloned(),
        state: params.get("state").and_then(|s| match s.as_str() {
            "queued" => Some(croniq_store::models::ExecutionState::Queued),
            "claimed" => Some(croniq_store::models::ExecutionState::Claimed),
            "completed" => Some(croniq_store::models::ExecutionState::Completed),
            "failed" => Some(croniq_store::models::ExecutionState::Failed),
            "dead" => Some(croniq_store::models::ExecutionState::Dead),
            "cancelled" => Some(croniq_store::models::ExecutionState::Cancelled),
            _ => None,
        }),
        limit: params.get("limit").and_then(|l| l.parse().ok()),
        ..Default::default()
    };
    let executions = store
        .list_executions(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::to_value(&executions).unwrap_or_default()))
}

/// `GET /health`
async fn handle_health(State(state): State<Arc<ServerState>>) -> Json<HealthResponse> {
    let now = Utc::now();
    let reg = state.runner.registry.read().await;
    let queue = state.runner.queue.read().await;

    Json(HealthResponse {
        status: "ok".into(),
        runners_online: reg
            .by_status_with_ttl(RunnerStatus::Online, now, state.runner.lease_ttl_secs)
            .len(),
        runners_stale: reg
            .by_status_with_ttl(RunnerStatus::Stale, now, state.runner.lease_ttl_secs)
            .len(),
        runners_dead: reg
            .by_status_with_ttl(RunnerStatus::Dead, now, state.runner.lease_ttl_secs)
            .len(),
        queued: queue.len(),
    })
}

/// `GET /version` response — build + environment metadata.
///
/// Public (no auth) so the login page can render a live version chip before
/// the user has a token. All four values are non-sensitive: the Cargo
/// version, the short git SHA, the build timestamp, and a deploy-environment
/// label (`production`, `staging`, `dev`, …) read from `CRONIQ_ENV`.
#[derive(Debug, Clone, Serialize)]
pub struct VersionResponse {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub build_time: String,
    pub env: String,
}

/// Cargo package version, baked in at compile time.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git SHA, stamped by `build.rs`. Falls back to `"unknown"` outside
/// a git checkout (release tarball, no `git` on PATH).
const GIT_SHA: &str = env!("CRONIQ_GIT_SHA");

/// Unix seconds at which this binary was built. Stamped by `build.rs` and
/// formatted as RFC 3339 at request time.
const BUILD_TIME_UNIX: &str = env!("CRONIQ_BUILD_TIME_UNIX");

/// `GET /version` — build + environment metadata.
async fn handle_version() -> Json<VersionResponse> {
    let build_time = BUILD_TIME_UNIX
        .parse::<i64>()
        .ok()
        .and_then(|s| DateTime::<Utc>::from_timestamp(s, 0))
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "unknown".into());

    let env = std::env::var("CRONIQ_ENV").unwrap_or_else(|_| "unknown".into());

    Json(VersionResponse {
        version: VERSION,
        git_sha: GIT_SHA,
        build_time,
        env,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    use super::*;
    use croniq_runner::WorkItem;

    fn make_state() -> (Arc<ServerState>, mpsc::UnboundedReceiver<CompletionEvent>) {
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();
        // Use a very short long-poll timeout in tests so they complete quickly
        let state = ServerState::with_timeout(runner, tx, Duration::from_millis(50));
        (state, rx)
    }

    async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn get_json(app: Router, uri: &str) -> serde_json::Value {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn poll_registers_runner() {
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": ["billing"],
                "max_inflight": 3,
                "inflight": []
            }),
        )
        .await;

        assert_eq!(resp["work"].as_array().unwrap().len(), 0);

        let reg = state.runner.registry.read().await;
        assert!(reg.get("r1").is_some());
    }

    #[tokio::test]
    async fn poll_dispatches_queued_work() {
        let (state, _rx) = make_state();

        {
            let mut q = state.runner.queue.write().await;
            q.enqueue(WorkItem {
                execution_id: "exec-1".into(),
                job_key: "billing:invoice".into(),
                fire_at: chrono::Utc::now(),
                scheduled_for: chrono::Utc::now(),
                attempt: 1,
                require: vec![],
                prefer: vec![],
                metadata: serde_json::json!({}),
                timeout: "15m".into(),
            });
        }

        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": [],
                "max_inflight": 3,
                "inflight": []
            }),
        )
        .await;

        let work = resp["work"].as_array().unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0]["execution_id"], "exec-1");
    }

    #[tokio::test]
    async fn complete_releases_inflight() {
        let (state, _rx) = make_state();

        {
            let mut reg = state.runner.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-42".into()], None, vec![]);
        }

        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/complete",
            serde_json::json!({
                "runner_id": "r1",
                "execution_id": "exec-42",
                "status": "success",
                "duration_ms": 1200
            }),
        )
        .await;

        assert_eq!(resp["received"], true);

        let reg = state.runner.registry.read().await;
        assert!(reg.get("r1").unwrap().inflight.is_empty());
    }

    #[tokio::test]
    async fn complete_forwards_to_channel() {
        let (state, mut rx) = make_state();

        {
            let mut reg = state.runner.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-99".into()], None, vec![]);
        }

        let app = server_router(Arc::clone(&state));

        post_json(
            app,
            "/v1/complete",
            serde_json::json!({
                "runner_id": "r1",
                "execution_id": "exec-99",
                "status": "failure",
                "error": "Connection refused",
                "duration_ms": 250
            }),
        )
        .await;

        let event = rx.try_recv().unwrap();
        assert_eq!(event.execution_id, "exec-99");
        assert_eq!(event.error.as_deref(), Some("Connection refused"));
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = get_json(app, "/health").await;
        assert_eq!(resp["status"], "ok");
        assert_eq!(resp["queued"], 0);
    }

    #[tokio::test]
    async fn runner_status_uses_configured_lease_ttl() {
        // With `pull_api.lease_ttl 300s` a runner that last polled 150 s ago
        // is Stale (stale threshold = ttl/2), not Dead. The API and health
        // counters must agree with the watchdog's TTL-based assessment
        // instead of the old hardcoded 120 s default, which would have
        // reported this runner as dead while the watchdog still treated it
        // as alive.
        let runner = AppState::with_lease_ttl(300);
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::new(runner, tx);

        {
            let mut reg = state.runner.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 1, vec![], None, vec![]);
            reg.get_mut("r1").unwrap().last_poll_at = Utc::now() - chrono::Duration::seconds(150);
        }

        let runners = get_json(server_router(Arc::clone(&state)), "/v1/runners").await;
        assert_eq!(runners[0]["runner_id"], "r1");
        assert_eq!(runners[0]["status"], "stale");

        let health = get_json(server_router(Arc::clone(&state)), "/health").await;
        assert_eq!(health["runners_stale"], 1);
        assert_eq!(health["runners_dead"], 0);
        assert_eq!(health["runners_online"], 0);
    }

    #[tokio::test]
    async fn version_returns_build_metadata() {
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = get_json(app, "/version").await;
        // Cargo always sets CARGO_PKG_VERSION, so this is never "unknown".
        assert_eq!(resp["version"], env!("CARGO_PKG_VERSION"));
        // git_sha + build_time are stamped by build.rs. We don't assert exact
        // values (they change every commit/build), only that they're present.
        assert!(resp["git_sha"].is_string());
        assert!(resp["build_time"].is_string());
        // env is read from CRONIQ_ENV at request time. The test process may or
        // may not have it set, but the field must always be a string.
        assert!(resp["env"].is_string());
    }

    #[tokio::test]
    async fn version_is_public() {
        // The login page renders before auth, so /version must not require a
        // token even when JWT auth is configured.
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(app, "GET", "/version", None, None).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn version_reads_env_var() {
        // SAFETY: tests in this module run on the tokio current-thread runtime
        // and don't race on env vars within a single test. CRONIQ_ENV is only
        // read by handle_version, so isolating to one test is sufficient.
        //
        // # Safety
        // `set_var` / `remove_var` are unsafe in Rust 2024 because they mutate
        // process-global state that may be observed by other threads. We
        // accept the risk here: the test runtime is single-threaded for this
        // test and no other code path reads CRONIQ_ENV concurrently.
        unsafe {
            std::env::set_var("CRONIQ_ENV", "staging");
        }
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = get_json(app, "/version").await;
        assert_eq!(resp["env"], "staging");

        unsafe {
            std::env::remove_var("CRONIQ_ENV");
        }
    }

    // ─── Auth middleware tests ────────────────────────────────────────────────

    fn make_auth_state(
        secret: &str,
    ) -> (Arc<ServerState>, mpsc::UnboundedReceiver<CompletionEvent>) {
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();
        let jwt_config = JwtConfig {
            secret: secret.to_string(),
            ..Default::default()
        };
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: Some(jwt_config),
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        });
        (state, rx)
    }

    async fn status_of(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        bearer: Option<&str>,
    ) -> u16 {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let body = body
            .map(|b| Body::from(b.to_string()))
            .unwrap_or(Body::empty());
        let resp = app.oneshot(builder.body(body).unwrap()).await.unwrap();
        resp.status().as_u16()
    }

    #[tokio::test]
    async fn auth_rejects_without_token() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(
            app,
            "POST",
            "/v1/poll",
            Some(serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
            })),
            None,
        )
        .await;

        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn auth_rejects_invalid_jwt() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(
            app,
            "POST",
            "/v1/poll",
            Some(serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
            })),
            Some("invalid.jwt.token"),
        )
        .await;

        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn auth_accepts_valid_jwt() {
        let (state, _rx) = make_auth_state("test-secret");
        let jwt_config = state.jwt_config.as_ref().unwrap();
        let pair = croniq_auth::jwt::issue_token_pair(
            jwt_config,
            "test-user",
            "test-client",
            croniq_auth::CallerType::User,
            Some("test-user"),
            Some(croniq_auth::Role::Admin),
            croniq_auth::AuthMethod::Password,
            &["admin".into()],
        )
        .unwrap();

        let app = server_router(Arc::clone(&state));
        let status = status_of(
            app,
            "POST",
            "/v1/poll",
            Some(serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
            })),
            Some(&pair.access_token),
        )
        .await;

        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn auth_health_is_public() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(app, "GET", "/health", None, None).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn auth_login_is_public() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        // Login endpoint should be reachable (will fail with 503 since no store)
        let status = status_of(
            app,
            "POST",
            "/v1/auth/login",
            Some(serde_json::json!({
                "username": "admin", "password": "pass"
            })),
            None,
        )
        .await;

        // 503 because no store configured, but NOT 401/404
        assert_eq!(status, 503);
    }

    // ─── Trigger endpoint: DSL metadata propagation (issue #89) ─────────────

    #[tokio::test]
    async fn trigger_inherits_dsl_runner_exec_metadata() {
        // Regression for issue #89: POST /v1/trigger must include __runner_exec
        // from the DSL-compiled job metadata so the shell runner can decode the
        // command. {{...}} inside quoted command strings must survive the round-trip.
        use crate::loader::load_str;
        use croniq_config::compile::RUNNER_EXEC_METADATA_KEY;

        let dsl = r#"
            job test:docker-ps {
                every 1 hour
                runner { require shell-runner }
                runner shell {
                    command "docker ps --format '{{.Image}}'"
                }
            }
        "#;
        let loaded = load_str(dsl).unwrap();
        let jobs = loaded.runtime.jobs;
        assert!(
            jobs[0].metadata.contains_key(RUNNER_EXEC_METADATA_KEY),
            "compile should stamp __runner_exec: {:?}",
            jobs[0].metadata
        );

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dsl_jobs = Arc::new(tokio::sync::RwLock::new(jobs));
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: Some(dsl_jobs),
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        });
        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/trigger",
            serde_json::json!({
                "job_key": "test:docker-ps",
                "metadata": {},
                "require": [],
                "prefer": []
            }),
        )
        .await;

        assert!(
            resp["execution_id"].is_string(),
            "expected execution_id in response, got: {resp}"
        );

        // The WorkItem in the queue must carry __runner_exec so the shell runner
        // can decode the command string (the original failure mode from issue #89).
        let q = state.runner.queue.read().await;
        let items = q.peek_n(1);
        assert_eq!(items.len(), 1, "one item should be queued");
        assert!(
            items[0].metadata.get(RUNNER_EXEC_METADATA_KEY).is_some(),
            "__runner_exec must be present in WorkItem.metadata; got: {:?}",
            items[0].metadata
        );
    }

    #[tokio::test]
    async fn trigger_request_metadata_overrides_dsl_metadata() {
        // Caller-supplied metadata overrides DSL values but does not wipe DSL keys
        // that the caller did not touch (e.g. __runner_exec stays present).
        use crate::loader::load_str;
        use croniq_config::compile::RUNNER_EXEC_METADATA_KEY;

        let dsl = r#"
            job test:override {
                every 1 hour
                runner shell { command "echo hello" }
                metadata { env prod }
            }
        "#;
        let loaded = load_str(dsl).unwrap();
        let jobs = loaded.runtime.jobs;

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dsl_jobs = Arc::new(tokio::sync::RwLock::new(jobs));
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: Some(dsl_jobs),
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        });
        let app = server_router(Arc::clone(&state));

        post_json(
            app,
            "/v1/trigger",
            serde_json::json!({
                "job_key": "test:override",
                "metadata": { "env": "staging" },
                "require": [],
                "prefer": []
            }),
        )
        .await;

        let q = state.runner.queue.read().await;
        let items = q.peek_n(1);
        assert_eq!(items.len(), 1);
        // __runner_exec from DSL must survive
        assert!(
            items[0].metadata.get(RUNNER_EXEC_METADATA_KEY).is_some(),
            "__runner_exec must survive caller override"
        );
        // Caller's env=staging must override DSL env=prod
        assert_eq!(
            items[0].metadata["env"].as_str().unwrap(),
            "staging",
            "caller env must override DSL env"
        );
    }

    #[tokio::test]
    async fn trigger_caller_metadata_cannot_touch_reserved_namespace() {
        // Caller-supplied metadata must not reach into the `__`-prefixed
        // namespace: __runner_exec is consumed verbatim by the shell runner,
        // and __require / __max_concurrent drive routing and the concurrency
        // guard. The DSL-stamped __runner_exec must win over any caller
        // attempt to replace it, and a caller-invented __max_concurrent must
        // not appear on the work item.
        use crate::loader::load_str;
        use croniq_config::compile::RUNNER_EXEC_METADATA_KEY;

        let dsl = r#"
            job test:reserved {
                every 1 hour
                runner shell { command "echo legit" }
            }
        "#;
        let loaded = load_str(dsl).unwrap();
        let jobs = loaded.runtime.jobs;
        let dsl_runner_exec = jobs[0]
            .metadata
            .get(RUNNER_EXEC_METADATA_KEY)
            .cloned()
            .expect("DSL should stamp __runner_exec");

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dsl_jobs = Arc::new(tokio::sync::RwLock::new(jobs));
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: Some(dsl_jobs),
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            policy_strict_calendars: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            config_path: None,
            boot_only_settings: None,
            reload_counters: ReloadCounters::new(),
            watchdog_counters: WatchdogCounters::new(),
            config_faults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            email_sender: crate::email::default_sender(),
            app_base_url: None,
            oidc: None,
            password_login_enabled: true,
            require_totp: false,
            retention_configured: false,
            alerts: croniq_config::compile::AlertsConfig::default(),
            console_hub: None,
            scheduler_heartbeat: None,
            trigger_dedup_window_secs: DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
            runner_identity_binding: true,
        });
        let app = server_router(Arc::clone(&state));

        post_json(
            app,
            "/v1/trigger",
            serde_json::json!({
                "job_key": "test:reserved",
                "metadata": {
                    "__runner_exec": "{\"Exec\":{\"argv\":[\"/bin/sh\",\"-c\",\"rm -rf /\"]}}",
                    "__max_concurrent": "999",
                    "env": "staging"
                },
                "require": [],
                "prefer": []
            }),
        )
        .await;

        let q = state.runner.queue.read().await;
        let items = q.peek_n(1);
        assert_eq!(items.len(), 1);
        // The DSL-stamped payload wins; the caller's injected one is dropped.
        assert_eq!(
            items[0]
                .metadata
                .get(RUNNER_EXEC_METADATA_KEY)
                .and_then(|v| v.as_str()),
            Some(dsl_runner_exec.as_str()),
            "caller must not override the reserved __runner_exec key"
        );
        // A caller-invented reserved key never lands on the work item.
        assert!(
            items[0].metadata.get("__max_concurrent").is_none(),
            "caller must not inject reserved __max_concurrent"
        );
        // Non-reserved caller metadata still flows through.
        assert_eq!(items[0].metadata["env"].as_str().unwrap(), "staging");
    }

    // ─── Trigger endpoint: idempotency_key dedup (issue #279) ───────────────

    fn make_store_state() -> (Arc<ServerState>, crate::store::DynStore) {
        let store =
            crate::store::sqlite_store(croniq_store::sqlite::SqliteStore::in_memory().unwrap());
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::with_auth(runner, tx, None, Some(store.clone()));
        (state, store)
    }

    async fn post_json_status(
        app: Router,
        uri: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// Seed an execution row carrying an idempotency key directly in the
    /// store, bypassing the endpoint — lets tests control `created_at` and
    /// terminal state without sleeping through the dedup window.
    fn seed_keyed_execution(
        store: &crate::store::DynStore,
        job_key: &str,
        key: &str,
        state: ExecutionState,
        created_at: DateTime<Utc>,
    ) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: created_at,
                scheduled_for: created_at,
                attempt: 1,
                state,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: Some(key.into()),
                metadata: HashMap::new(),
                created_at,
            })
            .unwrap();
        id
    }

    #[tokio::test]
    async fn trigger_with_idempotency_key_dedups_while_queued() {
        let (state, store) = make_store_state();

        let (status, first) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": "evt-1" }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(first["deduplicated"], false);
        assert_eq!(first["queued"], 1);
        let first_id = first["execution_id"].as_str().unwrap().to_string();

        // The key was persisted on the execution row.
        let row = store
            .get_execution(uuid::Uuid::parse_str(&first_id).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(row.idempotency_key.as_deref(), Some("evt-1"));

        // Second trigger with the same key while the first is still queued
        // coalesces: same execution_id, deduplicated=true, nothing enqueued.
        let (status, second) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": "evt-1" }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(second["deduplicated"], true);
        assert_eq!(second["execution_id"].as_str().unwrap(), first_id);
        assert_eq!(second["queued"], 1, "dedup must not enqueue a new item");

        let q = state.runner.queue.read().await;
        assert_eq!(
            q.len(),
            1,
            "queue length must be unchanged by the dedup hit"
        );
    }

    #[tokio::test]
    async fn trigger_without_key_creates_distinct_executions() {
        let (state, _store) = make_store_state();

        let (_, first) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice" }),
        )
        .await;
        let (_, second) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice" }),
        )
        .await;

        assert_eq!(first["deduplicated"], false);
        assert_eq!(second["deduplicated"], false);
        assert_ne!(
            first["execution_id"], second["execution_id"],
            "keyless triggers must never dedup"
        );

        let q = state.runner.queue.read().await;
        assert_eq!(q.len(), 2);
    }

    #[tokio::test]
    async fn trigger_enforces_per_job_queue_overflow_cap() {
        // #299: POST /v1/trigger must honour the same per-job max_queue_depth
        // cap the scheduler applies. With no DSL config loaded the fallback is
        // the default of 10.
        let (state, _store) = make_store_state();

        // Fill the queue to the default cap of 10 for one job.
        for i in 0..10 {
            let (status, _) = post_json_status(
                server_router(Arc::clone(&state)),
                "/v1/trigger",
                serde_json::json!({ "job_key": "billing:invoice" }),
            )
            .await;
            assert_eq!(status, 200, "trigger #{i} within the cap must succeed");
        }

        // The 11th trigger for the same job overflows the cap → 429, and
        // nothing extra is enqueued.
        let (status, _) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice" }),
        )
        .await;
        assert_eq!(status, 429, "trigger past the cap must be rejected");
        assert_eq!(
            state
                .runner
                .queue
                .read()
                .await
                .count_for_job("billing:invoice"),
            10,
            "a rejected trigger must not enqueue"
        );

        // The cap is per-job: a different job is unaffected by the first
        // job's full queue.
        let (status, _) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "reports:daily" }),
        )
        .await;
        assert_eq!(
            status, 200,
            "a different job must not be blocked by another job's queue"
        );
    }

    #[tokio::test]
    async fn trigger_overflow_429_carries_retry_after_header() {
        // #312: the per-job overflow 429 must carry a `Retry-After` hint so
        // producers (and the SDKs, via `retryAfterMs`) can back off instead
        // of hammering a full queue.
        let (state, _store) = make_store_state();

        // Fill the queue to the default cap of 10 for one job.
        for _ in 0..10 {
            let (status, _) = post_json_status(
                server_router(Arc::clone(&state)),
                "/v1/trigger",
                serde_json::json!({ "job_key": "billing:invoice" }),
            )
            .await;
            assert_eq!(status, 200);
        }

        // The 11th overflows — inspect the raw response so we can read headers
        // (post_json_status drops them).
        let resp = server_router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/trigger")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "job_key": "billing:invoice" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status().as_u16(), 429);
        assert_eq!(
            resp.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
            Some("1"),
            "overflow 429 must carry a Retry-After backpressure hint"
        );
    }

    #[tokio::test]
    async fn trigger_key_reuse_after_window_creates_new_execution() {
        let (state, store) = make_store_state();

        // A completed execution carrying the key, created well outside the
        // default 10-minute dedup window.
        let old_id = seed_keyed_execution(
            &store,
            "billing:invoice",
            "evt-old",
            ExecutionState::Completed,
            Utc::now() - chrono::Duration::seconds(2 * DEFAULT_TRIGGER_DEDUP_WINDOW_SECS as i64),
        );

        let (status, resp) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": "evt-old" }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(resp["deduplicated"], false);
        assert_ne!(
            resp["execution_id"].as_str().unwrap(),
            old_id.to_string(),
            "a key whose execution completed outside the window must trigger fresh"
        );
    }

    #[tokio::test]
    async fn trigger_key_dedups_against_completed_execution_within_window() {
        let (state, store) = make_store_state();

        // A completed execution carrying the key, created just now — inside
        // the window, so the repeat trigger coalesces even though the
        // execution already finished.
        let done_id = seed_keyed_execution(
            &store,
            "billing:invoice",
            "evt-done",
            ExecutionState::Completed,
            Utc::now(),
        );

        let (status, resp) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": "evt-done" }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(resp["deduplicated"], true);
        assert_eq!(resp["execution_id"].as_str().unwrap(), done_id.to_string());

        let q = state.runner.queue.read().await;
        assert_eq!(q.len(), 0, "dedup hit must not enqueue");
    }

    #[tokio::test]
    async fn trigger_with_oversized_idempotency_key_returns_400() {
        let (state, _store) = make_store_state();

        let oversized = "k".repeat(MAX_IDEMPOTENCY_KEY_CHARS + 1);
        let (status, _) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": oversized }),
        )
        .await;
        assert_eq!(status, 400);

        let q = state.runner.queue.read().await;
        assert_eq!(q.len(), 0, "rejected trigger must not enqueue");
    }

    #[tokio::test]
    async fn trigger_with_empty_idempotency_key_is_treated_as_absent() {
        let (state, _store) = make_store_state();

        let (_, first) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": "" }),
        )
        .await;
        let (_, second) = post_json_status(
            server_router(Arc::clone(&state)),
            "/v1/trigger",
            serde_json::json!({ "job_key": "billing:invoice", "idempotency_key": "" }),
        )
        .await;

        assert_eq!(second["deduplicated"], false);
        assert_ne!(
            first["execution_id"], second["execution_id"],
            "empty keys must not dedup against each other"
        );
    }

    // ─── Per-job concurrency guard (issue #278) ──────────────────────────────

    /// State with a real (in-memory SQLite) store so the guard can count
    /// claimed executions, and a short long-poll timeout for fast tests.
    fn make_guard_state() -> (
        Arc<ServerState>,
        DynStore,
        mpsc::UnboundedReceiver<CompletionEvent>,
    ) {
        use croniq_store::sqlite::SqliteStore;

        let store: DynStore = crate::store::sqlite_store(SqliteStore::in_memory().unwrap());
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();
        let mut state = ServerState::with_auth(runner, tx, None, Some(Arc::clone(&store)));
        {
            let s = Arc::get_mut(&mut state).expect("fresh state has one ref");
            s.long_poll_timeout = Duration::from_millis(50);
        }
        (state, store, rx)
    }

    /// Persist a queued execution and enqueue the matching work item,
    /// stamped with a `__max_concurrent` limit. Returns the execution id.
    async fn seed_guarded_execution(
        state: &Arc<ServerState>,
        store: &DynStore,
        job_key: &str,
        limit: u32,
    ) -> uuid::Uuid {
        use croniq_config::compile::MAX_CONCURRENT_METADATA_KEY;

        let id = uuid::Uuid::new_v4();
        let now = Utc::now();
        let mut metadata = HashMap::new();
        metadata.insert(MAX_CONCURRENT_METADATA_KEY.to_string(), limit.to_string());
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: now,
                scheduled_for: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: None,
                metadata,
                created_at: now,
            })
            .unwrap();
        state.runner.queue.write().await.enqueue(WorkItem {
            execution_id: id.to_string(),
            job_key: job_key.into(),
            fire_at: now,
            scheduled_for: now,
            attempt: 1,
            require: vec![],
            prefer: vec![],
            metadata: serde_json::json!({ MAX_CONCURRENT_METADATA_KEY: limit.to_string() }),
            timeout: "5m".into(),
        });
        id
    }

    #[tokio::test]
    async fn singleton_job_holds_second_execution_until_first_completes() {
        let (state, store, _rx) = make_guard_state();

        // Two queued executions of the same singleton job.
        let mut ids = Vec::new();
        for _ in 0..2 {
            ids.push(seed_guarded_execution(&state, &store, "etl:sync", 1).await);
        }

        // Poll with capacity 2 — only ONE execution may be assigned, even
        // within a single batch.
        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(
            work.len(),
            1,
            "singleton job must yield exactly one assignment, got: {resp}"
        );
        let first_id = work[0]["execution_id"].as_str().unwrap().to_string();

        // The blocked execution stays queued: in-memory item kept, store row
        // untouched.
        assert_eq!(state.runner.queue.read().await.len(), 1);
        let second_id = *ids
            .iter()
            .find(|id| id.to_string() != first_id)
            .expect("the other execution");
        assert_eq!(
            store.get_execution(second_id).unwrap().unwrap().state,
            ExecutionState::Queued,
            "blocked execution must remain queued in the store"
        );

        // Re-poll while the first is still running — the guard must hold.
        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2,
                "inflight": [first_id]
            }),
        )
        .await;
        assert_eq!(
            resp["work"].as_array().unwrap().len(),
            0,
            "no assignment while an execution of the singleton job is in flight"
        );
        assert_eq!(
            state.runner.queue.read().await.len(),
            1,
            "blocked item must stay queued, not be dropped"
        );

        // Complete the first (claimed → completed frees the slot)…
        store
            .complete_execution(
                uuid::Uuid::parse_str(&first_id).unwrap(),
                None,
                ExecutionState::Completed,
                Some(10),
                None,
                None,
                Utc::now(),
            )
            .unwrap();

        // …and the next poll hands out the second execution.
        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(work.len(), 1, "freed slot must release the blocked item");
        assert_eq!(
            work[0]["execution_id"].as_str().unwrap(),
            second_id.to_string()
        );
        assert!(state.runner.queue.read().await.is_empty());
        assert_eq!(
            store.get_execution(second_id).unwrap().unwrap().state,
            ExecutionState::Claimed
        );
    }

    #[tokio::test]
    async fn concurrency_guard_does_not_cross_jobs() {
        let (state, store, _rx) = make_guard_state();

        // Two DIFFERENT singleton jobs with one queued execution each — a
        // capacity-2 poll must assign both (the guard is per job_key).
        seed_guarded_execution(&state, &store, "etl:alpha", 1).await;
        seed_guarded_execution(&state, &store, "etl:beta", 1).await;

        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(
            work.len(),
            2,
            "different jobs must not block each other, got: {resp}"
        );
        assert!(state.runner.queue.read().await.is_empty());
    }

    #[tokio::test]
    async fn max_concurrent_two_allows_two_in_flight_blocks_third() {
        let (state, store, _rx) = make_guard_state();

        for _ in 0..3 {
            seed_guarded_execution(&state, &store, "etl:bulk", 2).await;
        }

        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 5, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(
            work.len(),
            2,
            "max_concurrent 2 must cap the batch at two assignments, got: {resp}"
        );
        assert_eq!(state.runner.queue.read().await.len(), 1);
    }

    #[tokio::test]
    async fn blocked_singleton_does_not_starve_other_jobs_behind_it() {
        let (state, store, _rx) = make_guard_state();

        // An in-flight execution of the guarded job…
        let running = seed_guarded_execution(&state, &store, "etl:guarded", 1).await;
        store
            .claim_execution(running, "other-runner", Utc::now())
            .unwrap();
        state
            .runner
            .queue
            .write()
            .await
            .remove(&running.to_string());

        // …a blocked second execution of it at the FRONT of the queue…
        seed_guarded_execution(&state, &store, "etl:guarded", 1).await;
        // …and an unguarded job queued BEHIND it. Persist the row too:
        // the dispatch path now claims each item in the store and drops
        // any whose row is missing (issue #374), so a store-backed item
        // is what a real poll would ever see here.
        let unguarded_id = uuid::Uuid::new_v4();
        let now = Utc::now();
        store
            .create_execution(&Execution {
                id: unguarded_id,
                job_key: "etl:free".into(),
                fire_at: now,
                scheduled_for: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: None,
                metadata: HashMap::new(),
                created_at: now,
            })
            .unwrap();
        state.runner.queue.write().await.enqueue(WorkItem {
            execution_id: unguarded_id.to_string(),
            job_key: "etl:free".into(),
            fire_at: now,
            scheduled_for: now,
            attempt: 1,
            require: vec![],
            prefer: vec![],
            metadata: serde_json::json!({}),
            timeout: "5m".into(),
        });

        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(
            work.len(),
            1,
            "unguarded job must be dispatched, got: {resp}"
        );
        assert_eq!(
            work[0]["execution_id"].as_str().unwrap(),
            unguarded_id.to_string(),
            "the item behind the blocked singleton must be picked"
        );
        // The blocked singleton item is still queued at the front.
        assert_eq!(state.runner.queue.read().await.len(), 1);
        assert_eq!(
            state.runner.queue.read().await.peek().unwrap().job_key,
            "etl:guarded"
        );
    }

    #[tokio::test]
    async fn trigger_inherits_dsl_max_concurrent_metadata() {
        // POST /v1/trigger inherits the DSL job's compiled metadata, so a
        // triggered execution of a `singleton` job carries __max_concurrent
        // and is subject to the same claim-time guard.
        use crate::loader::load_str;
        use croniq_config::compile::MAX_CONCURRENT_METADATA_KEY;

        let dsl = r#"
            job test:guarded {
                every 1 hour
                singleton
            }
        "#;
        let loaded = load_str(dsl).unwrap();
        let jobs = loaded.runtime.jobs;
        assert_eq!(
            jobs[0]
                .metadata
                .get(MAX_CONCURRENT_METADATA_KEY)
                .map(String::as_str),
            Some("1"),
            "compile should stamp __max_concurrent: {:?}",
            jobs[0].metadata
        );

        let (mut state, _store, _rx) = make_guard_state();
        {
            let s = Arc::get_mut(&mut state).expect("fresh state has one ref");
            s.dsl_jobs = Some(Arc::new(tokio::sync::RwLock::new(jobs)));
        }
        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/trigger",
            serde_json::json!({
                "job_key": "test:guarded",
                "metadata": {},
                "require": [],
                "prefer": []
            }),
        )
        .await;
        assert!(resp["execution_id"].is_string(), "trigger failed: {resp}");

        let q = state.runner.queue.read().await;
        let items = q.peek_n(1);
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0]
                .metadata
                .get(MAX_CONCURRENT_METADATA_KEY)
                .and_then(|v| v.as_str()),
            Some("1"),
            "__max_concurrent must be present in the triggered WorkItem metadata"
        );
    }

    #[tokio::test]
    async fn wedged_singleton_frees_after_stale_claim_reaper() {
        // Issue #374: a single orphaned `claimed` row saturates a singleton's
        // concurrency slot forever — no poll ever gets work for that job.
        // The stale-claim reaper must free the slot and re-enqueue the
        // orphan so the backlog drains.
        use crate::loader::load_str;
        use croniq_config::compile::MAX_CONCURRENT_METADATA_KEY;

        let jobs = load_str(
            r#"
            job etl:sync {
                every 1 hour
                singleton
                timeout 10m
            }
            "#,
        )
        .unwrap()
        .runtime
        .jobs;

        let (state, store, _rx) = make_guard_state();

        // Orphaned claim: grabbed an hour ago by a runner session that no
        // longer exists (not registered, not polling).
        let orphan_id = uuid::Uuid::new_v4();
        let stale = Utc::now() - chrono::Duration::hours(1);
        let mut metadata = HashMap::new();
        metadata.insert(MAX_CONCURRENT_METADATA_KEY.to_string(), "1".to_string());
        store
            .create_execution(&Execution {
                id: orphan_id,
                job_key: "etl:sync".into(),
                fire_at: stale,
                scheduled_for: stale,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: None,
                metadata,
                created_at: stale,
            })
            .unwrap();
        store
            .claim_execution(orphan_id, "app-runner", stale)
            .unwrap();

        // A fresh queued execution of the same job, blocked by the orphan.
        let queued_id = seed_guarded_execution(&state, &store, "etl:sync", 1).await;

        // Reproduce the wedge: the orphaned claim occupies the only slot.
        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        assert_eq!(
            resp["work"].as_array().unwrap().len(),
            0,
            "wedged singleton must yield no work: {resp}"
        );

        // One watchdog sweep reaps the stale claim and re-enqueues it.
        let watchdog =
            crate::watchdog::WatchdogLoop::new(jobs, Arc::clone(&store), Arc::clone(&state.runner));
        let result = watchdog.sweep(Utc::now()).await;
        assert_eq!(result.stale_claims, vec![orphan_id]);
        assert_eq!(
            store.get_execution(orphan_id).unwrap().unwrap().state,
            ExecutionState::Queued
        );

        // Slot freed: the next poll gets exactly one assignment — both the
        // requeued orphan and the blocked item are queued now, but the
        // singleton guard admits one at a time.
        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(
            work.len(),
            1,
            "freed singleton must yield exactly one assignment: {resp}"
        );
        let first_id: uuid::Uuid = work[0]["execution_id"].as_str().unwrap().parse().unwrap();

        // Completing it drains the rest of the backlog on the next poll.
        store
            .complete_execution(
                first_id,
                None,
                ExecutionState::Completed,
                Some(5),
                None,
                None,
                Utc::now(),
            )
            .unwrap();
        let app = server_router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 2, "inflight": []
            }),
        )
        .await;
        let work = resp["work"].as_array().unwrap();
        assert_eq!(work.len(), 1, "backlog must drain after completion: {resp}");
        let second_id: uuid::Uuid = work[0]["execution_id"].as_str().unwrap().parse().unwrap();
        assert_ne!(first_id, second_id);
        for id in [first_id, second_id] {
            assert!(
                id == orphan_id || id == queued_id,
                "assignments must come from the seeded executions"
            );
        }
    }

    // ─── Takeover audit + identity-flapping detection (issue #374 f-up) ─────

    /// Poll `runner_id` as `instance_id` with `max_inflight: 0` so the
    /// handler returns immediately (capacity 0 skips the long poll — the
    /// store-backed test state uses the 30 s production timeout).
    async fn poll_as(state: &Arc<ServerState>, instance_id: &str) -> u16 {
        let (status, _) = post_json_status(
            server_router(Arc::clone(state)),
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": [],
                "max_inflight": 0,
                "inflight": [],
                "instance_id": instance_id
            }),
        )
        .await;
        status
    }

    fn audit_actions(
        store: &crate::store::DynStore,
        action: &str,
    ) -> Vec<croniq_store::models::AuditEvent> {
        store
            .audit_list(&croniq_store::models::AuditFilter {
                action: Some(action.into()),
                ..Default::default()
            })
            .unwrap()
    }

    #[tokio::test]
    async fn takeover_records_audit_event() {
        let (state, store) = make_store_state();

        assert_eq!(poll_as(&state, "iid-A").await, 200);
        assert_eq!(poll_as(&state, "iid-B").await, 200);

        let events = audit_actions(&store, "runner.takeover");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].actor_type, "system");
        assert_eq!(events[0].target_type, "runner");
        assert_eq!(events[0].target_id.as_deref(), Some("r1"));

        // A single takeover is an ordinary restart — no flapping event.
        assert!(audit_actions(&store, "runner.identity_flapping").is_empty());
    }

    #[tokio::test]
    async fn repeated_takeovers_record_flapping_event_once() {
        let (state, store) = make_store_state();

        // iid-A registers; each later instance takes the identity over —
        // four takeovers back-to-back, as in a restart-policy ping-pong.
        for iid in ["iid-A", "iid-B", "iid-C", "iid-D", "iid-E"] {
            assert_eq!(poll_as(&state, iid).await, 200);
        }

        assert_eq!(audit_actions(&store, "runner.takeover").len(), 4);
        // Threshold (3 takeovers / 10 min) crossed at iid-D; iid-E's
        // takeover falls in the throttle window and must not re-fire.
        assert_eq!(audit_actions(&store, "runner.identity_flapping").len(), 1);
    }

    // ─── Runner identity binding (work protocol ownership fence) ───────────
    //
    // Every work handler used to take the acting `runner_id` from the request
    // body and check only the caller's scope, so any credential with a
    // `work:*` scope could act as any runner. These tests pin the fence: one
    // runner's credential must not be able to hijack, complete, log to, or
    // keep alive another runner's work. See `api::runner_identity`.

    /// Store-backed state *with* auth configured — binding is deliberately
    /// inert without auth (all callers share one anonymous identity) or
    /// without a store (nowhere to record the binding), so both are needed
    /// for the fence to engage.
    fn make_bound_state() -> (
        Arc<ServerState>,
        crate::store::DynStore,
        mpsc::UnboundedReceiver<CompletionEvent>,
    ) {
        let store =
            crate::store::sqlite_store(croniq_store::sqlite::SqliteStore::in_memory().unwrap());
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();
        let jwt_config = JwtConfig {
            secret: "runner-identity-test-secret".to_string(),
            ..Default::default()
        };
        let mut state =
            ServerState::with_auth(runner, tx, Some(jwt_config), Some(Arc::clone(&store)));
        {
            let s = Arc::get_mut(&mut state).expect("fresh state has one ref");
            s.long_poll_timeout = Duration::from_millis(50);
        }
        (state, store, rx)
    }

    /// Mint an API-key-shaped token for `client_id` carrying every `work:*`
    /// scope plus `runners:write`. Two different `client_id`s model two
    /// runners holding their own credentials — the multi-runner deployment
    /// the pull protocol is built for.
    fn runner_token(state: &Arc<ServerState>, client_id: &str) -> String {
        let jwt_config = state.jwt_config.as_ref().unwrap();
        croniq_auth::jwt::issue_token_pair(
            jwt_config,
            &format!("{client_id}-key"),
            client_id,
            croniq_auth::CallerType::ApiKey,
            None,
            None,
            croniq_auth::AuthMethod::ApiKey,
            &[
                "work:poll".into(),
                "work:ack".into(),
                "work:renew".into(),
                "work:events".into(),
                "runners:write".into(),
            ],
        )
        .unwrap()
        .access_token
    }

    async fn post_as(
        app: Router,
        uri: &str,
        token: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// `max_inflight: 0` keeps the handler off the long-poll path so the call
    /// returns immediately; the registration/binding side-effects still run.
    fn poll_body(runner_id: &str, instance_id: &str) -> serde_json::Value {
        serde_json::json!({
            "runner_id": runner_id,
            "capabilities": [],
            "max_inflight": 0,
            "inflight": [],
            "instance_id": instance_id,
        })
    }

    /// Seed an execution already claimed by `runner_id`, as a dispatch would
    /// leave it.
    fn seed_claimed_execution(
        store: &crate::store::DynStore,
        job_key: &str,
        runner_id: &str,
    ) -> uuid::Uuid {
        let now = Utc::now();
        let id = uuid::Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: now,
                scheduled_for: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: None,
                metadata: HashMap::new(),
                created_at: now,
            })
            .unwrap();
        store.claim_execution(id, runner_id, now).unwrap();
        id
    }

    /// (a) A foreign credential must not poll under someone else's
    /// `runner_id`. The damage is not the empty poll response — it is the
    /// takeover: registering a new `instance_id` under an existing
    /// `runner_id` requeues the incumbent's in-flight executions (so they run
    /// twice) and fences the real runner out with 409 on its next poll.
    #[tokio::test]
    async fn foreign_credential_cannot_poll_under_another_runner_id() {
        let (state, store, _rx) = make_bound_state();
        let victim = runner_token(&state, "client-a");
        let attacker = runner_token(&state, "client-b");

        // client-a claims `worker-1` by polling first, and holds work.
        assert_eq!(
            post_as(
                server_router(Arc::clone(&state)),
                "/v1/poll",
                &victim,
                poll_body("worker-1", "iid-victim"),
            )
            .await
            .0,
            200
        );
        let exec = seed_claimed_execution(&store, "billing:invoice", "worker-1");

        // client-b polls as `worker-1` with a fresh instance_id.
        let (status, _) = post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &attacker,
            poll_body("worker-1", "iid-attacker"),
        )
        .await;
        assert_eq!(status, 403, "foreign runner_id poll must be refused");

        // No takeover happened: the execution is untouched...
        let loaded = store.get_execution(exec).unwrap().unwrap();
        assert_eq!(loaded.state, ExecutionState::Claimed);
        assert_eq!(loaded.runner_id.as_deref(), Some("worker-1"));
        assert!(
            audit_actions(&store, "runner.takeover").is_empty(),
            "the refused poll must not reach the takeover path"
        );

        // ...and the real runner is not fenced out.
        assert_eq!(
            post_as(
                server_router(Arc::clone(&state)),
                "/v1/poll",
                &victim,
                poll_body("worker-1", "iid-victim"),
            )
            .await
            .0,
            200,
            "the legitimate runner must keep polling normally"
        );
    }

    /// (b) A foreign credential must not complete another runner's execution.
    /// The store CAS fences on `runner_id`, but the value used to come from
    /// the request body — so naming the victim's runner made the CAS match and
    /// forced the execution terminal (suppressing its retry, or fabricating a
    /// failure).
    #[tokio::test]
    async fn foreign_credential_cannot_complete_another_runners_execution() {
        let (state, store, mut rx) = make_bound_state();
        let victim = runner_token(&state, "client-a");
        let attacker = runner_token(&state, "client-b");

        post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &victim,
            poll_body("worker-1", "iid-victim"),
        )
        .await;
        let exec = seed_claimed_execution(&store, "billing:invoice", "worker-1");

        let (status, _) = post_as(
            server_router(Arc::clone(&state)),
            "/v1/complete",
            &attacker,
            serde_json::json!({
                "runner_id": "worker-1",
                "execution_id": exec.to_string(),
                "status": "failure",
                "error": "forged",
                "duration_ms": 1,
            }),
        )
        .await;
        assert_eq!(status, 403);

        // Still claimed, and no completion event was forwarded to the
        // processor (which is what would apply the terminal state).
        let loaded = store.get_execution(exec).unwrap().unwrap();
        assert_eq!(loaded.state, ExecutionState::Claimed);
        assert_eq!(loaded.error, None);
        assert!(rx.try_recv().is_err(), "no completion must be forwarded");
    }

    /// (c) A foreign credential must not append log events to another
    /// runner's execution. This endpoint is addressed by execution id and had
    /// no ownership check at all, so `work:events` plus an execution id was
    /// enough to inject or forge log lines.
    #[tokio::test]
    async fn foreign_credential_cannot_write_events_to_another_runners_execution() {
        let (state, store, _rx) = make_bound_state();
        let victim = runner_token(&state, "client-a");
        let attacker = runner_token(&state, "client-b");

        post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &victim,
            poll_body("worker-1", "iid-victim"),
        )
        .await;
        let exec = seed_claimed_execution(&store, "billing:invoice", "worker-1");

        let (status, _) = post_as(
            server_router(Arc::clone(&state)),
            &format!("/v1/work/{exec}/events"),
            &attacker,
            serde_json::json!([{ "level": "error", "message": "forged log line" }]),
        )
        .await;
        assert_eq!(status, 403);
        assert!(
            store.read_logs(exec, 100).unwrap().is_empty(),
            "no log entry may be written by a foreign credential"
        );

        // The owning runner's own events still land.
        let (status, body) = post_as(
            server_router(Arc::clone(&state)),
            &format!("/v1/work/{exec}/events"),
            &victim,
            serde_json::json!([{ "level": "info", "message": "real log line" }]),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["accepted"], 1);
        assert_eq!(store.read_logs(exec, 100).unwrap().len(), 1);
    }

    /// (d) A foreign credential must not renew another runner's lease —
    /// keeping a dead runner's lease alive suppresses the watchdog requeue of
    /// its abandoned executions.
    #[tokio::test]
    async fn foreign_credential_cannot_renew_another_runners_lease() {
        let (state, _store, _rx) = make_bound_state();
        let victim = runner_token(&state, "client-a");
        let attacker = runner_token(&state, "client-b");

        post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &victim,
            poll_body("worker-1", "iid-victim"),
        )
        .await;
        let exec = uuid::Uuid::new_v4();

        let (status, body) = post_as(
            server_router(Arc::clone(&state)),
            "/v1/work/renew",
            &attacker,
            serde_json::json!({ "runner_id": "worker-1", "execution_id": exec.to_string() }),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(body["renewed"], false);

        // The owner can still renew its own lease.
        let (status, body) = post_as(
            server_router(Arc::clone(&state)),
            "/v1/work/renew",
            &victim,
            serde_json::json!({ "runner_id": "worker-1", "execution_id": exec.to_string() }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["renewed"], true);
    }

    /// One credential shared by many runners — the pre-upgrade deployment
    /// shape the binding must not break — keeps working: every `runner_id`
    /// binds to the same owner, so every check matches. Restarts under a
    /// fresh `instance_id` (the ordinary redeploy) still take over.
    #[tokio::test]
    async fn one_credential_may_own_many_runner_ids() {
        let (state, store, _rx) = make_bound_state();
        let shared = runner_token(&state, "client-a");

        for runner_id in ["worker-1", "worker-2", "worker-3"] {
            let (status, _) = post_as(
                server_router(Arc::clone(&state)),
                "/v1/poll",
                &shared,
                poll_body(runner_id, "iid-1"),
            )
            .await;
            assert_eq!(status, 200);
            assert_eq!(
                store.runner_identity_owner(runner_id).unwrap().as_deref(),
                Some("client-a")
            );
        }

        // A redeploy of worker-1 under the same credential takes its own
        // identity over, exactly as before the binding existed.
        let (status, _) = post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &shared,
            poll_body("worker-1", "iid-2"),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(audit_actions(&store, "runner.takeover").len(), 1);
    }

    /// Deregistering a runner releases its binding, which is how an operator
    /// hands a `runner_id` to a different credential.
    #[tokio::test]
    async fn deregistering_a_runner_frees_its_identity() {
        let (state, store, _rx) = make_bound_state();
        let first = runner_token(&state, "client-a");
        let second = runner_token(&state, "client-b");

        post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &first,
            poll_body("worker-1", "iid-1"),
        )
        .await;
        assert_eq!(
            status_of(
                server_router(Arc::clone(&state)),
                "DELETE",
                "/v1/runners/worker-1",
                None,
                Some(&first),
            )
            .await,
            204
        );
        assert_eq!(store.runner_identity_owner("worker-1").unwrap(), None);

        let (status, _) = post_as(
            server_router(Arc::clone(&state)),
            "/v1/poll",
            &second,
            poll_body("worker-1", "iid-2"),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            store.runner_identity_owner("worker-1").unwrap().as_deref(),
            Some("client-b")
        );
    }

    /// The escape hatch (`pull_api { runner_identity_binding "off" }`)
    /// restores the previous behaviour for deployments that need it.
    #[tokio::test]
    async fn binding_off_restores_body_supplied_runner_ids() {
        let (mut state, store, _rx) = make_bound_state();
        Arc::get_mut(&mut state).unwrap().runner_identity_binding = false;
        let first = runner_token(&state, "client-a");
        let second = runner_token(&state, "client-b");

        for token in [&first, &second] {
            let (status, _) = post_as(
                server_router(Arc::clone(&state)),
                "/v1/poll",
                token,
                poll_body("worker-1", "iid-1"),
            )
            .await;
            assert_eq!(status, 200);
        }
        // Nothing was recorded, so turning binding back on re-binds from
        // scratch rather than locking anyone out.
        assert_eq!(store.runner_identity_owner("worker-1").unwrap(), None);
    }
}
