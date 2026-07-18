//! croniq-server binary: the main server process.
//!
//! Usage:
//! ```sh
//! croniq-server --config Croniqfile --listen :4000 --data-dir ./.data
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use croniq_runner::AppState;
use croniq_server::{
    CompletionProcessor, SchedulerLoop, WatchdogLoop,
    api::{ServerState, server_router},
    loader::{load_file, restore_queued_executions, restore_trigger_states},
    reload,
    store::{DynStore, sqlite_store},
    telemetry,
};
use croniq_store::sqlite::SqliteStore;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(
    name = "croniq-server",
    about = "Croniq distributed job scheduler",
    long_about = "Croniq distributed job scheduler.\n\
\n\
Logging is controlled via the RUST_LOG environment variable using the\n\
tracing-subscriber EnvFilter syntax. Examples:\n\
\n\
    RUST_LOG=info                       # default in Docker / docker-compose\n\
    RUST_LOG=debug                      # verbose, includes per-tick logs\n\
    RUST_LOG=info,croniq_scheduler=debug  # mixed: info globally, debug for scheduler\n\
    RUST_LOG=warn                       # production-quiet\n\
\n\
Unset RUST_LOG behaves as `info`."
)]
struct Cli {
    /// Path to the Croniqfile configuration.
    #[arg(short, long, default_value = "Croniqfile")]
    config: PathBuf,

    /// Address and port to listen on.
    #[arg(short, long, default_value = ":4000")]
    listen: String,

    /// Directory for persistent data (SQLite database).
    /// Defaults to `$CRONIQ_DATA_DIR` when set, otherwise `./.data`. The
    /// Docker entrypoint relies on the env-var path for first-run init, so
    /// the server resolves it the same way to keep both ends in sync.
    #[arg(short, long, default_value = "./.data", env = "CRONIQ_DATA_DIR")]
    data_dir: PathBuf,

    /// Address and port for the Prometheus metrics endpoint (e.g. ":9900").
    /// If not set, metrics are not exposed.
    #[arg(long)]
    metrics: Option<String>,

    /// Watch the Croniqfile for changes and hot-reload on modification.
    #[arg(long)]
    watch: bool,

    /// Directory containing the UI static files to serve.
    /// If set, serves files at / and falls back to index.html for SPA routing.
    #[arg(long)]
    ui_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

/// Optional subcommands. With none, `croniq-server` runs the server — the
/// default and overwhelmingly common case.
#[derive(clap::Subcommand)]
enum Command {
    /// Check the resolved configuration for missing / risky settings, print a
    /// report, and exit (non-zero if any critical finding). Does not start the
    /// server or bind any port.
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    let (telemetry_guard, console_hub) = telemetry::init()?;

    let cli = Cli::parse();

    tracing::info!(config = %cli.config.display(), "loading Croniqfile");
    let mut loaded = load_file(&cli.config)
        .with_context(|| format!("failed to load {}", cli.config.display()))?;

    let job_count = loaded.runtime.jobs.len();
    let active = loaded.triggers.len();
    // Jobs paused because a referenced calendar did not resolve (issue #361).
    // The per-job ERROR was already logged by the loader; surface the count in
    // the summary so a fail-closed startup does not read as fully healthy.
    let calendar_faults = loaded.calendar_faults.len();
    tracing::info!(
        jobs = job_count,
        triggers = active,
        calendar_faults,
        "configuration loaded"
    );

    // `croniq-server doctor`: report config health and exit without binding
    // ports, opening the DB, or starting any task.
    if matches!(cli.command, Some(Command::Doctor)) {
        return run_doctor(&loaded.runtime);
    }

    // Resolve UI sign-in method gates (issue #138). DSL beats env beats default.
    // The both-disabled check must run before we bind the HTTP listener so the
    // operator sees a clear startup error instead of a quiet lockout.
    let password_login_enabled = resolve_password_login_enabled(&loaded.runtime);
    if !password_login_enabled
        && croniq_server::oidc::config_from_dsl_and_env(loaded.runtime.oidc.as_ref()).is_err()
    {
        anyhow::bail!(
            "refusing to start: password login is disabled but no working OIDC config was found.\n\
             Either re-enable password login in the Croniqfile (auth {{ password {{ enabled true }} }})\n\
             or configure OIDC via CRONIQ_OIDC_ISSUER / CRONIQ_OIDC_CLIENT_ID / \
             CRONIQ_OIDC_CLIENT_SECRET / CRONIQ_OIDC_REDIRECT_URL."
        );
    }
    if !password_login_enabled {
        tracing::info!("password login disabled by configuration — only OIDC will accept logins");
    }

    // Enforced 2FA (auth { totp { required true } } / CRONIQ_REQUIRE_TOTP).
    // Non-enrolled users are walked through inline enrolment on next sign-in
    // rather than locked out; surfaced loudly at boot anyway so operators know
    // the gate is active and remember the recovery path if the worst happens.
    let require_totp = resolve_require_totp(&loaded.runtime);
    if require_totp {
        tracing::warn!(
            "enforced 2FA is ON — every password login must present a TOTP or recovery \
             code. Users without a confirmed secret are guided through inline enrolment \
             on next sign-in (not locked out). If you've lost both the authenticator and \
             all recovery codes, set auth {{ totp {{ required false }} }} (or \
             CRONIQ_REQUIRE_TOTP=false), re-enrol, then re-enable."
        );
    }

    // Open (or create) the persistence store. The backend is chosen from
    // `server { db … }` (the CRONIQ_DB env var overrides it): `sqlite`
    // (default) opens the embedded DB under --data-dir, while a `postgres://…`
    // DSN connects to PostgreSQL (requires a build with `--features postgres`).
    // An unrecognised value — or a Postgres DSN on a SQLite-only build — is a
    // hard boot error, never a silent fall-back to SQLite.
    let store: DynStore = open_store(&loaded.runtime.server.db, &cli.data_dir)?;

    // Reconcile CRONIQ_INIT_API_KEY against the stored 'default' client.
    // On an existing data dir, init has already run, so the env var was
    // previously silently ignored. We now always log whether it matches,
    // and rotate when CRONIQ_INIT_API_KEY_RECONCILE=1 is set. See #217.
    {
        let init_api_key = croniq_server::env_secret::env_or_file("CRONIQ_INIT_API_KEY");
        let inputs = croniq_server::init_api_key::ReconcileInputs::from_env_borrowed(&init_api_key);
        croniq_server::init_api_key::reconcile(&*store, inputs)
            .context("failed to reconcile CRONIQ_INIT_API_KEY against stored API client")?;
    }

