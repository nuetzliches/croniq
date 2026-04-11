//! Compiles a Croniqfile AST into a RuntimeConfig with resolved defaults and placeholders.

use crate::ast::*;
use crate::schedule::CompiledSchedule;
use serde::Serialize;
use std::collections::HashMap;

/// Fully resolved runtime configuration.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeConfig {
    pub server: ServerConfig,
    pub pull_api: Option<PullApiConfig>,
    pub observability: Option<ObservabilityConfig>,
    pub jobs: Vec<JobConfig>,
    pub calendars: Vec<CalendarConfig>,
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
        listen: ":8080".into(),
        data_dir: "./.data".into(),
        db: "sqlite".into(),
    };
    let mut pull_api = None;
    let mut observability = None;
    let mut default_timezone: Option<String> = None;
    let mut default_timeout: Option<String> = None;
    let mut default_retry = RetryConfig::default();
    let mut default_dead_letter = DeadLetterConfig::default();
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
                                d.args.iter().map(|a| a.value.as_str()).collect::<Vec<_>>().join(" "),
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
                                default_timezone =
                                    dir.args.first().map(|a| a.value.clone());
                            }
                            "timeout" => {
                                default_timeout = dir.args.first().map(|a| a.value.clone());
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
                    &default_timezone,
                    &default_timeout,
                    &default_retry,
                    &default_dead_letter,
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
                                        "level" => { if let Some(a) = dir.args.first() { log.level = a.value.clone(); } }
                                        "format" => { if let Some(a) = dir.args.first() { log.format = a.value.clone(); } }
                                        "output" => { if let Some(a) = dir.args.first() { log.output = a.value.clone(); } }
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
                                        "listen" => { if let Some(a) = dir.args.first() { metrics.listen = a.value.clone(); } }
                                        "path" => { if let Some(a) = dir.args.first() { metrics.path = a.value.clone(); } }
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
            _ => {}
        }
    }

    RuntimeConfig {
        server,
        pull_api,
        observability,
        jobs,
        calendars,
    }
}

fn compile_job(
    job: &JobBlock,
    default_tz: &Option<String>,
    default_timeout: &Option<String>,
    default_retry: &RetryConfig,
    default_dl: &DeadLetterConfig,
) -> JobConfig {
    let schedule = job
        .schedule
        .as_ref()
        .map(|s| CompiledSchedule::from_ast(&s.kind))
        .unwrap_or(CompiledSchedule::Disabled);

    let schedule_summary = schedule.summary();

    // Extract schedule options
    let mut timezone = default_tz.clone();
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
    let mut retry = default_retry.clone();
    let mut timeout = default_timeout.clone();
    let mut dead_letter = default_dl.clone();
    let mut metadata = HashMap::new();

    for dob in &job.directives {
        match dob {
            DirectiveOrBlock::Directive(d) => match d.key.value.as_str() {
                "description" => description = d.args.first().map(|a| a.value.clone()),
                "timeout" => timeout = d.args.first().map(|a| a.value.clone()),
                "window" => window = d.args.first().map(|a| a.value.clone()),
                _ => {}
            },
            DirectiveOrBlock::Block(block) => match block.name.value.as_str() {
                "runner" => runner = compile_runner_block(block),
                "retry" => retry = compile_retry_block(block),
                "dead_letter" => dead_letter = compile_dead_letter_block(block),
                "metadata" => {
                    for inner in &block.directives {
                        if let DirectiveOrBlock::Directive(d) = inner
                            && let Some(v) = d.args.first() {
                                metadata.insert(d.key.value.clone(), v.value.clone());
                            }
                    }
                }
                _ => {}
            },
            _ => {}
        }
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
            server { listen :8080; db sqlite }
            job etl:sync { every 15 minutes; timeout 10m }
        "#,
        )
        .unwrap();
        let cfg = compile(&ast);
        assert_eq!(cfg.server.listen, ":8080");
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
