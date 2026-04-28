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
};
use croniq_store::sqlite::SqliteStore;
use tokio::sync::mpsc;
use tracing_subscriber::{EnvFilter, fmt};

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
}

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    tracing::info!(config = %cli.config.display(), "loading Croniqfile");
    let mut loaded = load_file(&cli.config)
        .with_context(|| format!("failed to load {}", cli.config.display()))?;

    let job_count = loaded.runtime.jobs.len();
    let active = loaded.triggers.len();
    tracing::info!(jobs = job_count, triggers = active, "configuration loaded");

    // Open (or create) the SQLite store
    std::fs::create_dir_all(&cli.data_dir)?;
    let db_path = cli.data_dir.join("croniq.db");
    let store: DynStore = sqlite_store(
        SqliteStore::open(&db_path)
            .with_context(|| format!("failed to open database at {}", db_path.display()))?,
    );

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

    // Restore queued executions from DB into the in-memory work queue.
    // This ensures executions survive server restarts.
    let restored = restore_queued_executions(&*store, &loaded.runtime.jobs, &runner_state).await;
    tracing::info!(restored, "queued executions restored from database");

    // Completion channel: HTTP complete → processor task
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();

    // Server state wrapping runner + completion channel + JWT auth
    // JWT secret priority: Croniqfile pull_api.auth > CRONIQ_JWT_SECRET env > $DATA_DIR/jwt.secret (auto-created on first boot)
    let jwt_secret = loaded.runtime.pull_api.as_ref()
        .and_then(|p| p.auth.clone())
        .or_else(|| std::env::var("CRONIQ_JWT_SECRET").ok())
        .unwrap_or_else(|| {
            let secret_path = cli.data_dir.join("jwt.secret");
            if let Ok(s) = std::fs::read_to_string(&secret_path) {
                let s = s.trim().to_string();
                if !s.is_empty() {
                    tracing::info!(path = %secret_path.display(), "JWT secret loaded from disk");
                    return s;
                }
            }
            let secret = uuid::Uuid::new_v4().to_string();
            match write_secret_file(&secret_path, &secret) {
                Ok(()) => tracing::info!(path = %secret_path.display(), "JWT secret generated and persisted"),
                Err(e) => tracing::warn!(path = %secret_path.display(), error = %e, "could not persist JWT secret — runners will need new tokens after restart"),
            }
            secret
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

    let mut server_state = ServerState::with_auth(
        Arc::clone(&runner_state),
        completion_tx,
        jwt_config,
        Some(Arc::clone(&store)),
    );
    // Inject scheduler_tx, dsl_jobs, and config_path into the shared state
    {
        let s = Arc::get_mut(&mut server_state).unwrap();
        s.scheduler_tx = Some(scheduler_cmd_tx);
        s.dsl_jobs = Some(Arc::clone(&dsl_jobs_shared));
        s.dsl_calendars = Some(Arc::clone(&dsl_calendars_shared));
        s.policy_dsl_adopt_on_mutate.store(
            loaded.runtime.policy.dsl_adopt_on_mutate,
            std::sync::atomic::Ordering::Relaxed,
        );
        s.config_path = Some(config_path_abs.clone());
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

    let mut scheduler_loop = SchedulerLoop::new(
        triggers,
        jobs.clone(),
        scheduler_store,
        Arc::clone(&runner_state),
    );

    let scheduler_reload_store = Arc::clone(&store);
    let scheduler_reload_snapshot = Arc::clone(&trigger_snapshot);
    let scheduler_reload_dsl = Arc::clone(&dsl_jobs_shared);
    let scheduler_reload_dsl_cals = Arc::clone(&dsl_calendars_shared);
    let scheduler_reload_policy = Arc::clone(&server_state.policy_dsl_adopt_on_mutate);
    let scheduler_reload_counters = Arc::clone(&reload_counters);

    let _scheduler_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let result = scheduler_loop.tick(chrono::Utc::now()).await;
                    if !result.fired.is_empty() {
                        tracing::debug!(count = result.fired.len(), "scheduler tick: jobs fired");
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

    // ── Completion processor task ─────────────────────────────────────────────
    let proc_store = Arc::clone(&store);
    let proc_jobs = jobs;

    let processor = Arc::new(CompletionProcessor::new(
        proc_jobs,
        proc_store,
        Arc::clone(&runner_state),
    ));

    let _completion_task = tokio::spawn(async move {
        while let Some(event) = completion_rx.recv().await {
            let outcome = processor.process(event).await;
            tracing::debug!(?outcome, "completion processed");
        }
    });

    // ── Watchdog task ─────────────────────────────────────────────────────────
    let watchdog = WatchdogLoop::new(
        loaded.runtime.jobs.clone(),
        Arc::clone(&store),
        Arc::clone(&runner_state),
    );

    let _watchdog_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let result = watchdog.sweep(chrono::Utc::now()).await;
            if !result.dead_runners.is_empty() {
                tracing::warn!(
                    dead = result.dead_runners.len(),
                    requeued = result.requeued.len(),
                    "watchdog: processed dead runners"
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
        let mcp_router = croniq_server::mcp::mcp_router(
            mcp_state,
            mcp_runner,
            Some(mcp_store),
            mcp_jobs,
            Some(mcp_triggers),
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

/// Write `content` to `path` with mode 0600 on Unix (world-unreadable).
fn write_secret_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
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
