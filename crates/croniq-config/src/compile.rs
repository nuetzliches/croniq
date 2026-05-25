//! Compiles a Croniqfile AST into a RuntimeConfig with resolved defaults and placeholders.

use crate::ast::{self, *};
use crate::placeholders;
use crate::schedule::CompiledSchedule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resolve a single `StringValue` against the vars map.
///
/// If the value is a placeholder (`{vars.X}`, `{env.X}`, `{$X}`, `{file.X}`),
/// run it through `placeholders::resolve`. On failure (unresolved env var,
/// missing file, etc.) we fall back to the raw value so a misconfigured
/// Croniqfile still compiles — same shape as the previous behaviour, which
/// was to never resolve at all.
fn resolve_str(val: &StringValue, vars: &HashMap<String, String>) -> String {
    if val.is_placeholder {
        placeholders::resolve(&val.value, vars).unwrap_or_else(|_| val.value.clone())
    } else {
        val.value.clone()
    }
}

/// Resolve the first arg of a directive into an owned `String`, applying
/// placeholder substitution if present.
fn first_arg(d: &Directive, vars: &HashMap<String, String>) -> Option<String> {
    d.args.first().map(|a| resolve_str(a, vars))
}

/// Walk the top-level items and collect every `vars { … }` entry into a
/// `HashMap` BEFORE compilation proper begins. This makes the order of
/// `vars`/`defaults`/`calendar`/`job` blocks irrelevant — placeholders are
/// resolvable as long as the referenced var is defined somewhere in the file.
fn collect_vars(ast: &Croniqfile) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    // First, gather literal entries (so {vars.X} placeholders inside other
    // vars values can resolve against already-known vars). We do not attempt
    // recursive resolution beyond a single pass.
    for item in &ast.items {
        if let Item::Vars(v) = item {
            for d in &v.entries {
                if let Some(val) = d.args.first()
                    && !val.is_placeholder
                {
                    vars.insert(d.key.value.clone(), val.value.clone());
                }
            }
        }
    }
    // Second pass: resolve placeholder vars values against the literals we
    // just collected (plus env/file lookups via `placeholders::resolve`).
    for item in &ast.items {
        if let Item::Vars(v) = item {
            for d in &v.entries {
                if let Some(val) = d.args.first()
                    && val.is_placeholder
                {
                    let resolved = resolve_str(val, &vars);
                    vars.insert(d.key.value.clone(), resolved);
                }
            }
        }
    }
    vars
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeConfig {
    pub server: ServerConfig,
    pub pull_api: Option<PullApiConfig>,
    pub observability: Option<ObservabilityConfig>,
    pub mcp: Option<McpConfig>,
    /// OIDC/SSO provider. None unless an `oidc {}` block is present.
    /// Only `client_secret` stays out of the DSL — server boot pulls
    /// it from `CRONIQ_OIDC_CLIENT_SECRET` and merges with this struct.
    pub oidc: Option<OidcDslConfig>,
    /// UI sign-in method gates (`auth { password { enabled false } }`).
    /// Absent block ⇒ defaults (every method enabled).
    pub auth: AuthDslConfig,
    /// Server-wide opt-in flags. Absent block ⇒ all defaults (deny).
    pub policy: PolicyConfig,
    /// Failure-alert configuration (issue #140). Absent block ⇒ empty
    /// (no rules fire). The `CRONIQ_ON_FAILURE_CMD` env var continues
    /// to work for one release; at boot the server synthesises a
    /// catch-all rule from it when set.
    pub alerts: AlertsConfig,
    pub jobs: Vec<JobConfig>,
    pub calendars: Vec<CalendarConfig>,
}

/// OIDC settings parsed from the Croniqfile `oidc {}` block. All
/// fields are optional in the DSL; the server merges them with the
/// `CRONIQ_OIDC_*` env vars at startup (DSL wins where both are set).
#[derive(Debug, Clone, Default, Serialize)]
pub struct OidcDslConfig {
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub redirect_url: Option<String>,
    pub default_role: Option<String>,
    pub provider_name: Option<String>,
    pub post_login_redirect: Option<String>,
}

/// UI sign-in method gates, parsed from the Croniqfile `auth {}` block.
/// All sub-blocks are optional; an absent `auth {}` means every method is
/// enabled. Today only the `password {}` sub-block is defined.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AuthDslConfig {
    pub password: PasswordAuthConfig,
}

/// `auth { password { enabled bool } }`. The `enabled` flag governs the
/// `/v1/auth/login` (and related) endpoints. `None` here means the DSL
/// did not set it — the server merges with `CRONIQ_PASSWORD_LOGIN_ENABLED`
/// at boot; if neither source sets the flag, password login stays on.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PasswordAuthConfig {
    pub enabled: Option<bool>,
}

/// HTTP MCP-server configuration. Absent block ⇒ default (enabled).
#[derive(Debug, Clone, Serialize)]
pub struct McpConfig {
    pub enabled: bool,
    /// Additional Host-header values the `/mcp` endpoint should accept on
    /// top of rmcp's built-in loopback allowlist (`localhost`, `127.0.0.1`,
    /// `::1`). Per issue #114 the directive is additive — empty / absent
    /// keeps the loopback-only default; entries listed here are appended
    /// to rmcp's allowlist, never replace it. Wildcards are not supported;
    /// enumerate every public hostname explicitly. For an IPv6 literal with
    /// port, quote it: `allowed_hosts "[::1]:8443"`.
    pub allowed_hosts: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_hosts: Vec::new(),
        }
    }
}

/// Server-wide opt-in policy flags. Default state is restrictive — every
/// flag must be set explicitly in the Croniqfile to take effect.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PolicyConfig {
    /// When `true`, mutating an API endpoint on a DSL-managed resource (job,
    /// schedule, calendar) is allowed via the explicit `/adopt` action: the
    /// resource is copied into the API store and the DSL key is excluded
    /// from subsequent reloads. Default: `false` (mutations return 409).
    ///
    /// Phase 2 only supports the boolean form. The grammar is shaped so a
    /// future per-resource block (`dsl_adopt_on_mutate { calendars true; jobs false }`)
    /// can be added without breaking existing files.
    pub dsl_adopt_on_mutate: bool,
}

// ─── Alerts (issue #140) ───────────────────────────────────────────

/// Failure-alert configuration: named channels + rules referencing them.
///
/// Default state is empty — no rules, no channels, no alerts dispatched.
/// For back-compat, the server synthesises a catch-all rule on a
/// synthesised shell channel at boot when `CRONIQ_ON_FAILURE_CMD` is
/// set; that path emits a deprecation warning and lives separately
/// from this struct (it's a *runtime* addition, not a DSL fact).
#[derive(Debug, Clone, Default, Serialize)]
pub struct AlertsConfig {
    /// Channels keyed by name. The DSL guarantees uniqueness — the
    /// compile step rejects duplicates with a placeholder-error
    /// channel so the rest of the file still compiles for diff
    /// reporting.
    pub channels: HashMap<String, ChannelConfig>,
    /// Rules in declaration order. Order is preserved so audit logs
    /// reference rule names deterministically.
    pub rules: Vec<RuleConfig>,
}

