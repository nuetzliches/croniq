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
    loader::{load_file, restore_trigger_states, restore_queued_executions},
    store::{DynStore, sqlite_store},
};
use croniq_store::sqlite::SqliteStore;
use tokio::sync::mpsc;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser)]
#[command(name = "croniq-server", about = "Croniq distributed job scheduler")]
struct Cli {
    /// Path to the Croniqfile configuration.
    #[arg(short, long, default_value = "Croniqfile")]
    config: PathBuf,

    /// Address and port to listen on.
    #[arg(short, long, default_value = ":4000")]
    listen: String,

    /// Directory for persistent data (SQLite database).
    #[arg(short, long, default_value = "./.data")]
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
    let lease_ttl_secs = loaded.runtime.pull_api.as_ref()
        .map(|p| parse_duration_secs(&p.lease_ttl))
        .unwrap_or(120);
    let runner_state = AppState::with_lease_ttl(lease_ttl_secs);

    // Restore queued executions from DB into the in-memory work queue.
    // This ensures executions survive server restarts.
    let restored = restore_queued_executions(&*store, &loaded.runtime.jobs, &runner_state).await;
    tracing::info!(restored, "queued executions restored from database");

    // Completion channel: HTTP complete → processor task
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();

    // Server state wrapping runner + completion channel + JWT auth
    // JWT secret comes from: Croniqfile pull_api.auth > CRONIQ_JWT_SECRET env > generated random
    let jwt_secret = loaded.runtime.pull_api.as_ref()
        .and_then(|p| p.auth.clone())
        .or_else(|| std::env::var("CRONIQ_JWT_SECRET").ok())
        .unwrap_or_else(|| {
            let secret = uuid::Uuid::new_v4().to_string();
            tracing::warn!("no JWT secret configured — generated ephemeral secret (set CRONIQ_JWT_SECRET or pull_api.auth in Croniqfile for persistence)");
            secret
        });
    let jwt_config = Some(croniq_auth::jwt::JwtConfig {
        secret: jwt_secret,
        ..Default::default()
    });
    // Scheduler command channel for live job registration via API
    let (scheduler_cmd_tx, mut scheduler_cmd_rx) = mpsc::unbounded_channel::<croniq_server::scheduler::SchedulerCommand>();

    // Shared snapshot of DSL jobs — kept in sync by the scheduler task on
    // Croniqfile reload so the REST API can union DSL entries into
    // `/v1/jobs` and `/v1/schedules`.
    let dsl_jobs_shared = Arc::new(tokio::sync::RwLock::new(loaded.runtime.jobs.clone()));

    let mut server_state = ServerState::with_auth(Arc::clone(&runner_state), completion_tx, jwt_config, Some(Arc::clone(&store)));
    // Inject scheduler_tx and dsl_jobs into the shared state
    {
        let s = Arc::get_mut(&mut server_state).unwrap();
        s.scheduler_tx = Some(scheduler_cmd_tx);
        s.dsl_jobs = Some(Arc::clone(&dsl_jobs_shared));
    }

    // ── File watcher (optional) ─────────────────────────────────────────────
    let (reload_tx, mut reload_rx) = mpsc::unbounded_channel::<std::path::PathBuf>();

    if cli.watch {
        let config_path = cli.config.canonicalize().unwrap_or_else(|_| cli.config.clone());
        match croniq_server::watcher::watch_config(&config_path) {
            Ok(raw_rx) => {
                let debounce_tx = reload_tx.clone();
                tokio::spawn(croniq_server::watcher::debounced_reload_loop(
                    raw_rx,
                    std::time::Duration::from_millis(500),
                    debounce_tx,
                ));
                tracing::info!(path = %config_path.display(), "watching Croniqfile for changes");
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not start file watcher — hot-reload disabled");
            }
        }
    }

    // ── Scheduler task ────────────────────────────────────────────────────────
    let scheduler_store = Arc::clone(&store);
    let mut jobs = loaded.runtime.jobs.clone();
    let mut triggers = loaded.triggers;

    // Reconcile API/runner-registered jobs from DB (not in Croniqfile)
    {
        use croniq_server::loader::{trigger_from_definition, job_config_from_definition};

        let now = chrono::Utc::now();
        if let Ok(api_triggers) = store.list_triggers(None) {
            let mut api_count = 0;
            for def in &api_triggers {
                if def.managed_by == "dsl" || !def.enabled { continue; }
                if triggers.contains_key(&def.job_key) { continue; } // Croniqfile takes precedence
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

    let mut scheduler_loop =
        SchedulerLoop::new(triggers, jobs.clone(), scheduler_store, Arc::clone(&runner_state));

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
                    tracing::info!(path = %path.display(), "Croniqfile changed — reloading");
                    match load_file(&path) {
                        Ok(new_config) => {
                            let new_jobs = new_config.runtime.jobs.clone();
                            scheduler_loop.reload(new_config.triggers, new_config.runtime.jobs);
                            *dsl_jobs_shared.write().await = new_jobs;
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "failed to reload Croniqfile — keeping previous config");
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

    let processor = Arc::new(CompletionProcessor::new(proc_jobs, proc_store, Arc::clone(&runner_state)));

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
        loaded.runtime.observability.as_ref()
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
    let addr: std::net::SocketAddr = cli.listen
        .trim_start_matches(':')
        .parse::<u16>()
        .map(|p| ([0, 0, 0, 0], p).into())
        .or_else(|_| cli.listen.parse())
        .with_context(|| format!("invalid listen address: {}", cli.listen))?;

    let mut app = server_router(server_state);

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

/// Parse a duration string like "60s", "2m", "1h" to seconds. Falls back to 120.
fn parse_duration_secs(s: &str) -> u64 {
    let s = s.trim();
    if let Some(n) = s.strip_suffix('s') {
        n.parse().unwrap_or(120)
    } else if let Some(n) = s.strip_suffix('m') {
        n.parse::<u64>().map(|v| v * 60).unwrap_or(120)
    } else if let Some(n) = s.strip_suffix('h') {
        n.parse::<u64>().map(|v| v * 3600).unwrap_or(120)
    } else {
        s.parse().unwrap_or(120)
    }
}