    // Restore persisted trigger states (once-jobs, next_fire_at) from the DB.
    // Must happen before the scheduler loop starts.
    restore_trigger_states(&mut loaded.triggers, &*store, chrono::Utc::now());
    tracing::info!("trigger states restored from database");

    // Shared runner state (registry + queue) with lease TTL from config
    let lease_ttl_secs = match loaded.runtime.pull_api.as_ref() {
        Some(p) => parse_duration_secs(&p.lease_ttl).map_err(|e| {
            anyhow::anyhow!(
                "invalid pull_api.lease_ttl in {}: {e}",
                cli.config.display()
            )
        })?,
        None => 120,
    };
    let runner_state = AppState::with_lease_ttl(lease_ttl_secs);

    // Dedup window for POST /v1/trigger idempotency keys (issue #279),
    // from `pull_api { trigger_dedup_window … }`. Same resolution pattern
    // as lease_ttl above; falls back to the 10-minute default when no
    // pull_api block is present.
    let trigger_dedup_window_secs = match loaded.runtime.pull_api.as_ref() {
        Some(p) => parse_duration_secs(&p.trigger_dedup_window).map_err(|e| {
            anyhow::anyhow!(
                "invalid pull_api.trigger_dedup_window in {}: {e}",
                cli.config.display()
            )
        })?,
        None => croniq_server::api::DEFAULT_TRIGGER_DEDUP_WINDOW_SECS,
    };

    // Restore queued executions from DB into the in-memory work queue.
    // This ensures executions survive server restarts.
    let restored = restore_queued_executions(&*store, &loaded.runtime.jobs, &runner_state).await;
    tracing::info!(restored, "queued executions restored from database");

    // Completion channel: HTTP complete → processor task
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();

    // Server state wrapping runner + completion channel + JWT auth.
    // JWT secret resolution: CRONIQ_JWT_SECRET env > $DATA_DIR/jwt.secret
    // (auto-created on first boot). Shared with the CLI via
    // `croniq_auth::jwt_secret::ensure` so CLI-side TOTP encryption and
    // server-side decryption always agree on the same secret.
    let jwt_secret = croniq_auth::jwt_secret::ensure(&cli.data_dir).unwrap_or_else(|e| {
        tracing::error!(error = %e, "could not resolve JWT secret");
        std::process::exit(1);
    });
    let jwt_config = Some(croniq_auth::jwt::JwtConfig {
        secret: jwt_secret,
        ..Default::default()
    });
    // Scheduler command channel for live job registration via API
    let (scheduler_cmd_tx, mut scheduler_cmd_rx) =
        mpsc::unbounded_channel::<croniq_server::scheduler::SchedulerCommand>();