/// A named delivery target. PR-1 of #140 only ships the `shell` kind;
/// later PRs add `webhook` and `email`. The variant is `Unknown` when
/// the kind directive is missing or unrecognised — compile keeps the
/// channel so rule-name references still resolve, but the evaluator
/// skips it with a runtime warning at fire time.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelConfig {
    pub name: String,
    pub kind: ChannelKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChannelKind {
    /// `shell "/usr/local/bin/page-oncall.sh"` — invoked with the
    /// failure context in `CRONIQ_*` env vars (back-compat with the
    /// pre-#140 `CRONIQ_ON_FAILURE_CMD` env-var hook).
    Shell { command: String },
    /// Placeholder for unknown or future kinds (webhook, email).
    /// Channels with this kind compile cleanly so rule references
    /// resolve; the evaluator logs and skips them.
    Unknown { reason: String },
}

/// A rule = trigger predicate + named channels to dispatch to.
///
/// Trigger types in PR-1: `job_failed` only. `job_sla_missed` is
/// reserved and rejected at compile time so a typo doesn't silently
/// match nothing.
#[derive(Debug, Clone, Serialize)]
pub struct RuleConfig {
    pub name: String,
    pub trigger: RuleTrigger,
    /// Glob pattern matched against the job key. `*` matches anything
    /// (default).
    pub job_key_glob: String,
    /// Minimum attempt number that must have been reached before this
    /// rule fires. Defaults to 1.
    pub min_attempts: u32,
    /// When `true`, only fire on dead-letter (not on dropped-because-
    /// dead-letter-disabled). Defaults to false (fire on any permanent
    /// failure).
    pub dead_letter_only: bool,
    /// Per-(rule, job_key) suppression window. `None` disables
    /// throttling — every matching failure fires. Stored as duration
    /// string (parsed by the server at boot, kept as a string in the
    /// DSL so the formatter can round-trip).
    pub throttle: Option<String>,
    /// Channel names this rule dispatches to. Compile validates that
    /// every name resolves; unknown names become a compile error
    /// (returned as part of the rule for downstream diagnostic display
    /// while still letting the rest of the file compile).
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleTrigger {
    /// Permanent failure (dead-letter or dropped). The only trigger
    /// shipped in PR-1.
    JobFailed,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ObservabilityConfig {
    pub log: Option<LogConfig>,
    pub metrics: Option<MetricsConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogConfig {
    pub level: String,
    pub format: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsConfig {
    pub listen: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerConfig {
    pub listen: String,
    pub data_dir: String,
    pub db: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullApiConfig {
    pub listen: String,
    pub auth: Option<String>,
    pub lease_ttl: String,
}

/// How a job's executions are tracked and persisted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// Full persistence: execution record written to DB before dispatch,
    /// survives restarts, supports retry and dead-letter.
    #[default]
    Queued,
    /// Lightweight: no execution record at fire time, no catch-up after
    /// restart. Ideal for high-frequency monitoring/heartbeat jobs.
    Ephemeral,
}

/// What happens to missed fires after a server restart or prolonged downtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatchUpPolicy {
    /// Replay all missed fires (bounded by max_queue_depth).
    #[default]
    All,
    /// Coalesce missed fires into a single execution (the latest one).
    Latest,
    /// Discard missed fires; compute the next future fire time.
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobConfig {
    pub key: String,
    pub namespace: String,
    pub name: String,
    pub variant: Option<String>,
    pub description: Option<String>,
    pub schedule: CompiledSchedule,
    pub schedule_summary: String,
    pub timezone: Option<String>,
    pub calendar: Option<String>,
    pub window: Option<String>,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub runner: RunnerConfig,
    pub retry: RetryConfig,
    pub timeout: Option<String>,
    pub dead_letter: DeadLetterConfig,
    pub metadata: HashMap<String, String>,
    /// How executions are tracked: `queued` (persistent) or `ephemeral` (fire-and-forget).
    pub execution_mode: ExecutionMode,
    /// Restart behaviour for missed fires: `all`, `latest`, or `none`.
    pub catch_up: CatchUpPolicy,
    /// Max time an execution may sit in the queue before being cancelled.
    /// Duration string (e.g. "30m", "1h", "24h"). `None` means no limit.
    pub queue_ttl: Option<String>,
    /// Max queued executions per job before new fires are skipped.
    /// `None` falls back to the global default of 10.
    pub max_queue_depth: Option<u32>,
    /// Free-form tags for filtering/grouping. NOT routing-relevant —
    /// runner capabilities handle routing. Convention: `key=value` strings.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunnerConfig {
    pub require: Vec<String>,
    pub prefer: Vec<String>,
    pub exclude: Vec<String>,
    pub sticky: bool,
}

/// Execution payload attached to a job by a qualified `runner shell { ... }`
/// or `runner exec { ... }` block. Carried verbatim through the dispatch
/// pipeline as a JSON-encoded `__runner_exec` metadata key so that
/// thin runner binaries (e.g. `croniq-shell-runner`) can decode it without
/// touching the server-side data model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RunnerExec {
    /// `sh -c "<command>"` — the shell handles word-splitting, pipes, redirects.
    Shell {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workdir: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
    /// `argv[0] argv[1] ...` — direct exec, no shell. Avoids quoting hazards.
    Exec {
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workdir: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
}

/// Metadata key under which a compiled `RunnerExec` is stamped on the job's
/// `metadata` map. The runner binary deserialises this back into `RunnerExec`.
pub const RUNNER_EXEC_METADATA_KEY: &str = "__runner_exec";

#[derive(Debug, Clone, Serialize)]
pub struct RetryConfig {
    pub strategy: String,
    pub max_attempts: u32,
    pub base: Option<String>,
    pub cap: Option<String>,
    pub delay: Option<String>,
    pub step: Option<String>,
    pub jitter: Option<f64>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            strategy: "exponential".into(),
            max_attempts: 3,
            base: Some("2s".into()),
            cap: Some("30s".into()),
            delay: None,
            step: None,
            jitter: Some(0.25),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeadLetterConfig {
    pub enabled: bool,
    pub retention: Option<String>,
    pub operator_hint: Option<String>,
}

impl Default for DeadLetterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention: Some("30d".into()),
            operator_hint: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarConfig {
    pub name: String,
    pub timezone: Option<String>,
    pub rules: Vec<CalendarRuleConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarRuleConfig {
    pub kind: String, // "include" | "exclude"
    pub rule_type: String,
    pub args: Vec<String>,
}

/// Compile a Croniqfile AST into a RuntimeConfig.
pub fn compile(ast: &Croniqfile) -> RuntimeConfig {
    let mut server = ServerConfig {
        listen: ":4000".into(),
        data_dir: "./.data".into(),
        db: "sqlite".into(),
    };
    let mut pull_api = None;
    let mut observability = None;
    let mut mcp: Option<McpConfig> = None;
    let mut oidc: Option<OidcDslConfig> = None;
    let mut auth = AuthDslConfig::default();
    let mut policy = PolicyConfig::default();
    let mut alerts = AlertsConfig::default();
    let mut default_timezone: Option<String> = None;
    let mut default_timeout: Option<String> = None;
    let mut default_retry = RetryConfig::default();
    let mut default_dead_letter = DeadLetterConfig::default();
    let mut default_execution_mode = ExecutionMode::default();
    let mut default_catch_up = CatchUpPolicy::default();
    let mut default_queue_ttl: Option<String> = None;
    let mut default_max_queue_depth: Option<u32> = None;
    let mut jobs = Vec::new();
    let mut calendars = Vec::new();

    // Collect every `vars { … }` entry from the file up-front so placeholder
    // resolution does not depend on block ordering.
    let vars = collect_vars(ast);

    for item in &ast.items {
        match item {
            Item::Server(s) => {
                for d in &s.directives {
                    match d.key.value.as_str() {
                        "listen" => {
                            if let Some(v) = first_arg(d, &vars) {
                                server.listen = v;
                            }
                        }
                        "data_dir" => {
                            if let Some(v) = first_arg(d, &vars) {
                                server.data_dir = v;
                            }
                        }
                        "db" => {
                            if let Some(v) = first_arg(d, &vars) {
                                server.db = v;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Item::PullApi(p) => {
                let mut cfg = PullApiConfig {
                    listen: ":9443".into(),
                    auth: None,
                    lease_ttl: "60s".into(),
                };
                for d in &p.directives {
                    match d.key.value.as_str() {
                        "listen" => {
                            if let Some(v) = first_arg(d, &vars) {
                                cfg.listen = v;
                            }
                        }
                        "auth" => {
                            cfg.auth = Some(
                                d.args
                                    .iter()
                                    .map(|a| resolve_str(a, &vars))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            );
                        }
                        "lease_ttl" => {
                            if let Some(v) = first_arg(d, &vars) {
                                cfg.lease_ttl = v;
                            }
                        }
                        _ => {}
                    }
                }
                pull_api = Some(cfg);
            }
            Item::Vars(_) => {
                // Vars are pre-collected via `collect_vars`; nothing to do here.
            }
            Item::Defaults(d) => {
                for dob in &d.directives {
                    match dob {
                        DirectiveOrBlock::Directive(dir) => match dir.key.value.as_str() {
                            "timezone" => {
                                default_timezone = first_arg(dir, &vars);
                            }
                            "timeout" => {
                                default_timeout = first_arg(dir, &vars);
                            }
                            "execution_mode" => {
                                if let Some(v) = first_arg(dir, &vars) {
                                    default_execution_mode = match v.as_str() {
                                        "ephemeral" => ExecutionMode::Ephemeral,
                                        _ => ExecutionMode::Queued,
                                    };
                                }
                            }
                            "catch_up" => {
                                if let Some(v) = first_arg(dir, &vars) {
                                    default_catch_up = match v.as_str() {
                                        "latest" => CatchUpPolicy::Latest,
                                        "none" => CatchUpPolicy::None,
                                        _ => CatchUpPolicy::All,
                                    };
                                }
                            }
                            "queue_ttl" => {
                                default_queue_ttl = first_arg(dir, &vars).filter(|v| v != "none");
                            }
                            "max_queue_depth" => {
                                default_max_queue_depth =
                                    first_arg(dir, &vars).and_then(|v| v.parse().ok());
                            }
                            _ => {}
                        },
                        DirectiveOrBlock::Block(block) => match block.name.value.as_str() {
                            "retry" => {
                                default_retry = compile_retry_block(block, &vars);
                            }
                            "dead_letter" => {
                                default_dead_letter = compile_dead_letter_block(block, &vars);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
            Item::Calendar(cal) => {
                calendars.push(compile_calendar(cal, &vars));
            }
            Item::Job(job) => {
                jobs.push(compile_job(
                    job,
                    &JobDefaults {
                        timezone: default_timezone.clone(),
                        timeout: default_timeout.clone(),
                        retry: default_retry.clone(),
                        dead_letter: default_dead_letter.clone(),
                        execution_mode: default_execution_mode,
                        catch_up: default_catch_up,
                        queue_ttl: default_queue_ttl.clone(),
                        max_queue_depth: default_max_queue_depth,
                    },
                    &vars,
                ));
            }
            Item::Observability(obs) => {
                let mut obs_cfg = ObservabilityConfig::default();
                for block in &obs.sub_blocks {
                    match block.name.value.as_str() {
                        "log" => {
                            let mut log = LogConfig {
                                level: "info".into(),
                                format: "text".into(),
                                output: "stderr".into(),
                            };
                            for d in &block.directives {
                                if let DirectiveOrBlock::Directive(dir) = d {
                                    match dir.key.value.as_str() {
                                        "level" => {
                                            if let Some(v) = first_arg(dir, &vars) {
                                                log.level = v;
                                            }
                                        }
                                        "format" => {
                                            if let Some(v) = first_arg(dir, &vars) {
                                                log.format = v;
                                            }
                                        }
                                        "output" => {
                                            if let Some(v) = first_arg(dir, &vars) {
                                                log.output = v;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            obs_cfg.log = Some(log);
                        }
                        "metrics" => {
                            let mut metrics = MetricsConfig {
                                listen: ":9900".into(),
                                path: "/metrics".into(),
                            };
                            for d in &block.directives {
                                if let DirectiveOrBlock::Directive(dir) = d {
                                    match dir.key.value.as_str() {
                                        "listen" => {
                                            if let Some(v) = first_arg(dir, &vars) {
                                                metrics.listen = v;
                                            }
                                        }
                                        "path" => {
                                            if let Some(v) = first_arg(dir, &vars) {
                                                metrics.path = v;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            obs_cfg.metrics = Some(metrics);
                        }
                        _ => {}
                    }
                }
                observability = Some(obs_cfg);
            }
            Item::Mcp(m) => {
                let mut cfg = McpConfig::default();
                // Multiple `allowed_hosts` directives: last one wins. Matches
                // the existing `enabled` behaviour in this block — duplicate
                // directives across a single Croniqfile block are vanishingly
                // rare and a louder warning lives in `validate.rs`.
                for d in &m.directives {
                    match d.key.value.as_str() {
                        "enabled" => {
                            if let Some(a) = d.args.first() {
                                cfg.enabled =
                                    matches!(a.value.as_str(), "true" | "yes" | "1" | "on");
                            }
                        }
                        "allowed_hosts" => {
                            cfg.allowed_hosts = d
                                .args
                                .iter()
                                .map(|a| resolve_str(a, &vars))
                                .filter(|s| !s.is_empty())
                                .collect();
                        }
                        _ => {}
                    }
                }
                mcp = Some(cfg);
            }
            Item::Policy(p) => {
                for d in &p.directives {
                    if d.key.value.as_str() == "dsl_adopt_on_mutate"
                        && let Some(a) = d.args.first()
                    {
                        policy.dsl_adopt_on_mutate =
                            matches!(a.value.as_str(), "true" | "yes" | "1" | "on");
                    }
                }
            }
            Item::Oidc(o) => {
                let mut cfg = OidcDslConfig::default();
                for d in &o.directives {
                    let Some(a) = d.args.first() else { continue };
                    let v = resolve_str(a, &vars);
                    match d.key.value.as_str() {
                        "issuer" => cfg.issuer = Some(v),
                        "client_id" => cfg.client_id = Some(v),
                        "redirect_url" => cfg.redirect_url = Some(v),
                        "default_role" => cfg.default_role = Some(v),
                        "provider_name" => cfg.provider_name = Some(v),
                        "post_login_redirect" => cfg.post_login_redirect = Some(v),
                        _ => {}
                    }
                }
                oidc = Some(cfg);
            }
            Item::Auth(a) => {
                for nb in &a.sub_blocks {
                    if nb.name.value == "password" {
                        for dob in &nb.directives {
                            let DirectiveOrBlock::Directive(d) = dob else {
                                continue;
                            };
                            if d.key.value == "enabled"
                                && let Some(arg) = d.args.first()
                            {
                                let v = resolve_str(arg, &vars);
                                auth.password.enabled = parse_bool(&v);
                            }
                        }
                    }
                }
            }
            Item::Alerts(a) => {
                alerts = compile_alerts(a, &vars);
            }
            _ => {}
        }
    }

    RuntimeConfig {
        server,
        pull_api,
        observability,
        mcp,
        oidc,
        auth,
        policy,
        alerts,
        jobs,
        calendars,
    }
}

/// Parse a DSL boolean argument. Accepts the same liberal set as `policy {}`:
/// `true|yes|on|1` ⇒ true, `false|no|off|0` ⇒ false. Unknown values produce
/// `None` so the caller can keep its default.
fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Walk an `alerts { … }` AST block into a fully resolved
/// [`AlertsConfig`]. Unknown channel kinds become
/// [`ChannelKind::Unknown`] so rule references still resolve cleanly
/// — the evaluator logs and skips them at fire time. Rules that
/// reference a missing channel name are emitted as a compile-time
/// warning in the future (PR-2 wires that path) but for PR-1 the
/// reference is kept verbatim and a runtime warning fires.
fn compile_alerts(block: &AlertsBlock, vars: &HashMap<String, String>) -> AlertsConfig {
    let mut cfg = AlertsConfig::default();
    for sub in &block.sub_blocks {
        let qualifier = sub
            .qualifier
            .as_ref()
            .map(|q| resolve_str(q, vars))
            .unwrap_or_default();
        match sub.name.value.as_str() {
            "channel" => {
                if qualifier.is_empty() {
                    continue;
                }
                let kind = compile_channel_kind(sub, vars);
                cfg.channels.insert(
                    qualifier.clone(),
                    ChannelConfig {
                        name: qualifier,
                        kind,
                    },
                );
            }
            "rule" => {
                if qualifier.is_empty() {
                    continue;
                }
                if let Some(rule) = compile_rule(&qualifier, sub, vars) {
                    cfg.rules.push(rule);
                }
            }
            _ => {
                // Unknown sub-block kind — skip silently. The parser
                // already accepted it as a NamedBlock; future kinds
                // (e.g. `template`) extend this match without churn.
            }
        }
    }
    cfg
}

fn compile_channel_kind(block: &NamedBlock, vars: &HashMap<String, String>) -> ChannelKind {
    for dob in &block.directives {
        let DirectiveOrBlock::Directive(d) = dob else {
            continue;
        };
        match d.key.value.as_str() {
            "shell" => {
                if let Some(arg) = d.args.first() {
                    return ChannelKind::Shell {
                        command: resolve_str(arg, vars),
                    };
                }
                return ChannelKind::Unknown {
                    reason: "shell directive requires a command argument".into(),
                };
            }
            // Reserved for future PRs — recognised at compile time so the
            // evaluator can produce a clearer "not implemented in this
            // build" message than "unknown channel kind".
            "webhook" | "email" => {
                return ChannelKind::Unknown {
                    reason: format!(
                        "channel kind `{}` is not yet implemented (PR-1 of #140 ships shell only)",
                        d.key.value
                    ),
                };
            }
            _ => {}
        }
    }
    ChannelKind::Unknown {
        reason: "no channel kind directive (expected `shell`, `webhook`, or `email`)".into(),
    }
}

fn compile_rule(
    name: &str,
    block: &NamedBlock,
    vars: &HashMap<String, String>,
) -> Option<RuleConfig> {
    let mut trigger: Option<RuleTrigger> = None;
    let mut job_key_glob = "*".to_string();
    let mut min_attempts: u32 = 1;
    let mut dead_letter_only = false;
    let mut throttle: Option<String> = None;
    let mut channels: Vec<String> = Vec::new();

    for dob in &block.directives {
        let DirectiveOrBlock::Directive(d) = dob else {
            continue;
        };
        match d.key.value.as_str() {
            "when" => {
                if let Some(arg) = d.args.first() {
                    let v = resolve_str(arg, vars);
                    trigger = match v.as_str() {
                        "job_failed" => Some(RuleTrigger::JobFailed),
                        // `job_sla_missed` and any other future
                        // trigger are reserved and silently produce
                        // `None`, which causes the rule to be dropped
                        // below. Operators get a runtime warning when
                        // the evaluator notices the dropped rule on
                        // the next reload (TBD in PR-4).
                        _ => None,
                    };
                }
            }
            "job_key" => {
                if let Some(arg) = d.args.first() {
                    job_key_glob = resolve_str(arg, vars);
                }
            }
            "min_attempts" => {
                if let Some(arg) = d.args.first() {
                    let v = resolve_str(arg, vars);
                    if let Ok(n) = v.parse::<u32>() {
                        min_attempts = n.max(1);
                    }
                }
            }
            "dead_letter" => {
                if let Some(arg) = d.args.first() {
                    let v = resolve_str(arg, vars);
                    dead_letter_only = matches!(v.as_str(), "true" | "yes" | "1" | "on");
                }
            }
            "throttle" => {
                if let Some(arg) = d.args.first() {
                    throttle = Some(resolve_str(arg, vars));
                }
            }
            "channels" => {
                for arg in &d.args {
                    let v = resolve_str(arg, vars);
                    if !v.is_empty() {
                        channels.push(v);
                    }
                }
            }
            _ => {}
        }
    }

    let trigger = trigger?;
    Some(RuleConfig {
        name: name.to_string(),
        trigger,
        job_key_glob,
        min_attempts,
        dead_letter_only,
        throttle,
        channels,
    })
}

/// Bundled defaults passed to `compile_job` to avoid too many parameters.
struct JobDefaults {
    timezone: Option<String>,
    timeout: Option<String>,
    retry: RetryConfig,
    dead_letter: DeadLetterConfig,
    execution_mode: ExecutionMode,
    catch_up: CatchUpPolicy,
    queue_ttl: Option<String>,
    max_queue_depth: Option<u32>,
}

fn compile_job(
    job: &JobBlock,
    defaults: &JobDefaults,
    vars: &HashMap<String, String>,
) -> JobConfig {
    let schedule = job
        .schedule
        .as_ref()
        .map(|s| CompiledSchedule::from_ast(&s.kind))
        .unwrap_or(CompiledSchedule::Disabled);

    let schedule_summary = schedule.summary();

    // Extract schedule options
    let mut timezone = defaults.timezone.clone();
    let mut calendar = None;
    let mut not_before = None;
    let mut not_after = None;

    if let Some(ref sched) = job.schedule {
        for opt in &sched.options {
            match opt.key.value.as_str() {
                "timezone" => timezone = first_arg(opt, vars),
                "calendar" => calendar = first_arg(opt, vars),
                "not_before" => not_before = first_arg(opt, vars),
                "not_after" => not_after = first_arg(opt, vars),
                _ => {}
            }
        }
    }

    // Extract job-level directives
    let mut description = None;
    let mut window = None;
    let mut runner = RunnerConfig::default();
    let mut retry = defaults.retry.clone();
    let mut timeout = defaults.timeout.clone();
    let mut dead_letter = defaults.dead_letter.clone();
    let mut metadata = HashMap::new();
    // Schedule-prefix mode takes precedence over directive and defaults.
    let mut execution_mode = match job.schedule.as_ref().and_then(|s| s.mode) {
        Some(ast::ScheduleMode::Ephemeral) => ExecutionMode::Ephemeral,
        Some(ast::ScheduleMode::Queued) => ExecutionMode::Queued,
        None => defaults.execution_mode,
    };
    let mut catch_up = defaults.catch_up;
    let mut queue_ttl = defaults.queue_ttl.clone();
    let mut max_queue_depth = defaults.max_queue_depth;
    let mut tags: Vec<String> = Vec::new();

    for dob in &job.directives {
        match dob {
            DirectiveOrBlock::Directive(d) => match d.key.value.as_str() {
                "description" => description = first_arg(d, vars),
                "timeout" => timeout = first_arg(d, vars),
                "window" => window = first_arg(d, vars),
                "execution_mode" => {
                    if let Some(v) = first_arg(d, vars) {
                        execution_mode = match v.as_str() {
                            "ephemeral" => ExecutionMode::Ephemeral,
                            _ => ExecutionMode::Queued,
                        };
                    }
                }
                "catch_up" => {
                    if let Some(v) = first_arg(d, vars) {
                        catch_up = match v.as_str() {
                            "latest" => CatchUpPolicy::Latest,
                            "none" => CatchUpPolicy::None,
                            _ => CatchUpPolicy::All,
                        };
                    }
                }
                "queue_ttl" => {
                    queue_ttl = first_arg(d, vars).filter(|v| v != "none");
                }
                "max_queue_depth" => {
                    max_queue_depth = first_arg(d, vars).and_then(|v| v.parse().ok());
                }
                "tags" => {
                    for a in &d.args {
                        let v = resolve_str(a, vars);
                        if !v.is_empty() && !tags.contains(&v) {
                            tags.push(v);
                        }
                    }
                }
                _ => {}
            },
            DirectiveOrBlock::Block(block) => match block.name.value.as_str() {
                "runner" => match block.qualifier.as_ref().map(|q| q.value.as_str()) {
                    None => runner = compile_runner_block(block, vars),
                    Some("shell") | Some("exec") => {
                        if let Some(exec) = compile_runner_exec_block(block, vars)
                            && let Ok(json) = serde_json::to_string(&exec)
                        {
                            metadata.insert(RUNNER_EXEC_METADATA_KEY.into(), json);
                        }
                    }
                    Some(_) => {
                        // Unknown qualifier — leave to validate.rs to surface a
                        // diagnostic so unrecognised runner types don't silently
                        // compile to a bare placement-constraint block.
                    }
                },
                "retry" => retry = compile_retry_block(block, vars),
                "dead_letter" => dead_letter = compile_dead_letter_block(block, vars),
                "metadata" => {
                    for inner in &block.directives {
                        if let DirectiveOrBlock::Directive(d) = inner
                            && let Some(v) = first_arg(d, vars)
                        {
                            metadata.insert(d.key.value.clone(), v);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    // Ephemeral mode implies: no queue persistence, no catch-up, no retry, no dead-letter.
    // Silently override queue-only settings to avoid confusing behaviour.
    if execution_mode == ExecutionMode::Ephemeral {
        catch_up = CatchUpPolicy::None;
        queue_ttl = None;
        max_queue_depth = Some(1);
    }

    JobConfig {
        key: job.key.raw.clone(),
        namespace: job.key.namespace.clone(),
        name: job.key.name.clone(),
        variant: job.key.variant.clone(),
        description,
        schedule,
        schedule_summary,
        timezone,
        calendar,
        window,
        not_before,
        not_after,
        runner,
        retry,
        timeout,
        dead_letter,
        metadata,
        execution_mode,
        catch_up,
        queue_ttl,
        max_queue_depth,
        tags,
    }
}

fn compile_runner_block(block: &NamedBlock, vars: &HashMap<String, String>) -> RunnerConfig {
    let mut cfg = RunnerConfig::default();
    for dob in &block.directives {
        if let DirectiveOrBlock::Directive(d) = dob {
            let val = first_arg(d, vars).unwrap_or_default();
            match d.key.value.as_str() {
                "require" => cfg.require.push(val),
                "prefer" => cfg.prefer.push(val),
                "exclude" => cfg.exclude.push(val),
                "sticky" => cfg.sticky = true,
                _ => {}
            }
        }
    }
    cfg
}

/// Compile a qualified `runner shell { ... }` or `runner exec { ... }` block
/// into a [`RunnerExec`]. Returns `None` if required directives are missing
/// (`command` for shell, `args` for exec) — `validate.rs` surfaces the
/// diagnostic so that the user-facing error has a span to point at.
fn compile_runner_exec_block(
    block: &NamedBlock,
    vars: &HashMap<String, String>,
) -> Option<RunnerExec> {
    let kind = block.qualifier.as_ref()?.value.clone();

    let mut command: Option<String> = None;
    let mut argv: Vec<String> = Vec::new();
    let mut workdir: Option<String> = None;
    let mut user: Option<String> = None;
    let mut env: HashMap<String, String> = HashMap::new();

    for dob in &block.directives {
        match dob {
            DirectiveOrBlock::Directive(d) => match d.key.value.as_str() {
                "command" => command = first_arg(d, vars),
                "args" => {
                    argv = d.args.iter().map(|a| resolve_str(a, vars)).collect();
                }
                "workdir" => workdir = first_arg(d, vars),
                "user" => user = first_arg(d, vars),
                _ => {}
            },
            DirectiveOrBlock::Block(inner) if inner.name.value == "env" => {
                for entry in &inner.directives {
                    if let DirectiveOrBlock::Directive(d) = entry
                        && let Some(val) = first_arg(d, vars)
                    {
                        env.insert(d.key.value.clone(), val);
                    }
                }
            }
            _ => {}
        }
    }

    match kind.as_str() {
        "shell" => command.map(|command| RunnerExec::Shell {
            command,
            workdir,
            user,
            env,
        }),
        "exec" => {
            if argv.is_empty() {
                None
            } else {
                Some(RunnerExec::Exec {
                    argv,
                    workdir,
                    user,
                    env,
                })
            }
        }
        _ => None,
    }
}

fn compile_retry_block(block: &NamedBlock, vars: &HashMap<String, String>) -> RetryConfig {
    let strategy = block
        .qualifier
        .as_ref()
        .map(|q| q.value.clone())
        .unwrap_or_else(|| "exponential".into());

    let mut cfg = RetryConfig {
        strategy,
        ..Default::default()
    };

    for dob in &block.directives {
        if let DirectiveOrBlock::Directive(d) = dob {
            let val = first_arg(d, vars).unwrap_or_default();
            match d.key.value.as_str() {
                "max_attempts" => cfg.max_attempts = val.parse().unwrap_or(3),
                "base" => cfg.base = Some(val),
                "cap" => cfg.cap = Some(val),
                "delay" => cfg.delay = Some(val),
                "step" => cfg.step = Some(val),
                "jitter" => cfg.jitter = val.parse().ok(),
                _ => {}
            }
        }
    }
    cfg
}

fn compile_dead_letter_block(
    block: &NamedBlock,
    vars: &HashMap<String, String>,
) -> DeadLetterConfig {
    let mut cfg = DeadLetterConfig::default();
    for dob in &block.directives {
        if let DirectiveOrBlock::Directive(d) = dob {
            match d.key.value.as_str() {
                "retention" => cfg.retention = first_arg(d, vars),
                "operator_hint" => cfg.operator_hint = first_arg(d, vars),
                _ => {}
            }
        }
    }
    cfg
}

fn compile_calendar(cal: &CalendarBlock, vars: &HashMap<String, String>) -> CalendarConfig {
    let mut timezone = None;
    let mut rules = Vec::new();

    for rule in &cal.rules {
        if rule.rule_type.value == "timezone" {
            // Special case: timezone is stored as a rule in AST
            timezone = rule.args.first().map(|a| resolve_str(a, vars));
            continue;
        }
        let kind = match rule.kind {
            CalendarRuleKind::Include => "include",
            CalendarRuleKind::Exclude => "exclude",
        };
        rules.push(CalendarRuleConfig {
            kind: kind.to_string(),
            rule_type: rule.rule_type.value.clone(),
            args: rule.args.iter().map(|a| resolve_str(a, vars)).collect(),
        });
    }

    CalendarConfig {
        name: cal.name.value.clone(),
        timezone,
        rules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    #[test]
    fn compile_basic() {
        let ast = Parser::parse(
            r#"
            server { listen :4000; db sqlite }
            job etl:sync { every 15 minutes; timeout 10m }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.server.listen, ":4000");
        assert_eq!(cfg.jobs.len(), 1);
        assert_eq!(cfg.jobs[0].key, "etl:sync");
        assert_eq!(cfg.jobs[0].schedule_summary, "every 15 minutes");
    }

    #[test]
    fn compile_with_defaults() {
        let ast = Parser::parse(
            r#"
            defaults {
                timezone Europe/Vienna
                timeout 5m
            }
            job etl:sync { every 15 minutes }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.jobs[0].timezone.as_deref(), Some("Europe/Vienna"));
        assert_eq!(cfg.jobs[0].timeout.as_deref(), Some("5m"));
    }

    #[test]
    fn compile_job_overrides_defaults() {
        let ast = Parser::parse(
            r#"
            defaults { timeout 5m }
            job etl:sync { every 15 minutes; timeout 10m }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.jobs[0].timeout.as_deref(), Some("10m"));
    }

    #[test]
    fn vars_placeholder_resolves_in_calendar_timezone() {
        let ast = Parser::parse(
            r#"
            vars { default_tz Europe/Vienna }
            calendar business-days {
                timezone {vars.default_tz}
                include weekly monday tuesday wednesday thursday friday
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.calendars.len(), 1);
        assert_eq!(
            cfg.calendars[0].timezone.as_deref(),
            Some("Europe/Vienna"),
            "calendar timezone must resolve {{vars.default_tz}} to its value"
        );
    }

    #[test]
    fn vars_placeholder_resolves_in_defaults_and_job_timezone() {
        let ast = Parser::parse(
            r#"
            vars { default_tz Europe/Vienna; alt_tz UTC }
            defaults { timezone {vars.default_tz} }
            job etl:sync { every 15 minutes { timezone {vars.alt_tz} } }
            job etl:report { every 1 hours }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        // Job with explicit schedule timezone option uses its own var.
        let sync = cfg.jobs.iter().find(|j| j.key == "etl:sync").unwrap();
        assert_eq!(sync.timezone.as_deref(), Some("UTC"));
        // Job without override inherits the resolved defaults timezone.
        let report = cfg.jobs.iter().find(|j| j.key == "etl:report").unwrap();
        assert_eq!(report.timezone.as_deref(), Some("Europe/Vienna"));
    }

    #[test]
    fn vars_resolution_works_when_vars_block_is_after_consumer() {
        // collect_vars runs as a pre-pass, so block ordering must not matter.
        let ast = Parser::parse(
            r#"
            calendar business-days { timezone {vars.default_tz} }
            vars { default_tz Europe/Vienna }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.calendars[0].timezone.as_deref(), Some("Europe/Vienna"));
    }

    #[test]
    fn unresolved_vars_placeholder_falls_back_to_raw_value() {
        // Missing var → raw `vars.X` is preserved (does not panic).
        let ast = Parser::parse(
            r#"
            calendar holidays { timezone {vars.missing} }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.calendars[0].timezone.as_deref(), Some("vars.missing"));
    }

    fn exec_payload(cfg: &RuntimeConfig, key: &str) -> RunnerExec {
        let job = cfg.jobs.iter().find(|j| j.key == key).expect("job present");
        let raw = job
            .metadata
            .get(RUNNER_EXEC_METADATA_KEY)
            .expect("__runner_exec metadata stamped");
        serde_json::from_str::<RunnerExec>(raw).expect("metadata is valid RunnerExec JSON")
    }

    #[test]
    fn runner_shell_block_compiles_to_metadata_stamp() {
        let ast = Parser::parse(
            r#"
            job ops:dump {
                every day at 03:00
                runner shell {
                    command "pg_dump -U app app > /backups/app.sql"
                    workdir /opt
                    user 0
                    env { PGPASSWORD secret-stuff; LANG en_US.UTF-8 }
                }
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let exec = exec_payload(&cfg, "ops:dump");
        match exec {
            RunnerExec::Shell {
                command,
                workdir,
                user,
                env,
            } => {
                assert_eq!(command, "pg_dump -U app app > /backups/app.sql");
                assert_eq!(workdir.as_deref(), Some("/opt"));
                assert_eq!(user.as_deref(), Some("0"));
                assert_eq!(
                    env.get("PGPASSWORD").map(String::as_str),
                    Some("secret-stuff")
                );
                assert_eq!(env.get("LANG").map(String::as_str), Some("en_US.UTF-8"));
            }
            other => panic!("expected Shell, got {other:?}"),
        }
    }

    #[test]
    fn runner_exec_block_compiles_argv() {
        let ast = Parser::parse(
            r#"
            job ops:rotate {
                every 1 hour
                runner exec {
                    args /usr/local/bin/logrotate /etc/logrotate.conf
                }
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let exec = exec_payload(&cfg, "ops:rotate");
        match exec {
            RunnerExec::Exec { argv, env, .. } => {
                assert_eq!(
                    argv,
                    vec!["/usr/local/bin/logrotate", "/etc/logrotate.conf"]
                );
                assert!(env.is_empty());
            }
            other => panic!("expected Exec, got {other:?}"),
        }
    }

    #[test]
    fn runner_placement_and_exec_blocks_coexist() {
        let ast = Parser::parse(
            r#"
            job ops:dump {
                every day at 03:00
                runner { require shell-runner; sticky }
                runner shell { command "echo hello" }
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let job = cfg.jobs.iter().find(|j| j.key == "ops:dump").unwrap();
        assert!(job.runner.require.contains(&"shell-runner".to_string()));
        assert!(job.runner.sticky);
        assert!(job.metadata.contains_key(RUNNER_EXEC_METADATA_KEY));
    }

    #[test]
    fn runner_shell_without_command_does_not_stamp() {
        // Compile is best-effort; validate.rs surfaces the error. We just
        // need to make sure we do not stamp a half-baked payload.
        let ast = Parser::parse(
            r#"
            job ops:broken {
                every 1 hour
                runner shell { workdir /opt }
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let job = cfg.jobs.iter().find(|j| j.key == "ops:broken").unwrap();
        assert!(!job.metadata.contains_key(RUNNER_EXEC_METADATA_KEY));
    }

    #[test]
    fn runner_exec_resolves_placeholders_in_argv_and_env() {
        let ast = Parser::parse(
            r#"
            vars { backup_dir /var/backups }
            job ops:dump {
                every 1 hour
                runner exec {
                    args /usr/bin/pg_dump -f {vars.backup_dir} app
                    env { TARGET {vars.backup_dir} }
                }
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let exec = exec_payload(&cfg, "ops:dump");
        match exec {
            RunnerExec::Exec { argv, env, .. } => {
                assert_eq!(
                    argv,
                    vec!["/usr/bin/pg_dump", "-f", "/var/backups", "app"],
                    "argv should resolve isolated {{vars.X}} placeholders"
                );
                assert_eq!(env.get("TARGET").map(String::as_str), Some("/var/backups"));
            }
            other => panic!("expected Exec, got {other:?}"),
        }
    }

    #[test]
    fn runner_shell_command_with_double_braces_is_stamped() {
        // Reproducer for issue #89: {{...}} inside a quoted command string
        // (Docker/Go-template format) must be passed verbatim to the runner
        // and must not confuse the block tokenizer into dropping the command.
        let ast = Parser::parse(
            r#"
            job test:docker-format-string {
                every 1 hour
                runner { require shell-runner }
                runner shell {
                    command "docker ps --format '{{.Image}}'"
                }
            }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let exec = exec_payload(&cfg, "test:docker-format-string");
        match exec {
            RunnerExec::Shell { command, .. } => {
                assert_eq!(
                    command, "docker ps --format '{{.Image}}'",
                    "{{...}} must survive the DSL round-trip verbatim"
                );
            }
            other => panic!("expected Shell, got {other:?}"),
        }
    }

    // ─── mcp { allowed_hosts ... } (issue #114) ───

    #[test]
    fn compile_mcp_block_without_allowed_hosts_yields_empty_vec() {
        let ast = Parser::parse("mcp { enabled true }").unwrap();
        let cfg = compile(&ast);
        let mcp = cfg.mcp.expect("mcp block compiled");
        assert!(mcp.enabled);
        assert!(
            mcp.allowed_hosts.is_empty(),
            "absent `allowed_hosts` directive must leave the Vec empty so the server \
             falls through to rmcp's loopback default"
        );
    }

    #[test]
    fn compile_mcp_allowed_hosts_collects_all_args() {
        // Includes the IPv6 literal `::1` to lock its lexing as a single
        // bare ident — `:` is part of `is_ident_char` (lexer.rs).
        let ast = Parser::parse(
            r#"mcp {
                enabled true
                allowed_hosts cron.internal admin.example.com 127.0.0.1 ::1
            }"#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let mcp = cfg.mcp.expect("mcp block compiled");
        assert!(mcp.enabled);
        assert_eq!(
            mcp.allowed_hosts,
            vec![
                "cron.internal".to_string(),
                "admin.example.com".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ]
        );
    }

    #[test]
    fn compile_mcp_allowed_hosts_resolves_placeholders() {
        let ast = Parser::parse(
            r#"
            vars { proxy_host edge.internal }
            mcp {
                allowed_hosts {vars.proxy_host} localhost
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let mcp = cfg.mcp.expect("mcp block compiled");
        assert_eq!(
            mcp.allowed_hosts,
            vec!["edge.internal".to_string(), "localhost".to_string()]
        );
    }

    #[test]
    fn compile_mcp_duplicate_allowed_hosts_last_wins() {
        let ast = Parser::parse(
            r#"mcp {
                allowed_hosts first.example.com
                allowed_hosts second.example.com third.example.com
            }"#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let mcp = cfg.mcp.expect("mcp block compiled");
        assert_eq!(
            mcp.allowed_hosts,
            vec![
                "second.example.com".to_string(),
                "third.example.com".to_string(),
            ],
            "second `allowed_hosts` directive must replace the first — matches \
             the `enabled` directive behaviour in the same block"
        );
    }

    // ─── #138 auth block ─────────────────────────────────────────

    #[test]
    fn compile_auth_block_default_unset() {
        let ast = Parser::parse("server { listen :4000 }").unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.auth.password.enabled, None);
    }

    #[test]
    fn compile_auth_password_disabled() {
        let ast = Parser::parse(
            r#"
            auth {
                password { enabled false }
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.auth.password.enabled, Some(false));
    }

    #[test]
    fn compile_auth_password_explicit_true() {
        let ast = Parser::parse(
            r#"
            auth {
                password { enabled true }
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.auth.password.enabled, Some(true));
    }

    #[test]
    fn compile_auth_password_accepts_aliases() {
        // Mirror the policy block — accept the same loose boolean syntax.
        for (value, want) in [
            ("yes", true),
            ("on", true),
            ("1", true),
            ("no", false),
            ("off", false),
            ("0", false),
        ] {
            let src = format!("auth {{ password {{ enabled {value} }} }}");
            let ast = Parser::parse(&src).unwrap();
            let cfg = compile(&ast);
            assert_eq!(
                cfg.auth.password.enabled,
                Some(want),
                "value {value:?} should map to {want}"
            );
        }
    }

    // ─── #140 alerts block ─────────────────────────────────────────

    #[test]
    fn compile_alerts_empty_when_absent() {
        let ast = Parser::parse("server { listen :4000 }").unwrap();
        let cfg = compile(&ast);
        assert!(cfg.alerts.channels.is_empty());
        assert!(cfg.alerts.rules.is_empty());
    }

    #[test]
    fn compile_alerts_shell_channel_and_rule() {
        let ast = Parser::parse(
            r#"
            alerts {
                channel "ops-paging" {
                    shell "/usr/local/bin/page-oncall.sh"
                }
                rule "billing-fail" {
                    when job_failed
                    job_key "billing:*"
                    min_attempts 2
                    dead_letter true
                    throttle 10m
                    channels "ops-paging"
                }
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let ch = cfg
            .alerts
            .channels
            .get("ops-paging")
            .expect("channel parsed");
        assert_eq!(ch.name, "ops-paging");
        match &ch.kind {
            ChannelKind::Shell { command } => {
                assert_eq!(command, "/usr/local/bin/page-oncall.sh");
            }
            other => panic!("expected Shell, got {other:?}"),
        }

        assert_eq!(cfg.alerts.rules.len(), 1);
        let rule = &cfg.alerts.rules[0];
        assert_eq!(rule.name, "billing-fail");
        assert!(matches!(rule.trigger, RuleTrigger::JobFailed));
        assert_eq!(rule.job_key_glob, "billing:*");
        assert_eq!(rule.min_attempts, 2);
        assert!(rule.dead_letter_only);
        assert_eq!(rule.throttle.as_deref(), Some("10m"));
        assert_eq!(rule.channels, vec!["ops-paging"]);
    }

    #[test]
    fn compile_alerts_rule_defaults() {
        // No `job_key`, `min_attempts`, `dead_letter`, or `throttle`
        // directives — defaults must apply.
        let ast = Parser::parse(
            r#"
            alerts {
                channel "anything" { shell "/bin/true" }
                rule "any-failure" {
                    when job_failed
                    channels "anything"
                }
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let rule = &cfg.alerts.rules[0];
        assert_eq!(rule.job_key_glob, "*");
        assert_eq!(rule.min_attempts, 1);
        assert!(!rule.dead_letter_only);
        assert!(rule.throttle.is_none());
    }

    #[test]
    fn compile_alerts_unknown_trigger_drops_rule() {
        // `job_sla_missed` is reserved but not implemented in PR-1 —
        // rules with unsupported triggers must not silently match
        // every failure. Dropping the rule (with a future warning
        // path) is safer than synthesising a default trigger.
        let ast = Parser::parse(
            r#"
            alerts {
                channel "x" { shell "/bin/true" }
                rule "future-rule" {
                    when job_sla_missed
                    channels "x"
                }
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert!(
            cfg.alerts.rules.is_empty(),
            "unsupported trigger drops the rule"
        );
        // Channel still compiles — rule references aren't a hard error.
        assert!(cfg.alerts.channels.contains_key("x"));
    }

    #[test]
    fn compile_alerts_unknown_channel_kind_is_unknown_variant() {
        // `webhook` is recognised at compile time (so the error
        // message is specific) but produces ChannelKind::Unknown —
        // PR-2 will ship the real implementation.
        let ast = Parser::parse(
            r#"
            alerts {
                channel "slack" {
                    webhook https://hooks.slack.com/services/xxx
                }
            }
            "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        let ch = cfg.alerts.channels.get("slack").expect("channel parsed");
        match &ch.kind {
            ChannelKind::Unknown { reason } => {
                assert!(reason.contains("webhook"), "reason mentions kind: {reason}");
            }
            other => panic!("expected Unknown for webhook in PR-1, got {other:?}"),
        }
    }

    #[test]
    fn compile_alerts_format_roundtrip() {
        // The formatter must preserve channel + rule blocks so an
        // operator's editor format-on-save doesn't churn the file.
        let src = r#"alerts {
    channel "ops" {
        shell "/bin/true"
    }
    rule "fail" {
        when job_failed
        channels "ops"
    }
}
"#;
        let ast = Parser::parse(src).unwrap();
        let formatted = crate::format::format(&ast);
        let ast2 = Parser::parse(&formatted).unwrap();
        let cfg2 = compile(&ast2);
        assert!(cfg2.alerts.channels.contains_key("ops"));
        assert_eq!(cfg2.alerts.rules.len(), 1);
        assert_eq!(cfg2.alerts.rules[0].name, "fail");
    }
}
