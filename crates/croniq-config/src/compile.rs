//! Compiles a Croniqfile AST into a RuntimeConfig with resolved defaults and placeholders.

use crate::ast::{self, *};
use crate::schedule::CompiledSchedule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    let mut vars = HashMap::new();

    for item in &ast.items {
        match item {
            Item::Server(s) => {
                for d in &s.directives {
                    match d.key.value.as_str() {
                        "listen" => {
                            if let Some(a) = d.args.first() {
                                server.listen = a.value.clone();
                            }
                        }
                        "data_dir" => {
                            if let Some(a) = d.args.first() {
                                server.data_dir = a.value.clone();
                            }
                        }
                        "db" => {
                            if let Some(a) = d.args.first() {
                                server.db = a.value.clone();
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
                            if let Some(a) = d.args.first() {
                                cfg.listen = a.value.clone();
                            }
                        }
                        "auth" => {
                            cfg.auth = Some(
                                d.args
                                    .iter()
                                    .map(|a| a.value.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            );
                        }
                        "lease_ttl" => {
                            if let Some(a) = d.args.first() {
                                cfg.lease_ttl = a.value.clone();
                            }
                        }
                        _ => {}
                    }
                }
                pull_api = Some(cfg);
            }
            Item::Vars(v) => {
                for d in &v.entries {
                    if let Some(val) = d.args.first() {
                        vars.insert(d.key.value.clone(), val.value.clone());
                    }
                }
            }
            Item::Defaults(d) => {
                for dob in &d.directives {
                    match dob {
                        DirectiveOrBlock::Directive(dir) => match dir.key.value.as_str() {
                            "timezone" => {
                                default_timezone = dir.args.first().map(|a| a.value.clone());
                            }
                            "timeout" => {
                                default_timeout = dir.args.first().map(|a| a.value.clone());
                            }
                            "execution_mode" => {
                                if let Some(v) = dir.args.first() {
                                    default_execution_mode = match v.value.as_str() {
                                        "ephemeral" => ExecutionMode::Ephemeral,
                                        _ => ExecutionMode::Queued,
                                    };
                                }
                            }
                            "catch_up" => {
                                if let Some(v) = dir.args.first() {
                                    default_catch_up = match v.value.as_str() {
                                        "latest" => CatchUpPolicy::Latest,
                                        "none" => CatchUpPolicy::None,
                                        _ => CatchUpPolicy::All,
                                    };
                                }
                            }
                            "queue_ttl" => {
                                default_queue_ttl = dir
                                    .args
                                    .first()
                                    .map(|a| a.value.clone())
                                    .filter(|v| v != "none");
                            }
                            "max_queue_depth" => {
                                default_max_queue_depth =
                                    dir.args.first().and_then(|a| a.value.parse().ok());
                            }
                            _ => {}
                        },
                        DirectiveOrBlock::Block(block) => match block.name.value.as_str() {
                            "retry" => {
                                default_retry = compile_retry_block(block);
                            }
                            "dead_letter" => {
                                default_dead_letter = compile_dead_letter_block(block);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
            Item::Calendar(cal) => {
                calendars.push(compile_calendar(cal));
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
                                            if let Some(a) = dir.args.first() {
                                                log.level = a.value.clone();
                                            }
                                        }
                                        "format" => {
                                            if let Some(a) = dir.args.first() {
                                                log.format = a.value.clone();
                                            }
                                        }
                                        "output" => {
                                            if let Some(a) = dir.args.first() {
                                                log.output = a.value.clone();
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
                                            if let Some(a) = dir.args.first() {
                                                metrics.listen = a.value.clone();
                                            }
                                        }
                                        "path" => {
                                            if let Some(a) = dir.args.first() {
                                                metrics.path = a.value.clone();
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

fn compile_job(job: &JobBlock, defaults: &JobDefaults) -> JobConfig {
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
                "timezone" => timezone = opt.args.first().map(|a| a.value.clone()),
                "calendar" => calendar = opt.args.first().map(|a| a.value.clone()),
                "not_before" => not_before = opt.args.first().map(|a| a.value.clone()),
                "not_after" => not_after = opt.args.first().map(|a| a.value.clone()),
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
                "description" => description = d.args.first().map(|a| a.value.clone()),
                "timeout" => timeout = d.args.first().map(|a| a.value.clone()),
                "window" => window = d.args.first().map(|a| a.value.clone()),
                "execution_mode" => {
                    if let Some(v) = d.args.first() {
                        execution_mode = match v.value.as_str() {
                            "ephemeral" => ExecutionMode::Ephemeral,
                            _ => ExecutionMode::Queued,
                        };
                    }
                }
                "catch_up" => {
                    if let Some(v) = d.args.first() {
                        catch_up = match v.value.as_str() {
                            "latest" => CatchUpPolicy::Latest,
                            "none" => CatchUpPolicy::None,
                            _ => CatchUpPolicy::All,
                        };
                    }
                }
                "queue_ttl" => {
                    queue_ttl = d
                        .args
                        .first()
                        .map(|a| a.value.clone())
                        .filter(|v| v != "none");
                }
                "max_queue_depth" => {
                    max_queue_depth = d.args.first().and_then(|a| a.value.parse().ok());
                }
                _ => {}
            },
            DirectiveOrBlock::Block(block) => match block.name.value.as_str() {
                "runner" => runner = compile_runner_block(block),
                "retry" => retry = compile_retry_block(block),
                "dead_letter" => dead_letter = compile_dead_letter_block(block),
                "metadata" => {
                    for inner in &block.directives {
                        if let DirectiveOrBlock::Directive(d) = inner
                            && let Some(v) = d.args.first()
                        {
                            metadata.insert(d.key.value.clone(), v.value.clone());
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

fn compile_runner_block(block: &NamedBlock) -> RunnerConfig {
    let mut cfg = RunnerConfig::default();
    for dob in &block.directives {
        if let DirectiveOrBlock::Directive(d) = dob {
            let val = d.args.first().map(|a| a.value.clone()).unwrap_or_default();
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

fn compile_retry_block(block: &NamedBlock) -> RetryConfig {
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
            let val = d.args.first().map(|a| a.value.as_str()).unwrap_or("");
            match d.key.value.as_str() {
                "max_attempts" => cfg.max_attempts = val.parse().unwrap_or(3),
                "base" => cfg.base = Some(val.to_string()),
                "cap" => cfg.cap = Some(val.to_string()),
                "delay" => cfg.delay = Some(val.to_string()),
                "step" => cfg.step = Some(val.to_string()),
                "jitter" => cfg.jitter = val.parse().ok(),
                _ => {}
            }
        }
    }
    cfg
}

fn compile_dead_letter_block(block: &NamedBlock) -> DeadLetterConfig {
    let mut cfg = DeadLetterConfig::default();
    for dob in &block.directives {
        if let DirectiveOrBlock::Directive(d) = dob {
            match d.key.value.as_str() {
                "retention" => cfg.retention = d.args.first().map(|a| a.value.clone()),
                "operator_hint" => cfg.operator_hint = d.args.first().map(|a| a.value.clone()),
                _ => {}
            }
        }
    }
    cfg
}

fn compile_calendar(cal: &CalendarBlock) -> CalendarConfig {
    let mut timezone = None;
    let mut rules = Vec::new();

    for rule in &cal.rules {
        if rule.rule_type.value == "timezone" {
            // Special case: timezone is stored as a rule in AST
            timezone = rule.args.first().map(|a| a.value.clone());
            continue;
        }
        let kind = match rule.kind {
            CalendarRuleKind::Include => "include",
            CalendarRuleKind::Exclude => "exclude",
        };
        rules.push(CalendarRuleConfig {
            kind: kind.to_string(),
            rule_type: rule.rule_type.value.clone(),
            args: rule.args.iter().map(|a| a.value.clone()).collect(),
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
}