    // Apply DSL adoptions on startup: anything in dsl_adoptions is owned by
    // the API store, so the in-memory DSL view must drop it. Without this,
    // an adopted resource would resurface every time the server restarts.
    let adopted_jobs: std::collections::HashSet<String> = store
        .list_adoptions("job")
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.resource_key)
        .collect();
    let adopted_calendars: std::collections::HashSet<String> = store
        .list_adoptions("calendar")
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.resource_key)
        .collect();

    let dsl_jobs_initial: Vec<_> = loaded
        .runtime
        .jobs
        .iter()
        .filter(|j| !adopted_jobs.contains(&j.key))
        .cloned()
        .collect();
    let dsl_calendars_initial: Vec<_> = loaded
        .runtime
        .calendars
        .iter()
        .filter(|c| !adopted_calendars.contains(&c.name))
        .cloned()
        .collect();
    if !adopted_jobs.is_empty() || !adopted_calendars.is_empty() {
        tracing::info!(
            jobs = adopted_jobs.len(),
            calendars = adopted_calendars.len(),
            "skipping DSL definitions superseded by API adoptions"
        );
    }

    // Shared snapshots of DSL jobs and calendars — kept in sync by the
    // scheduler task on Croniqfile reload so the REST API can union DSL
    // entries into `/v1/jobs`, `/v1/schedules`, and `/v1/calendars`.
    let dsl_jobs_shared = Arc::new(tokio::sync::RwLock::new(dsl_jobs_initial.clone()));
    let dsl_calendars_shared = Arc::new(tokio::sync::RwLock::new(dsl_calendars_initial.clone()));

    let config_path_abs = cli
        .config
        .canonicalize()
        .unwrap_or_else(|_| cli.config.clone());

    // Scheduler liveness signal (issue #248). Shared between the scheduler
    // task (writer) and the `/metrics` endpoint (reader).
    let scheduler_heartbeat = Arc::new(croniq_server::scheduler::SchedulerHeartbeat::default());

    let mut server_state = ServerState::with_auth(
        Arc::clone(&runner_state),
        completion_tx,
        jwt_config,
        Some(Arc::clone(&store)),
    );
    // Inject scheduler_tx, dsl_jobs, config_path, password-login
    // flag, the public app base URL, the EmailSender, and the merged
    // alerts config into the shared state. The EmailSender is resolved
    // once here so both
    // the user-management endpoints (invitations / password-reset)
    // and the alerts evaluator share the same SMTP transport —
    // operators only configure it once. Must happen BEFORE the
    // MCP/metrics tasks take their own Arc::clone of `server_state`
    // — Arc::get_mut requires a unique strong-ref.
    {
        let s = Arc::get_mut(&mut server_state).unwrap();
        s.scheduler_tx = Some(scheduler_cmd_tx);
        s.dsl_jobs = Some(Arc::clone(&dsl_jobs_shared));
        s.dsl_calendars = Some(Arc::clone(&dsl_calendars_shared));
        s.policy_dsl_adopt_on_mutate.store(
            loaded.runtime.policy.dsl_adopt_on_mutate,
            std::sync::atomic::Ordering::Relaxed,
        );
        // Surface jobs paused by an unresolved calendar reference (issue #361)
        // so `GET /v1/jobs/states` and `/metrics` can report them.
        *s.config_faults.write().unwrap() = std::mem::take(&mut loaded.calendar_faults);
        s.config_path = Some(config_path_abs.clone());
        s.password_login_enabled = password_login_enabled;
        s.app_base_url = resolve_app_base_url(loaded.runtime.server.app_url.as_deref());
        s.require_totp = require_totp;
        s.email_sender = croniq_server::email::build_from_dsl_and_env(loaded.runtime.smtp.as_ref());
        // Issue #140 PR-5: surface the effective alerts config
        // (after CRONIQ_ON_FAILURE_CMD synthesis) so the read-only
        // `GET /v1/alerts/config` endpoint can serve it.
        s.alerts = croniq_server::alerts::merge_legacy_env_hook(loaded.runtime.alerts.clone());
        // Issue #141: wire the in-memory tracing fan-out into ServerState
        // so the Live Console SSE endpoint can subscribe.
        s.console_hub = Some(Arc::clone(&console_hub));
        // Issue #248: expose the scheduler liveness signal via /metrics.
        s.scheduler_heartbeat = Some(Arc::clone(&scheduler_heartbeat));
        // Issue #279: dedup window for POST /v1/trigger idempotency keys.
        s.trigger_dedup_window_secs = trigger_dedup_window_secs;
    }
    // Issue #231: prune orphan alert-rule overrides whose DSL rule no
    // longer exists (FK-cascade-by-name). The alerts config is loaded at
    // boot only, so boot is the cascade point.
    {
        let valid: Vec<String> = server_state
            .alerts
            .rules
            .iter()
            .map(|r| r.name.clone())
            .collect();
        match store.prune_alert_rule_overrides(&valid) {
            Ok(pruned) if !pruned.is_empty() => tracing::info!(
                target: "croniq::alerts",
                count = pruned.len(),
                rules = ?pruned,
                "pruned orphan alert-rule overrides (DSL rule removed)"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(
                target: "croniq::alerts",
                error = %e,
                "could not prune orphan alert-rule overrides at boot"
            ),
        }
    }
    if let Some(url) = &server_state.app_base_url {
        tracing::info!(app_base_url = %url, "public base URL pinned (server.app_url / CRONIQ_APP_URL)");
    }
    // Surface configuration health at boot (missing SMTP, unpinned URL, …).
    // The same checks back `croniq-server doctor` and GET /v1/system/diagnostics.
    {
        use croniq_server::diagnostics::{DiagnosticsInput, Severity, run_diagnostics};
        let input = DiagnosticsInput::from_runtime(
            server_state.app_base_url.is_some(),
            server_state.email_sender.delivers(),
            require_totp,
            server_state.store.as_ref(),
        );
        for d in run_diagnostics(&input) {
            let remedy = d.remedy.as_deref().unwrap_or("");
            match d.severity {
                Severity::Critical => {
                    tracing::error!(id = d.id, remedy, "{} — {}", d.title, d.detail)
                }
                Severity::Warning => {
                    tracing::warn!(id = d.id, remedy, "{} — {}", d.title, d.detail)
                }
                Severity::Info => tracing::info!(id = d.id, "{} — {}", d.title, d.detail),
            }
        }
    }
    let reload_counters = Arc::clone(&server_state.reload_counters);

    // ── Reload signalling: file watcher (optional) + SIGHUP ─────────────────
    let (reload_tx, mut reload_rx) = mpsc::unbounded_channel::<std::path::PathBuf>();

    if cli.watch {
        match croniq_server::watcher::watch_config(&config_path_abs) {
            Ok(raw_rx) => {
                let debounce_tx = reload_tx.clone();
                tokio::spawn(croniq_server::watcher::debounced_reload_loop(
                    raw_rx,
                    std::time::Duration::from_millis(500),
                    debounce_tx,
                ));
                tracing::info!(path = %config_path_abs.display(), "watching Croniqfile for changes");
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not start file watcher — hot-reload disabled");
            }
        }
    }

    // SIGHUP → re-read Croniqfile (unix only). Matches the long-standing
    // daemon convention: `kill -HUP <pid>` reloads config without restart.
    #[cfg(unix)]
    {
        let sighup_tx = reload_tx.clone();
        let sighup_path = config_path_abs.clone();
        tokio::spawn(async move {
            let mut signal =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "could not register SIGHUP handler");
                        return;
                    }
                };
            while signal.recv().await.is_some() {
                tracing::info!("SIGHUP received — requesting config reload");
                if sighup_tx.send(sighup_path.clone()).is_err() {
                    break;
                }
            }
        });
    }
    let _ = reload_tx; // keep sender alive even when no watcher/SIGHUP fires (not applicable on non-unix)

    // ── Scheduler task ────────────────────────────────────────────────────────
    let scheduler_store = Arc::clone(&store);
    // Drop adopted DSL keys from the scheduler's view too — otherwise the
    // adopted job would still fire from the in-memory trigger built by the
    // loader, alongside (or instead of) the API-managed copy.
    let mut jobs: Vec<_> = loaded
        .runtime
        .jobs
        .clone()
        .into_iter()
        .filter(|j| !adopted_jobs.contains(&j.key))
        .collect();
    let mut triggers: std::collections::HashMap<_, _> = loaded
        .triggers
        .into_iter()
        .filter(|(k, _)| !adopted_jobs.contains(k))
        .collect();

    // Reconcile API/runner-registered jobs from DB (not in Croniqfile)
    {
        use croniq_server::loader::{job_config_from_definition, trigger_from_definition};

        let now = chrono::Utc::now();
        if let Ok(api_triggers) = store.list_triggers(None) {
            let mut api_count = 0;
            for def in &api_triggers {
                if def.managed_by == "dsl" || !def.enabled {
                    continue;
                }
                if triggers.contains_key(&def.job_key) {
                    continue;
                } // Croniqfile takes precedence
                if let Some(trigger) = trigger_from_definition(def, now) {
                    let job_config = job_config_from_definition(def, None);
                    jobs.push(job_config);
                    triggers.insert(def.job_key.clone(), trigger);
                    api_count += 1;
                }
            }
            if api_count > 0 {
                tracing::info!(api_count, "API-registered jobs restored from database");
            }
        }
    }

    // Share a snapshot of the triggers map with the API layer so the
    // dashboard forecast endpoint can compute upcoming fires.
    let trigger_snapshot = Arc::new(tokio::sync::RwLock::new(triggers.clone()));
    Arc::get_mut(&mut server_state).unwrap().triggers = Some(Arc::clone(&trigger_snapshot));

    // Capture handles for the MCP HTTP transport. Must happen after the last
    // `Arc::get_mut(&mut server_state)` so the unique-ref invariant holds; the
    // clone here is harmless thereafter. Defaults to enabled when the
    // Croniqfile has no `mcp { ... }` block.
    let mcp_enabled = loaded.runtime.mcp.as_ref().is_none_or(|m| m.enabled);
    let mcp_state = Arc::clone(&server_state);
    let mcp_runner = Arc::clone(&runner_state);
    let mcp_store = Arc::clone(&store);
    let mcp_jobs = dsl_jobs_initial.clone();
    let mcp_triggers = Arc::clone(&trigger_snapshot);

    // Load the persisted maintenance switch into the shared in-memory cache
    // (the store stays the source of truth) so the switch survives restarts,
    // then hand the scheduler a clone so its tick can freeze dispatch.
    if let Ok(m) = store.get_maintenance()
        && let Ok(mut guard) = server_state.maintenance.write()
    {
        *guard = m;
    }

    let mut scheduler_loop = SchedulerLoop::new(
        triggers,
        jobs.clone(),
        scheduler_store,
        Arc::clone(&runner_state),
    );
    scheduler_loop.set_maintenance_handle(Arc::clone(&server_state.maintenance));

    let scheduler_reload_store = Arc::clone(&store);
    let scheduler_reload_snapshot = Arc::clone(&trigger_snapshot);
    let scheduler_reload_dsl = Arc::clone(&dsl_jobs_shared);
    let scheduler_reload_dsl_cals = Arc::clone(&dsl_calendars_shared);
    let scheduler_reload_policy = Arc::clone(&server_state.policy_dsl_adopt_on_mutate);
    let scheduler_reload_faults = Arc::clone(&server_state.config_faults);
    let scheduler_reload_counters = Arc::clone(&reload_counters);

    let scheduler_task_heartbeat = Arc::clone(&scheduler_heartbeat);
    let scheduler_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // A single tick should finish in microseconds; a tick that runs this
        // long means a hung store call or a wedged queue lock (issue #248).
        // Bound it so one stuck tick is logged + skipped instead of wedging
        // the whole loop forever — and leave the heartbeat stale so the
        // liveness metric reflects the stall.
        const TICK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
        // ~5 minutes at the 1 s tick cadence. A positive "scheduler is alive"
        // signal at INFO, independent of whether any job fired.
        const HEARTBEAT_EVERY_TICKS: u64 = 300;
        let mut ticks_since_heartbeat: u64 = 0;
        // Per-job ephemeral dispatch counts since the last heartbeat. Folded
        // into the heartbeat line at `INFO` so ephemeral fires — which only
        // log at `DEBUG` per-fire — leave an observable server-side trace
        // without per-fire spam (issue #275). `BTreeMap` keeps the rendered
        // order stable across heartbeats.
        let mut ephemeral_since_heartbeat: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = chrono::Utc::now();
                    match tokio::time::timeout(TICK_TIMEOUT, scheduler_loop.tick(now)).await {
                        Ok(result) => {
                            scheduler_task_heartbeat.record_tick(now);
                            ticks_since_heartbeat += 1;
                            if !result.fired.is_empty() {
                                tracing::debug!(count = result.fired.len(), "scheduler tick: jobs fired");
                            }
                            for fired in &result.fired {
                                if fired.is_ephemeral {
                                    *ephemeral_since_heartbeat
                                        .entry(fired.job_key.clone())
                                        .or_insert(0) += 1;
                                }
                            }
                            if ticks_since_heartbeat >= HEARTBEAT_EVERY_TICKS {
                                ticks_since_heartbeat = 0;
                                let ephemeral = if ephemeral_since_heartbeat.is_empty() {
                                    "[]".to_string()
                                } else {
                                    let body = ephemeral_since_heartbeat
                                        .iter()
                                        .map(|(key, count)| format!("{key}:{count}"))
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    format!("[{body}]")
                                };
                                tracing::info!(
                                    ticks_total = scheduler_task_heartbeat.ticks_total(),
                                    triggers = scheduler_loop.triggers.len(),
                                    ephemeral = %ephemeral,
                                    "scheduler heartbeat — alive"
                                );
                                ephemeral_since_heartbeat.clear();
                            }
                        }
                        Err(_elapsed) => {
                            // Tick dropped at its timeout (lock guards release on
                            // drop). Deliberately do NOT update the heartbeat, so
                            // a wedged scheduler shows up as a stale
                            // croniq_scheduler_last_tick_timestamp instead of
                            // looking healthy.
                            tracing::error!(
                                timeout_secs = TICK_TIMEOUT.as_secs(),
                                "scheduler tick exceeded timeout and was skipped — a hung store/lock keeps the liveness metric stale"
                            );
                        }
                    }
                }
                Some(path) = reload_rx.recv() => {
                    tracing::info!(path = %path.display(), "Croniqfile reload requested");
                    match reload::build_plan(
                        &path,
                        &scheduler_reload_store,
                        &scheduler_reload_snapshot,
                        &scheduler_reload_dsl,
                    ).await {
                        Ok(plan) => {
                            let diff = plan.diff.clone();
                            reload::apply_plan_direct(
                                plan,
                                &mut scheduler_loop,
                                &scheduler_reload_dsl,
                                &scheduler_reload_dsl_cals,
                                &scheduler_reload_policy,
                                &scheduler_reload_snapshot,
                                &scheduler_reload_faults,
                            ).await;
                            scheduler_reload_counters.inc_success();
                            tracing::info!(
                                added = diff.added.len(),
                                removed = diff.removed.len(),
                                changed = diff.changed.len(),
                                total = diff.total,
                                "config reloaded"
                            );
                        }
                        Err(e) => {
                            scheduler_reload_counters.inc_validation_error();
                            tracing::error!(error = %e, "config reload failed — keeping previous config");
                        }
                    }
                }
                Some(cmd) = scheduler_cmd_rx.recv() => {
                    scheduler_loop.apply_command(cmd);
                }
            }
        }
    });

    // ── Scheduler supervisor (issue #248) ──────────────────────────────────────
    //
    // The scheduler loop above never returns under normal operation, so the
    // JoinHandle completing at all means the task panicked (or was aborted).
    // Previously the handle was dropped (`let _scheduler_task = ...`), so a
    // panicking tick left a silently-dead scheduler while HTTP kept serving —
    // jobs simply stopped firing with no log and no restart. Now we watch the
    // handle and, on unexpected completion, log loudly and exit non-zero so
    // the container's `restart:` policy (or systemd) brings the process — and
    // a fresh scheduler — back.
    tokio::spawn(async move {
        match scheduler_handle.await {
            Ok(()) => tracing::error!(
                "scheduler task exited unexpectedly (loop returned) — aborting process so it restarts"
            ),
            Err(e) if e.is_panic() => tracing::error!(
                error = %e,
                "scheduler task panicked — aborting process so it restarts"
            ),
            Err(e) => tracing::error!(
                error = %e,
                "scheduler task was cancelled — aborting process so it restarts"
            ),
        }
        // Hard-exit (EX_SOFTWARE) so the supervisor/orchestrator restarts us
        // rather than running on with a dead scheduler.
        std::process::exit(70);
    });

    // ── Completion processor task ─────────────────────────────────────────────
    let proc_store = Arc::clone(&store);
    let proc_jobs = jobs;

    // Failure-alert config (issue #140): DSL `alerts {}` block plus the
    // back-compat synthesis from `CRONIQ_ON_FAILURE_CMD` for installs
    // that haven't migrated yet. Throttle map is seeded from the
    // existing delivery log so a server restart doesn't reset
    // suppression windows.
    // alerts_cfg was already resolved at boot (before the MCP task
    // took an Arc::clone of server_state) so the read-only
    // `GET /v1/alerts/config` endpoint can serve it. Reuse the
    // snapshot stored on ServerState rather than re-running
    // merge_legacy_env_hook here — keeps a single source of truth
    // for "what the server thinks the alerts config is".
    let alerts_cfg = server_state.alerts.clone();
    let alert_throttle = croniq_server::alerts::load_throttle_state(&proc_store, &alerts_cfg);
    if !alerts_cfg.rules.is_empty() {
        tracing::info!(
            channels = alerts_cfg.channels.len(),
            rules = alerts_cfg.rules.len(),
            "failure-alert evaluator armed"
        );
    }
    // Watchdog gets its own clones of the config + throttle Arc so
    // the SLA-miss sweep dispatches through the same evaluator
    // pipeline as `job_failed`. Sharing the throttle Arc is what
    // makes `throttle 10m` apply across both trigger types for the
    // same (rule, job_key).
    let alerts_cfg_for_watchdog = alerts_cfg.clone();
    let alert_throttle_for_watchdog = Arc::clone(&alert_throttle);

    let processor = Arc::new(CompletionProcessor::with_alerts(
        proc_jobs,
        proc_store,
        Arc::clone(&runner_state),
        alerts_cfg,
        alert_throttle,
        Arc::clone(&server_state.email_sender),
    ));

    let _completion_task = tokio::spawn(async move {
        while let Some(event) = completion_rx.recv().await {
            let outcome = processor.process(event).await;
            tracing::debug!(?outcome, "completion processed");
        }
    });

    // ── Watchdog task ─────────────────────────────────────────────────────────
    //
    // Watchdog shares the alerts config + throttle map with the
    // completion processor: the SLA-miss sweep (#140 PR-4) uses the
    // same `evaluate_failure` pipeline as `job_failed`, so a rule
    // with `throttle 10m` correctly suppresses both kinds of fires
    // in the same window.
    let watchdog = WatchdogLoop::with_alerts(
        loaded.runtime.jobs.clone(),
        Arc::clone(&store),
        Arc::clone(&runner_state),
        alerts_cfg_for_watchdog,
        alert_throttle_for_watchdog,
        croniq_server::watchdog::empty_sla_fired_set(),
        Arc::clone(&server_state.email_sender),
    );
    let watchdog_store = Arc::clone(&store);

    // Age-based execution retention (issue #344). Parsed once at boot from
    // `server { execution_retention <dur> }`. Invalid or zero ⇒ disabled: a
    // data-deleting knob must never fall back to "delete everything". Per-job
    // `keep_last` caps are read by the watchdog from its DSL job snapshot and
    // need no wiring here.
    let execution_retention: Option<chrono::Duration> = loaded
        .runtime
        .server
        .execution_retention
        .as_deref()
        .and_then(|raw| match croniq_execution::retry::parse_duration(raw) {
            Some(d) if !d.is_zero() => match chrono::Duration::from_std(d) {
                Ok(d) => Some(d),
                Err(_) => {
                    tracing::error!(value = %raw, "server.execution_retention too large; execution pruning disabled");
                    None
                }
            },
            Some(_) => {
                tracing::warn!(
                    "server.execution_retention is zero; ignoring (would delete all run history)"
                );
                None
            }
            None => {
                tracing::error!(value = %raw, "invalid server.execution_retention duration; execution pruning disabled");
                None
            }
        });
    if let Some(dur) = execution_retention {
        tracing::info!(days = dur.num_days(), "execution retention enabled");
    }

    let _watchdog_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let now = chrono::Utc::now();
            let result = watchdog.sweep(now).await;
            if !result.dead_runners.is_empty() {
                tracing::warn!(
                    dead = result.dead_runners.len(),
                    requeued = result.requeued.len(),
                    "watchdog: processed dead runners"
                );
            }
            if !result.sla_missed.is_empty() {
                tracing::warn!(
                    count = result.sla_missed.len(),
                    "watchdog: SLA-miss alerts fired"
                );
            }

            // Reap dead-letter rows whose retention has lapsed. Same cadence
            // as the abandoned-runner sweep — `purge_expired` is a single
            // DELETE so the cost is negligible even on busy deployments.
            match watchdog_store.purge_expired(now) {
                Ok(count) if count > 0 => {
                    tracing::info!(count, "watchdog: purged expired dead-letter rows");
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "watchdog: failed to purge expired dead-letters");
                }
            }

            // Enforce execution retention (issue #344): the global age sweep
            // (`execution_retention`) plus per-job `keep_last` caps. No-op when
            // neither is configured; a large first-time backlog drains over
            // several ticks (bounded batches, see WatchdogLoop::prune_executions).
            let pruned = watchdog.prune_executions(now, execution_retention);
            if pruned.total() > 0 {
                tracing::info!(
                    by_age = pruned.by_age,
                    by_cap = pruned.by_cap,
                    "watchdog: pruned terminal executions"
                );
            }
        }
    });

    // ── Metrics server (from CLI --metrics or observability.metrics in Croniqfile)
    let metrics_listen = cli.metrics.clone().or_else(|| {
        loaded
            .runtime
            .observability
            .as_ref()
            .and_then(|o| o.metrics.as_ref())
            .map(|m| m.listen.clone())
    });
    if let Some(ref metrics_listen) = metrics_listen {
        let metrics_addr: std::net::SocketAddr = metrics_listen
            .trim_start_matches(':')
            .parse::<u16>()
            .map(|p| ([0, 0, 0, 0], p).into())
            .or_else(|_| metrics_listen.parse())
            .with_context(|| format!("invalid metrics address: {metrics_listen}"))?;

        let metrics_app = croniq_server::metrics::metrics_router(Arc::clone(&server_state));
        let metrics_listener = tokio::net::TcpListener::bind(metrics_addr).await?;
        tracing::info!(address = %metrics_addr, "metrics endpoint listening");
        tokio::spawn(async move {
            axum::serve(metrics_listener, metrics_app).await.ok();
        });
    }

    // ── HTTP server ───────────────────────────────────────────────────────────
    let addr: std::net::SocketAddr = cli
        .listen
        .trim_start_matches(':')
        .parse::<u16>()
        .map(|p| ([0, 0, 0, 0], p).into())
        .or_else(|_| cli.listen.parse())
        .with_context(|| format!("invalid listen address: {}", cli.listen))?;

    let mut app = server_router(server_state);

    // Mount the in-process MCP HTTP transport at /mcp. Auth + per-tool
    // admin-scope gating are applied inside `mcp_router`.
    if mcp_enabled {
        // `mcp { allowed_hosts ... }` is additive on top of rmcp's
        // loopback-only default (issue #114). `None` / empty Vec keeps the
        // post-v0.10.1 behaviour where only `localhost` / `127.0.0.1` / `::1`
        // are accepted.
        let extra_allowed_hosts = loaded
            .runtime
            .mcp
            .as_ref()
            .map(|m| m.allowed_hosts.clone())
            .filter(|v| !v.is_empty());
        if let Some(ref hosts) = extra_allowed_hosts {
            tracing::info!(
                hosts = ?hosts,
                "MCP allowed_hosts directive appends entries to rmcp loopback default"
            );
        }
        let mcp_router = croniq_server::mcp::mcp_router(
            mcp_state,
            mcp_runner,
            Some(mcp_store),
            mcp_jobs,
            Some(mcp_triggers),
            extra_allowed_hosts,
        );
        app = app.merge(mcp_router);
        tracing::info!("MCP HTTP transport enabled at /mcp");
    } else {
        tracing::info!("MCP HTTP transport disabled by Croniqfile");
    }

    // Serve UI static files if --ui-dir is set
    if let Some(ref ui_dir) = cli.ui_dir {
        use tower_http::services::{ServeDir, ServeFile};
        let index = ui_dir.join("index.html");
        let serve = ServeDir::new(ui_dir).fallback(ServeFile::new(&index));
        app = app.fallback_service(serve);
        tracing::info!(path = %ui_dir.display(), "serving UI static files");
    }

    tracing::info!(address = %addr, "croniq-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("croniq-server stopped gracefully");

    // Flush in-flight OTLP spans/logs before the process exits.
    // No-op when the `otlp` feature is off or the endpoint env was unset.
    telemetry_guard.shutdown();

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => tracing::info!("received SIGINT"),
            _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
        tracing::info!("received SIGINT");
    }
}

