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
    /// Server-wide opt-in flags. Absent block ⇒ all defaults (deny).
    pub policy: PolicyConfig,
    pub jobs: Vec<JobConfig>,
    pub calendars: Vec<CalendarConfig>,
}

/// HTTP MCP-server configuration. Absent block ⇒ default (enabled).
#[derive(Debug, Clone, Serialize)]
pub struct McpConfig {
    pub enabled: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { enabled: true }
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
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunnerConfig {
    pub require: Vec<String>,
    pub prefer: Vec<String>,
    pub exclude: Vec<String>,
    pub sticky: bool,
}

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
    let mut policy = PolicyConfig::default();
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
                for d in &m.directives {
                    if d.key.value.as_str() == "enabled"
                        && let Some(a) = d.args.first()
                    {
                        cfg.enabled = matches!(a.value.as_str(), "true" | "yes" | "1" | "on");
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
            _ => {}
        }
    }

    RuntimeConfig {
        server,
        pull_api,
        observability,
        mcp,
        policy,
        jobs,
        calendars,
    }
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
                _ => {}
            },
            DirectiveOrBlock::Block(block) => match block.name.value.as_str() {
                "runner" => runner = compile_runner_block(block, vars),
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
}
