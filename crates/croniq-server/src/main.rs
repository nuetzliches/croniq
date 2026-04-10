//! croniq-server binary: the main server process.
//!
//! Usage:
//! ```sh
//! croniq-server --config Croniqfile --listen :9090 --data-dir ./.data
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
    #[arg(short, long, default_value = ":9090")]
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

    // Shared runner state (registry + queue)
    let runner_state = AppState::new();

    // Restore queued executions from DB into the in-memory work queue.
    // This ensures executions survive server restarts.
    let restored = restore_queued_executions(&*store, &loaded.runtime.jobs, &runner_state).await;
    tracing::info!(restored, "queued executions restored from database");

    // Completion channel: HTTP complete → processor task
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();

    // Server state wrapping runner + completion channel + auth
    let auth_token = loaded.runtime.pull_api.as_ref().and_then(|p| p.auth.clone());
    let server_state = ServerState::with_auth(Arc::clone(&runner_state), completion_tx, auth_token);

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
    let jobs = loaded.runtime.jobs.clone();
    let triggers = loaded.triggers;

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
                            scheduler_loop.reload(new_config.triggers, new_config.runtime.jobs);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "failed to reload Croniqfile — keeping previous config");
                        }
                    }
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

    // ── Metrics server (optional) ────────────────────────────────────────────
    if let Some(ref metrics_listen) = cli.metrics {
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

    let app = server_router(server_state);

    tracing::info!(address = %addr, "croniq-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