/// Resolve the effective `password_login_enabled` flag for the running server.
///
/// Precedence (highest first): DSL `auth { password { enabled bool } }` →
/// env `CRONIQ_PASSWORD_LOGIN_ENABLED` → default `true`.
///
/// Env-var parsing is conservative: only the explicit falsy set
/// (`false`/`no`/`off`/`0`) disables. Anything else (incl. typos, empty
/// string, garbage) keeps password login on — silently locking everyone
/// out because of `CRONIQ_PASSWORD_LOGIN_ENABLED=disable` would be a bad
/// surprise.
fn resolve_password_login_enabled(rt: &croniq_config::compile::RuntimeConfig) -> bool {
    if let Some(v) = rt.auth.password.enabled {
        return v;
    }
    match std::env::var("CRONIQ_PASSWORD_LOGIN_ENABLED").ok() {
        Some(s) => !matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "false" | "no" | "off" | "0"
        ),
        None => true,
    }
}

/// Resolve the operator-configured public base URL for invite / password-reset
/// / OIDC login links (backs `ServerState::app_base_url`).
///
/// Precedence: DSL `server { app_url "…" }` (passed as `dsl_app_url`) → env
/// `CRONIQ_APP_URL` → `None`. `None` means "not configured", and the link base
/// is then derived per-request from the forwarded / `Host` headers (see
/// `croniq_server::api::resolve_link_base`). Blank values are ignored and
/// surrounding whitespace trimmed; trailing slashes are tolerated (the link
/// builders trim them).
fn resolve_app_base_url(dsl_app_url: Option<&str>) -> Option<String> {
    if let Some(u) = dsl_app_url.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(u.to_string());
    }
    match std::env::var("CRONIQ_APP_URL") {
        Ok(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// `croniq-server doctor` — print a configuration health report and exit.
///
/// Offline preflight: runs the same checks surfaced at boot and via
/// `GET /v1/system/diagnostics`, but from the loaded Croniqfile + env only
/// (no server, DB, or ports). Exits non-zero on any `Critical` finding so it
/// can gate a deploy.
fn run_doctor(rt: &croniq_config::compile::RuntimeConfig) -> Result<()> {
    use croniq_server::diagnostics::{DiagnosticsInput, Severity, run_diagnostics};

    let smtp_feature = croniq_server::email::smtp_feature_compiled();
    let smtp_configured = croniq_server::email::smtp_configured(rt.smtp.as_ref());
    let input = DiagnosticsInput::from_runtime(
        resolve_app_base_url(rt.server.app_url.as_deref()).is_some(),
        // Offline approximation of build_from_dsl_and_env(): a real transport
        // needs the `smtp` feature compiled AND a usable config (URL or host
        // + from) from the Croniqfile smtp{} block or CRONIQ_SMTP_* env.
        smtp_feature && smtp_configured,
        resolve_require_totp(rt),
        None, // no live store offline → the enforced-2FA enrollment check is skipped
    );

    let findings = run_diagnostics(&input);
    if findings.is_empty() {
        println!("[OK] croniq configuration looks healthy — no findings.");
        return Ok(());
    }

    let mut criticals = 0usize;
    for d in &findings {
        let label = match d.severity {
            Severity::Critical => {
                criticals += 1;
                "CRITICAL"
            }
            Severity::Warning => "WARNING",
            Severity::Info => "INFO",
        };
        println!("[{label}] {}", d.title);
        println!("    {}", d.detail);
        if let Some(remedy) = &d.remedy {
            println!("    fix: {remedy}");
        }
        println!();
    }
    println!(
        "{} finding(s) ({} critical). Address the items above to resolve.",
        findings.len(),
        criticals
    );
    if criticals > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Resolve the effective `require_totp` flag (enforced 2FA).
///
/// Precedence (highest first): DSL `auth { totp { required bool } }` → env
/// `CRONIQ_REQUIRE_TOTP` → default `false`.
///
/// Mirror-image of [`resolve_password_login_enabled`]'s conservatism: only
/// the explicit truthy set turns enforcement on. Anything unrecognised
/// (typo, empty, garbage) leaves it OFF — accidentally enabling enforcement
/// would lock out every account that hasn't enrolled yet.
fn resolve_require_totp(rt: &croniq_config::compile::RuntimeConfig) -> bool {
    if let Some(v) = rt.auth.totp.required {
        return v;
    }
    match std::env::var("CRONIQ_REQUIRE_TOTP").ok() {
        Some(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1"
        ),
        None => false,
    }
}

/// The persistence backend selected by `server { db … }` / `CRONIQ_DB`.
///
/// SQLite (the default) is always available. PostgreSQL is available when the
/// binary is built with `--features postgres`: the server drives the
/// synchronous `croniq-store` `PgStore` on a dedicated OS thread (via
/// `croniq_store::pg_actor::PgStoreHandle`) so the driver's internal `block_on`
/// never runs inside croniq-server's `#[tokio::main]` runtime — which would
/// otherwise panic ("Cannot start a runtime from within a runtime"). On a build
/// *without* that feature, a `postgres://…` DSN is rejected at boot rather than
/// silently opening SQLite.
enum DbBackend {
    /// Embedded SQLite under `--data-dir` (the default).
    Sqlite,
    /// A `postgres://…` / `postgresql://…` DSN. Served by
    /// `croniq_store::pg_actor::PgStoreHandle` on `--features postgres` builds;
    /// rejected at boot otherwise.
    Postgres,
}

/// Resolve the effective DB spec. Precedence (highest first): the `CRONIQ_DB`
/// env var → the Croniqfile `server { db … }` value → the default `sqlite`.
/// A blank value at any level falls through to the next.
fn resolve_db_spec(server_db: &str) -> String {
    if let Ok(s) = std::env::var("CRONIQ_DB") {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    let s = server_db.trim();
    if s.is_empty() {
        "sqlite".to_string()
    } else {
        s.to_string()
    }
}

/// Classify a resolved DB spec into a [`DbBackend`], or fail with a clear
/// message. Deliberately strict: an unknown value is an error rather than a
/// silent fall-back to SQLite, so a typo (`postgress://…`), a different engine
/// (`mysql://…`), or a bare path never masquerade as a working config.
fn classify_db_backend(spec: &str) -> Result<DbBackend> {
    let s = spec.trim();
    if s.eq_ignore_ascii_case("sqlite") {
        Ok(DbBackend::Sqlite)
    } else if s.starts_with("postgres://") || s.starts_with("postgresql://") {
        Ok(DbBackend::Postgres)
    } else {
        anyhow::bail!(
            "unrecognised `server {{ db … }}` value {s:?} (or CRONIQ_DB): expected \
             `sqlite` or a `postgres://…` / `postgresql://…` connection string"
        )
    }
}

/// Open the persistence store selected by `server_db` (resolved together with
/// the `CRONIQ_DB` env var). SQLite creates `<data_dir>/croniq.db`. A Postgres
/// DSN connects via [`croniq_store::pg_actor::PgStoreHandle`] on a
/// `--features postgres` build, and is rejected at boot otherwise (see
/// [`DbBackend`]). An unknown backend is always a hard error, never a silent
/// fall-back to SQLite.
fn open_store(server_db: &str, data_dir: &std::path::Path) -> Result<DynStore> {
    let spec = resolve_db_spec(server_db);
    match classify_db_backend(&spec)? {
        DbBackend::Sqlite => {
            std::fs::create_dir_all(data_dir)?;
            let db_path = data_dir.join("croniq.db");
            tracing::info!(backend = "sqlite", path = %db_path.display(), "opening store");
            Ok(sqlite_store(SqliteStore::open(&db_path).with_context(
                || format!("failed to open database at {}", db_path.display()),
            )?))
        }
        DbBackend::Postgres => open_postgres_store(&spec),
    }
}

/// Connect to PostgreSQL. On a `--features postgres` build this drives the
/// synchronous `PgStore` on a dedicated OS thread (so its internal `block_on`
/// never runs inside the async runtime); on a build without the feature it is a
/// hard boot error rather than a silent SQLite fall-back.
#[cfg(feature = "postgres")]
fn open_postgres_store(spec: &str) -> Result<DynStore> {
    tracing::info!(backend = "postgres", "opening store");
    let handle = croniq_store::pg_actor::PgStoreHandle::connect(spec)
        .map_err(|e| anyhow::anyhow!("failed to connect to PostgreSQL: {e}"))?;
    Ok(croniq_server::store::pg_store(handle))
}

#[cfg(not(feature = "postgres"))]
fn open_postgres_store(_spec: &str) -> Result<DynStore> {
    anyhow::bail!(
        "refusing to start: `server {{ db postgres://… }}` (or CRONIQ_DB) selects \
         PostgreSQL, but this croniq-server binary was built without the `postgres` \
         feature. Rebuild with `--features postgres` (the official Docker image and \
         release binaries include it), or use `db sqlite` (embedded, the default)."
    )
}

/// Parse a duration string like `"60s"`, `"2m"`, `"1h"`, or a bare integer
/// (interpreted as seconds) into seconds. Returns an error string on malformed
/// input rather than silently falling back, so that bad config surfaces at boot
/// instead of becoming a 2-minute lease nobody asked for.
fn parse_duration_secs(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let parse = |body: &str, mult: u64, suffix: char| -> Result<u64, String> {
        body.parse::<u64>()
            .map_err(|_| format!("invalid duration {s:?}: cannot parse number before '{suffix}'"))
            .and_then(|v| {
                v.checked_mul(mult)
                    .ok_or_else(|| format!("duration {s:?} overflows u64 seconds"))
            })
    };
    if let Some(n) = s.strip_suffix('s') {
        parse(n, 1, 's')
    } else if let Some(n) = s.strip_suffix('m') {
        parse(n, 60, 'm')
    } else if let Some(n) = s.strip_suffix('h') {
        parse(n, 3600, 'h')
    } else {
        s.parse::<u64>()
            .map_err(|_| format!("invalid duration {s:?}: expected '<n>[s|m|h]' or bare seconds"))
    }
}

#[cfg(test)]
mod parse_duration_tests {
    use super::parse_duration_secs;

    #[test]
    fn parses_units_and_bare_seconds() {
        assert_eq!(parse_duration_secs("30s").unwrap(), 30);
        assert_eq!(parse_duration_secs("2m").unwrap(), 120);
        assert_eq!(parse_duration_secs("1h").unwrap(), 3600);
        assert_eq!(parse_duration_secs("45").unwrap(), 45);
        assert_eq!(parse_duration_secs("  10s  ").unwrap(), 10);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_duration_secs("abc").is_err());
        assert!(parse_duration_secs("10x").is_err());
        assert!(parse_duration_secs("ms").is_err());
        assert!(parse_duration_secs("").is_err());
    }
}

#[cfg(test)]
mod app_base_url_tests {
    use super::resolve_app_base_url;

    /// Serialise env mutation: cargo runs tests in parallel and
    /// `CRONIQ_APP_URL` is process-global, so concurrent set/remove would
    /// race. Each test holds this guard for its whole body.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        match M.get_or_init(|| Mutex::new(())).lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[test]
    fn none_when_unset() {
        let _g = env_guard();
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(resolve_app_base_url(None), None);
    }

    #[test]
    fn reads_env_override() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_APP_URL", "https://croniq.example.com") };
        let url = resolve_app_base_url(None);
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(url, Some("https://croniq.example.com".to_string()));
    }

    #[test]
    fn blank_env_is_none() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_APP_URL", "   ") };
        let url = resolve_app_base_url(None);
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(url, None);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_APP_URL", "  https://app.example  ") };
        let url = resolve_app_base_url(None);
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(url, Some("https://app.example".to_string()));
    }

    #[test]
    fn dsl_app_url_beats_env() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_APP_URL", "https://env.example") };
        let url = resolve_app_base_url(Some("https://dsl.example"));
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(url, Some("https://dsl.example".to_string()));
    }

    #[test]
    fn blank_dsl_falls_through_to_env() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_APP_URL", "https://env.example") };
        let url = resolve_app_base_url(Some("   "));
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(url, Some("https://env.example".to_string()));
    }

    #[test]
    fn dsl_value_is_trimmed() {
        let _g = env_guard();
        unsafe { std::env::remove_var("CRONIQ_APP_URL") };
        assert_eq!(
            resolve_app_base_url(Some("  https://dsl.example  ")),
            Some("https://dsl.example".to_string())
        );
    }
}

