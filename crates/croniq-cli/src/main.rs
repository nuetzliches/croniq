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

        /// Timeout hint for the runner (e.g. `15m`, `2h`).
        /// Defaults to the job's configured timeout.
        #[arg(long)]
        timeout: Option<String>,
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

        /// Admin password (prompted without echo if not given). Prefer the
        /// `CRONIQ_ADMIN_PASSWORD` environment variable: a value on the
        /// command line is visible to every local user in the process list.
        #[arg(long, env = "CRONIQ_ADMIN_PASSWORD", hide_env_values = true)]
        password: Option<String>,

        /// Seed a default API client bound to this key. Must start with
        /// "croniq_". Without it no API client/key is created — use the UI
        /// (Settings → API Keys) for production setups. Prefer the
        /// `CRONIQ_INIT_API_KEY` environment variable over the flag;
        /// `CRONIQ_API_KEY` is never used here.
        ///
        /// Same clap id as the global `--api-key`, which this shadows: a
        /// distinct id would register `--api-key` twice on this subcommand.
        /// The value is therefore only taken when it was typed on the
        /// command line — see `init_api_key`.
        #[arg(long)]
        api_key: Option<String>,

        /// Comma-separated scopes for the seeded client when an API key is
        /// seeded. Defaults to `admin`. Examples:
        ///   `--scopes work:poll,work:ack,work:renew,work:events` (runner)
        ///   `--scopes jobs:read,executions:read,dead-letters:read` (read-only dashboard)
        #[arg(long, value_delimiter = ',')]
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

        /// No effect; accepted so existing scripts keep working. `init`
        /// generates no secret: the API key comes from `--api-key` and is
        /// only confirmed by its prefix.
        #[arg(long, hide = true)]
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

/// Environment variable `croniq init` seeds its API key from.
const INIT_API_KEY_ENV: &str = "CRONIQ_INIT_API_KEY";

/// The key `croniq init` seeds, if any: `--api-key` typed on the command
/// line, else `CRONIQ_INIT_API_KEY`.
///
/// `Init.api_key` shares its clap id with the global `--api-key`, whose
/// value — including one from `CRONIQ_API_KEY` — clap propagates into the
/// subcommand. `CRONIQ_API_KEY` is what the CLI and SDKs read to *present* a
/// credential, often a narrow runner key, so taking it here seeded it as a
/// new admin-scoped client on any host that happened to export it (review
/// 2026-10-24, follow-up to R-6). Only a value whose source is the command
/// line is the operator asking for a seed.
fn init_api_key(init: &clap::ArgMatches, init_env: Option<String>) -> Option<String> {
    let typed = init.value_source("api_key") == Some(clap::parser::ValueSource::CommandLine);
    if typed {
        return init.get_one::<String>("api_key").cloned();
    }
    init_env.filter(|v| !v.is_empty())
}

fn main() -> Result<()> {
    use clap::{CommandFactory, FromArgMatches};
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
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
        } => commands::server::trigger(&remote, &job_key, require, prefer, timeout),

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
            api_key: _,
            scopes,
            demo_mfa,
            print_secrets: _,
        } => {
            let api_key = matches
                .subcommand_matches("init")
                .and_then(|m| init_api_key(m, std::env::var(INIT_API_KEY_ENV).ok()));
            if scopes.is_some() && api_key.is_none() {
                return Err(miette::miette!(
                    "--scopes needs a key to seed: pass --api-key or set {INIT_API_KEY_ENV}"
                ));
            }
            commands::init::init(
                &data_dir,
                &username,
                password.as_deref(),
                api_key.as_deref(),
                scopes,
                demo_mfa,
            )
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_passes_clap_debug_asserts() {
        // Duplicate long flags between global and subcommand args, among
        // others, only surface here.
        Cli::command().debug_assert();
    }

    fn init_key(args: &[&str], init_env: Option<&str>) -> Option<String> {
        let m = Cli::command().try_get_matches_from(args).unwrap();
        init_api_key(
            m.subcommand_matches("init").unwrap(),
            init_env.map(str::to_string),
        )
    }

    /// Review 2026-10-24, follow-up to R-6: `CRONIQ_API_KEY` (the global
    /// `--api-key`) must never become the key `init` seeds as an admin
    /// client. Set for this process only; no other test reads it.
    #[test]
    fn init_never_seeds_from_the_global_api_key() {
        // SAFETY: test-only; nothing else in this crate's unit tests reads
        // or writes CRONIQ_API_KEY.
        unsafe { std::env::set_var("CRONIQ_API_KEY", "croniq_runner_key_from_env") };
        assert_eq!(init_key(&["croniq", "init"], None), None);
        assert_eq!(
            init_key(&["croniq", "init"], Some("croniq_init_env")),
            Some("croniq_init_env".into())
        );
        assert_eq!(
            init_key(&["croniq", "init", "--api-key", "croniq_typed"], None),
            Some("croniq_typed".into()),
            "the subcommand's own flag still seeds"
        );
        assert_eq!(
            init_key(
                &["croniq", "init", "--api-key", "croniq_typed"],
                Some("croniq_init_env")
            ),
            Some("croniq_typed".into()),
            "the flag wins over the env var"
        );
        assert_eq!(init_key(&["croniq", "init"], Some("")), None);
        unsafe { std::env::remove_var("CRONIQ_API_KEY") };
    }
}
