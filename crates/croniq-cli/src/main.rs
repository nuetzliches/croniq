use clap::{Parser, Subcommand};
use miette::Result;
use std::path::PathBuf;

/// Parse common truthy/falsy strings so env-var inputs like
/// `CRONIQ_DEMO_MFA=1` (docker convention) work alongside the bare
/// `--demo-mfa` flag form. Clap's default bool parser only accepts
/// the literal `true`/`false`.
fn parse_truthy(s: &str) -> std::result::Result<bool, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" | "" => Ok(false),
        other => Err(format!(
            "expected 1/true/yes/on or 0/false/no/off, got '{other}'"
        )),
    }
}

mod commands;

// ─── Default server URL ───────────────────────────────────────────────────────

const DEFAULT_SERVER_URL: &str = "http://localhost:4000";

// ─── CLI definition ───────────────────────────────────────────────────────────

#[derive(Parser)]
// `version` picks up CARGO_PKG_VERSION, which the release workflow rewrites
// in-place to the pushed tag — so `--version` reports the same value the API
// and the MCP handshake do (issue #407).
#[command(
    name = "croniq",
    version,
    about = "Better Cron — job scheduling done right"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Croniq server URL, for the commands that talk to a running server.
    #[arg(long, global = true, default_value = DEFAULT_SERVER_URL, env = "CRONIQ_URL")]
    url: String,

    /// API key to authenticate with, sent as `Authorization: ApiKey <key>`.
    ///
    /// Global rather than per-command: nine subcommands need it, and an
    /// operator should be able to export it once. Without it every
    /// authenticated endpoint answers 401 (issue #475).
    #[arg(long, global = true, env = "CRONIQ_API_KEY", hide_env_values = true)]
    api_key: Option<String>,
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
    Status,

    /// List all connected runners with liveness status and capabilities
    #[command(name = "list-runners")]
    ListRunners,

    /// Trigger a job immediately, bypassing its schedule
    Trigger {
        /// Job key (e.g. `billing:invoice-generate`)
        job_key: String,

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

    // ── Credential commands ───────────────────────────────────────────────────
    /// Manage API clients on a running server (requires `api-clients:admin`)
    #[command(name = "api-clients")]
    ApiClients {
        #[command(subcommand)]
        action: ApiClientAction,
    },

    /// Manage API keys on a running server (requires `api-keys:admin`)
    #[command(name = "api-keys")]
    ApiKeys {
        #[command(subcommand)]
        action: ApiKeyAction,
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

    /// Initialize the database: create admin user (pass --api-key to also
    /// seed a default API client bound to that key)
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

        /// Seed a default API client bound to this key. Must start with
        /// "croniq_". Without this flag no API client/key is created —
        /// use the UI (Settings → API Keys) for production setups.
        #[arg(long)]
        api_key: Option<String>,

        /// Comma-separated scopes for the seeded client when `--api-key` is
        /// passed. Defaults to `admin`. Examples:
        ///   `--scopes work:poll,work:ack,work:renew,work:events` (runner)
        ///   `--scopes jobs:read,executions:read,dead-letters:read` (read-only dashboard)
        #[arg(long, value_delimiter = ',', requires = "api_key")]
        scopes: Option<Vec<String>>,

        /// Demo-only: pre-enable TOTP for the seeded admin and bake the
        /// literal "123456" into the recovery codes so a marketing
        /// walkthrough can exercise the MFA step. Driven by
        /// `CRONIQ_DEMO_MFA=1` in the docker entrypoint. Never use in
        /// production — anyone with the demo image learns admin login
        /// + a working recovery code.
        #[arg(
            long,
            env = "CRONIQ_DEMO_MFA",
            num_args = 0..=1,
            default_value_t = false,
            default_missing_value = "true",
            value_parser = parse_truthy,
        )]
        demo_mfa: bool,

        /// Print the seeded API key to stdout even when it is not a
        /// terminal. Without this, a redirected stdout gets the key written
        /// to `$DATA_DIR/initial-credentials` (mode 0600) instead, so it
        /// does not leak into logs.
        #[arg(long)]
        print_secrets: bool,
    },

    /// Zero-to-running in one command: creates Croniqfile, inits DB, prints next steps
    Quickstart {
        /// Data directory
        #[arg(long, default_value = "./.data")]
        data_dir: PathBuf,

        /// Croniqfile output path
        #[arg(long, default_value = "Croniqfile")]
        config: PathBuf,

        /// Admin password (random if not given)
        #[arg(long)]
        password: Option<String>,

        /// Print the generated admin password + API key to stdout even when
        /// it is not a terminal. Without this, a redirected stdout gets them
        /// written to `$DATA_DIR/initial-credentials` (mode 0600) instead, so
        /// they do not leak into logs.
        #[arg(long)]
        print_secrets: bool,
    },

    /// Migrate a crontab file to a Croniqfile
    Migrate {
        /// Path to the crontab file to convert
        crontab: PathBuf,

        /// Output Croniqfile path (prints to stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,
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

#[derive(Subcommand)]
enum ApiClientAction {
    /// List API clients with their scopes and owner
    List {
        /// Emit raw JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Create an API client
    Create {
        /// Client name (e.g. `runner-poll`)
        #[arg(long)]
        name: String,
        /// Comma-separated scopes (e.g. `work:poll,work:ack,work:renew`)
        #[arg(long, value_delimiter = ',')]
        scopes: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Change a client's name, scopes, or active flag
    Update {
        /// Client ID from `croniq api-clients list`
        client_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_delimiter = ',')]
        scopes: Option<Vec<String>>,
        /// Re-enable a deactivated client
        #[arg(long, conflicts_with = "inactive")]
        active: bool,
        /// Deactivate the client without deleting it
        #[arg(long)]
        inactive: bool,
        #[arg(long)]
        json: bool,
    },
    /// Delete a client and every API key bound to it
    Delete {
        /// Client ID from `croniq api-clients list`
        client_id: String,
    },
}

#[derive(Subcommand)]
enum ApiKeyAction {
    /// List a client's keys, including retiring and revoked ones
    List {
        /// Client ID from `croniq api-clients list`
        #[arg(long = "client")]
        client_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Mint a new key. The raw value is shown once and never again.
    Create {
        /// Client ID from `croniq api-clients list`
        #[arg(long = "client")]
        client_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Revoke a key immediately, ending any rotation grace window
    Revoke {
        /// Key ID from `croniq api-keys list`
        key_id: String,
    },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Built once, used by every command that reaches the server. Cheap for
    // the ones that do not (`validate`, `fmt`, …) — it opens no connection.
    let remote = commands::remote::Remote::new(&cli.url, cli.api_key.clone());

    match cli.command {
        // Config
        Commands::Validate { path } => commands::config::validate(&path),
        Commands::Fmt { path, write } => commands::config::fmt(&path, write),
        Commands::Compile { path } => commands::config::compile(&path),
        Commands::Diff { old, new } => commands::config::diff(&old, &new),
        Commands::Convert { expr } => commands::config::convert_cron(&expr),

        // Server
        Commands::Status => commands::server::status(&remote),
        Commands::ListRunners => commands::server::list_runners(&remote),
        Commands::Trigger {
            job_key,
            require,
            prefer,
            timeout,
        } => commands::server::trigger(&remote, &job_key, require, prefer, &timeout),

        // Credentials
        Commands::ApiClients { action } => match action {
            ApiClientAction::List { json } => commands::credentials::clients_list(&remote, json),
            ApiClientAction::Create { name, scopes, json } => {
                commands::credentials::clients_create(&remote, &name, &scopes, json)
            }
            ApiClientAction::Update {
                client_id,
                name,
                scopes,
                active,
                inactive,
                json,
            } => {
                // Absent means "leave it alone"; the two flags conflict at the
                // clap level, so at most one can be set here.
                let is_active = if active {
                    Some(true)
                } else if inactive {
                    Some(false)
                } else {
                    None
                };
                commands::credentials::clients_update(
                    &remote,
                    &client_id,
                    name.as_deref(),
                    scopes.as_deref(),
                    is_active,
                    json,
                )
            }
            ApiClientAction::Delete { client_id } => {
                commands::credentials::clients_delete(&remote, &client_id)
            }
        },
        Commands::ApiKeys { action } => match action {
            ApiKeyAction::List { client_id, json } => {
                commands::credentials::keys_list(&remote, &client_id, json)
            }
            ApiKeyAction::Create { client_id, json } => {
                commands::credentials::keys_create(&remote, &client_id, json)
            }
            ApiKeyAction::Revoke { key_id } => commands::credentials::keys_revoke(&remote, &key_id),
        },

        // Store
        Commands::DeadLetters {
            data_dir,
            job,
            limit,
        } => commands::store::dead_letters(&data_dir, job.as_deref(), limit),
        Commands::DeadLettersInspect { id, data_dir } => {
            commands::store::dead_letters_inspect(&data_dir, &id)
        }
        Commands::Init {
            data_dir,
            username,
            password,
            api_key,
            scopes,
            demo_mfa,
            print_secrets,
        } => {
            let mut sink = commands::secret_output::CredentialSink::new(print_secrets);
            commands::init::init(
                &data_dir,
                &username,
                password.as_deref(),
                api_key.as_deref(),
                scopes,
                demo_mfa,
                &mut sink,
            )?;
            sink.flush(&data_dir)
        }
        Commands::Quickstart {
            data_dir,
            config,
            password,
            print_secrets,
        } => {
            commands::quickstart::quickstart(&data_dir, &config, password.as_deref(), print_secrets)
        }
        Commands::Migrate { crontab, output } => {
            commands::migrate::migrate(&crontab, output.as_deref())
        }
    }
}