#[cfg(test)]
mod db_backend_tests {
    use super::{DbBackend, classify_db_backend, resolve_db_spec};

    /// Serialise env mutation: `CRONIQ_DB` is process-global and cargo runs
    /// tests in parallel, so concurrent set/remove would race.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        match M.get_or_init(|| Mutex::new(())).lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[test]
    fn defaults_to_sqlite_when_blank() {
        let _g = env_guard();
        unsafe { std::env::remove_var("CRONIQ_DB") };
        assert_eq!(resolve_db_spec(""), "sqlite");
        assert_eq!(resolve_db_spec("   "), "sqlite");
        assert_eq!(resolve_db_spec("sqlite"), "sqlite");
    }

    #[test]
    fn env_overrides_dsl() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_DB", "postgres://h/db") };
        let spec = resolve_db_spec("sqlite");
        unsafe { std::env::remove_var("CRONIQ_DB") };
        assert_eq!(spec, "postgres://h/db");
    }

    #[test]
    fn blank_env_falls_through_to_dsl() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_DB", "   ") };
        let spec = resolve_db_spec("postgresql://dsl/db");
        unsafe { std::env::remove_var("CRONIQ_DB") };
        assert_eq!(spec, "postgresql://dsl/db");
    }

    #[test]
    fn classify_accepts_sqlite_and_postgres() {
        assert!(matches!(
            classify_db_backend("sqlite").unwrap(),
            DbBackend::Sqlite
        ));
        // Case-insensitive keyword.
        assert!(matches!(
            classify_db_backend("SQLite").unwrap(),
            DbBackend::Sqlite
        ));
        assert!(matches!(
            classify_db_backend("postgres://u:p@h:5432/db").unwrap(),
            DbBackend::Postgres
        ));
        assert!(matches!(
            classify_db_backend("postgresql://h/db").unwrap(),
            DbBackend::Postgres
        ));
    }

    #[test]
    fn classify_rejects_unknown() {
        // A different engine, a typo'd scheme, and a bare path all error
        // rather than silently opening SQLite.
        assert!(classify_db_backend("mysql://h/db").is_err());
        assert!(classify_db_backend("postgress://typo").is_err());
        assert!(classify_db_backend("/var/lib/croniq/croniq.db").is_err());
        assert!(classify_db_backend("").is_err());
    }
}
