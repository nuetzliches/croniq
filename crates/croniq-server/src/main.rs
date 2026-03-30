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
    loader::{load_file, restore_trigger_states},
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

    // Completion channel: HTTP complete → processor task
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();

    // Server state wrapping runner + completion channel
    let server_state = ServerState::new(Arc::clone(&runner_state), completion_tx);

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
            interval.tick().await;
            let result = scheduler_loop.tick(chrono::Utc::now()).await;
            if !result.fired.is_empty() {
                tracing::debug!(count = result.fired.len(), "scheduler tick: jobs fired");
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
