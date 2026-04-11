use clap::{Parser, Subcommand};
use miette::Result;
use std::path::PathBuf;

mod commands;

// ─── Default server URL ───────────────────────────────────────────────────────

const DEFAULT_SERVER_URL: &str = "http://localhost:8080";

// ─── CLI definition ───────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "croniq", about = "Better Cron — job scheduling done right")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    // ── Config commands ───────────────────────────────────────────────────────

    /// Validate a Croniqfile for errors and warnings
    Validate {
        /// Path to Croniqfile
        #[arg(default_value = "Croniqfile")]
        path: PathBuf,
    },

    /// Format a Croniqfile
    Fmt {
        /// Path to Croniqfile
        #[arg(default_value = "Croniqfile")]
        path: PathBuf,

        /// Write formatted output back to file
        #[arg(short, long)]
        write: bool,
    },

    /// Compile a Croniqfile and print the runtime config as JSON
    Compile {
        /// Path to Croniqfile
        #[arg(default_value = "Croniqfile")]
        path: PathBuf,
    },

    /// Show diff between two Croniqfiles
    Diff {
        /// First file
        old: PathBuf,
        /// Second file
        new: PathBuf,
    },

    /// Convert a standard cron expression to Croniq DSL
    ///
    /// Examples:
    ///   croniq convert '*/15 * * * *'     → every 15 minutes
    ///   croniq convert '0 9 * * 1-5'      → every weekday at 09:00
    ///   croniq convert '0 3 1 * *'        → every 1st of month at 03:00
    ///   croniq convert '@daily'            → every day at 00:00
    Convert {
        /// Cron expression (5 fields: minute hour dom month dow, or @-macro)
        expr: String,
    },

    // ── Server commands ───────────────────────────────────────────────────────

    /// Show live scheduler status (queue depth, runner counts)
    Status {
        /// Croniq server URL
        #[arg(long, default_value = DEFAULT_SERVER_URL)]
        url: String,
    },

    /// List all connected runners with liveness status and capabilities
    #[command(name = "list-runners")]
    ListRunners {
        /// Croniq server URL
        #[arg(long, default_value = DEFAULT_SERVER_URL)]
        url: String,
    },

    /// Trigger a job immediately, bypassing its schedule
    Trigger {
        /// Job key (e.g. `billing:invoice-generate`)
        job_key: String,

        /// Croniq server URL
        #[arg(long, default_value = DEFAULT_SERVER_URL)]
        url: String,

        /// Capabilities a runner MUST have (repeatable)
        #[arg(long)]
        require: Vec<String>,

        /// Capabilities that are preferred but not mandatory (repeatable)
        #[arg(long)]
        prefer: Vec<String>,

        /// Timeout hint for the runner (e.g. `15m`, `2h`)
        #[arg(long, default_value = "5m")]
        timeout: String,
    },

    // ── Store commands ────────────────────────────────────────────────────────

    /// List dead-lettered executions from the SQLite store
    #[command(name = "dead-letters")]
    DeadLetters {
        /// Path to the croniq data directory (must contain `croniq.db`)
        #[arg(long)]
        data_dir: PathBuf,

        /// Filter to a specific job key
        #[arg(long)]
        job: Option<String>,

        /// Maximum number of entries to show
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },

    /// Initialize the database: create admin user and default API client
    Init {
        /// Path to the croniq data directory (will be created if needed)
        #[arg(long, default_value = "./.data")]
        data_dir: PathBuf,

        /// Admin username
        #[arg(long, default_value = "admin")]
        username: String,

        /// Admin password (prompted if not given)
        #[arg(long)]
        password: Option<String>,
    },

    /// Inspect a specific dead letter by ID
    #[command(name = "dead-letters-inspect")]
    DeadLettersInspect {
        /// The dead letter UUID to inspect
        id: String,

        /// Path to the croniq data directory (must contain `croniq.db`)
        #[arg(long)]
        data_dir: PathBuf,
    },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // Config
        Commands::Validate { path } => commands::config::validate(&path),
        Commands::Fmt { path, write } => commands::config::fmt(&path, write),
        Commands::Compile { path } => commands::config::compile(&path),
        Commands::Diff { old, new } => commands::config::diff(&old, &new),
        Commands::Convert { expr } => commands::config::convert_cron(&expr),

        // Server
        Commands::Status { url } => commands::server::status(&url),
        Commands::ListRunners { url } => commands::server::list_runners(&url),
        Commands::Trigger { job_key, url, require, prefer, timeout } => {
            commands::server::trigger(&url, &job_key, require, prefer, &timeout)
        }

        // Store
        Commands::DeadLetters { data_dir, job, limit } => {
            commands::store::dead_letters(&data_dir, job.as_deref(), limit)
        }
        Commands::DeadLettersInspect { id, data_dir } => {
            commands::store::dead_letters_inspect(&data_dir, &id)
        }
        Commands::Init { data_dir, username, password } => {
            commands::init::init(&data_dir, &username, password.as_deref())
        }
    }
}
