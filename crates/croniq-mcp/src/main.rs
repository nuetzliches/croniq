//! croniq-mcp binary: start the MCP server on stdio.
//!
//! Usage (e.g. from Claude Desktop config):
//! ```json
//! {
//!   "mcpServers": {
//!     "croniq": {
//!       "command": "croniq-mcp",
//!       "args": ["--mutations", "--data-dir", "/var/lib/croniq"]
//!     }
//!   }
//! }
//! ```
//!
//! ## Flags
//!
//! | Flag | Description |
//! |------|-------------|
//! | `--mutations` | Enable write tools (enqueue_job, cancel_execution, job_trigger, dlq_retry) |
//! | `--data-dir <path>` | Open the SQLite store at `<path>/croniq.db` (required for dlq_retry and store-backed operations) |

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use croniq_mcp::CroniqMcp;
use croniq_runner::AppState;
use rmcp::ServiceExt;
use tracing_subscriber::{EnvFilter, fmt};

// ─── CLI args ─────────────────────────────────────────────────────────────────

struct Args {
    /// Enable mutation tools.
    mutations: bool,
    /// Path to data directory containing the SQLite database.
    data_dir: Option<PathBuf>,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut mutations = false;
        let mut data_dir: Option<PathBuf> = None;
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--mutations" => mutations = true,
                "--data-dir" => {
                    let path = args.next().context("--data-dir requires a path argument")?;
                    data_dir = Some(PathBuf::from(path));
                }
                other => {
                    anyhow::bail!("Unknown argument: {other}. Usage: croniq-mcp [--mutations] [--data-dir <path>]");
                }
            }
        }

        Ok(Self { mutations, data_dir })
    }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Log to stderr so stdout remains clean for the MCP stdio transport.
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .without_time()
        .with_ansi(false)
        .init();

    let args = Args::parse()?;

    tracing::info!(
        mutations = args.mutations,
        data_dir = ?args.data_dir,
        "croniq-mcp starting"
    );

    let state = AppState::new();

    let server = match args.data_dir {
        Some(data_dir) => {
            // Open the SQLite database.
            let db_path = data_dir.join("croniq.db");
            tracing::info!(path = %db_path.display(), "opening store");

            let sqlite = croniq_store::sqlite::SqliteStore::open(&db_path)
                .with_context(|| format!("failed to open store at {}", db_path.display()))?;
            let store: croniq_mcp::DynStore = Arc::new(sqlite);

            CroniqMcp::new_with_store(Arc::clone(&state), store, vec![], args.mutations)
        }
        None if args.mutations => {
            tracing::warn!(
                "--mutations enabled without --data-dir: dlq_retry will not be available"
            );
            CroniqMcp::new_mutations_only(Arc::clone(&state))
        }
        None => CroniqMcp::new(Arc::clone(&state)),
    };

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .inspect_err(|e| tracing::error!("MCP server error: {e}"))?;

    service.waiting().await?;

    tracing::info!("croniq-mcp stopped");
    Ok(())
}
