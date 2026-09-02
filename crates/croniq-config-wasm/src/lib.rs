//! WebAssembly bindings for the Croniqfile DSL.
//!
//! Exposes a small surface for the in-browser schedule and calendar
//! generators (PR-B and PR-C):
//!
//! - `parse_schedule(dsl)` — validate a schedule line, return a typed AST
//! - `format_schedule(structured)` — emit the canonical schedule *line*
//! - `format_schedule_block(structured, key)` — emit a full, paste-ready
//!   `job <key> { … }` block
//! - `parse_calendar_rules(dsl)` — validate a calendar rules block
//! - `format_calendar_rules(structured)` — emit the canonical rule *lines*
//! - `format_calendar_block(structured, name)` — emit a full, paste-ready
//!   `calendar <name> { … }` block
//! - `next_fires(dsl, now_iso, count)` — preview upcoming firing times
//! - `evaluate_calendar_day(dsl, day_iso)` — is this day active?
//!
//! The `format_*` emitters build a loose, unambiguous fragment from the
//! form shape, parse it, and re-emit through `croniq_config::format` so
//! the output can never drift from the canonical `croniq fmt`.
//!
//! All inputs and outputs go through `serde-wasm-bindgen` so the
//! TypeScript caller sees plain objects, not opaque handles. Errors
//! become rejected promises (in the typed wrappers) or thrown JsValues
//! (in the raw bindings).

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use croniq_config::ast::{IntervalUnit, MonthOrdinal, ScheduleKind, ScheduleNode, Weekday};
use croniq_config::format::{format_calendar_rule_parts, format_schedule_line};
use croniq_config::parser::Parser;

/// Install a panic hook that forwards Rust panics to `console.error`.
/// Safe to call multiple times (the inner crate guards against that).
/// The TypeScript wrapper calls this once on first import.
#[wasm_bindgen(start)]
pub fn _start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

// ── Schedule shapes (serde-friendly mirror of croniq_config::ast::ScheduleKind) ──

/// Form-builder schedule shape. Mirrors `ScheduleKind` from the parser
/// AST but with simpler field types so the TypeScript caller can build
/// it from `<select>` values without knowing about `SourceSpan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum SchedulePayload {
    Interval {
        count: u32,
        unit: String,
    },
    Daily {
        hour: u8,
        minute: u8,
    },
    Weekdays {
        days: Vec<String>,
        hour: u8,
        minute: u8,
    },
    Monthly {
        ordinals: Vec<String>,
        hour: u8,
        minute: u8,
    },
    Once {
        at: String,
    },
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseScheduleResult {
    pub ok: bool,
    pub schedule: Option<SchedulePayload>,
    pub error: Option<String>,
}

/// Parse a single schedule line (e.g. `every 5 minutes`) and return the
/// structured form. Wraps the input in a placeholder job block so we can
/// reuse the existing recursive-descent parser.
#[wasm_bindgen(js_name = parseSchedule)]
pub fn parse_schedule(dsl: &str) -> Result<JsValue, JsValue> {
    let result = parse_schedule_inner(dsl);
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn parse_schedule_inner(dsl: &str) -> ParseScheduleResult {
    // Wrap in a synthetic job block — the parser only exposes
    // `parse_croniqfile`, not a standalone `parse_schedule_line`.
    let trimmed = dsl.trim();
    if trimmed.is_empty() {
        return ParseScheduleResult {
            ok: false,
            schedule: None,
            error: Some("schedule is empty".into()),
        };
    }
    // Schedule lines parse inline inside a job block — no nested
    // `schedule {}`. Synthetic key namespaces avoid clashes if anyone
    // reads the wrapped output for diagnostics.
    let wrapped = format!("job preview:line {{\n  {trimmed}\n}}\n");
    match Parser::parse(&wrapped) {
        Ok(ast) => {
            // Find the job's schedule node. The wrapping guarantees one
            // job and exactly one schedule, so unwrap is safe.
            let schedule = ast.items.iter().find_map(|i| {
                if let croniq_config::ast::Item::Job(j) = i {
                    j.schedule.clone()
                } else {
                    None
                }
            });
            match schedule {
                Some(node) => ParseScheduleResult {
                    ok: true,
                    schedule: Some(schedule_to_payload(&node)),
                    error: None,
                },
                None => ParseScheduleResult {
                    ok: false,
                    schedule: None,
                    error: Some("no schedule found".into()),
                },
            }
        }
        Err(e) => ParseScheduleResult {
            ok: false,
            schedule: None,
            error: Some(e.to_string()),
        },
    }
}

fn schedule_to_payload(node: &ScheduleNode) -> SchedulePayload {
    match &node.kind {
        ScheduleKind::Interval { count, unit } => SchedulePayload::Interval {
            count: *count,
            unit: match unit {
                IntervalUnit::Seconds => "seconds".into(),
                IntervalUnit::Minutes => "minutes".into(),
                IntervalUnit::Hours => "hours".into(),
            },
        },
        ScheduleKind::Daily { time } => SchedulePayload::Daily {
            hour: time.hour,
            minute: time.minute,
        },
        ScheduleKind::Weekdays { days, time } => SchedulePayload::Weekdays {
            days: days.iter().map(weekday_str).collect(),
            hour: time.hour,
            minute: time.minute,
        },
        ScheduleKind::Monthly { ordinals, time } => SchedulePayload::Monthly {
            ordinals: ordinals.iter().map(ordinal_str).collect(),
            hour: time.hour,
            minute: time.minute,
        },
        ScheduleKind::Once { at } => SchedulePayload::Once {
            at: at.value.clone(),
        },
        ScheduleKind::Disabled => SchedulePayload::Disabled,
    }
}

fn weekday_str(w: &Weekday) -> String {
    match w {
        Weekday::Monday => "monday",
        Weekday::Tuesday => "tuesday",
        Weekday::Wednesday => "wednesday",
        Weekday::Thursday => "thursday",
        Weekday::Friday => "friday",
        Weekday::Saturday => "saturday",
        Weekday::Sunday => "sunday",
    }
    .into()
}

fn ordinal_str(o: &MonthOrdinal) -> String {
    match o {
        MonthOrdinal::Day(n) => match n {
            1 => "1st".into(),
            2 => "2nd".into(),
            3 => "3rd".into(),
            21 => "21st".into(),
            22 => "22nd".into(),
            23 => "23rd".into(),
            31 => "31st".into(),
            n => format!("{n}th"),
        },
        MonthOrdinal::Last => "last".into(),
    }
}

// ── Format direction: structured → DSL string ──

/// Render a schedule payload as the canonical schedule *line*
/// (`every 5 minutes`, `once at 2026-…`, `disabled`, …).
///
/// Used for the live next-fires preview. For a paste-ready block use
/// [`format_schedule_block`].
#[wasm_bindgen(js_name = formatSchedule)]
pub fn format_schedule(value: JsValue) -> Result<String, JsValue> {
    let payload: SchedulePayload =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(format_schedule_inner(&payload))
}

/// Render a schedule payload as a full, copy-paste-ready job block:
/// `job <key> {\n  <schedule>\n}`. Rejects an invalid `key` (the parse
/// error is surfaced to the caller as a thrown JsValue).
#[wasm_bindgen(js_name = formatScheduleBlock)]
pub fn format_schedule_block(value: JsValue, key: &str) -> Result<String, JsValue> {
    let payload: SchedulePayload =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    format_schedule_block_inner(&payload, key).map_err(|e| JsValue::from_str(&e))
}

/// Build the loose (but always-valid-if-inputs-are) schedule line from
/// the form shape. This is deliberately *verbose* — canonicalisation
/// (weekday collapsing, unit spelling, quoting) is left to the shared
/// formatter so there's a single source of truth.
fn schedule_payload_to_loose_line(p: &SchedulePayload) -> String {
    match p {
        SchedulePayload::Interval { count, unit } => {
            let unit = match unit.as_str() {
                "seconds" | "minutes" | "hours" => unit.as_str(),
                _ => "minutes",
            };
            format!("every {count} {unit}")
        }
        SchedulePayload::Daily { hour, minute } => format!("every day at {hour:02}:{minute:02}"),
        SchedulePayload::Weekdays { days, hour, minute } => {
            // Space-joined day names — the parser accepts full and
            // 3-letter forms and does its own range collapsing.
            format!("every {} at {hour:02}:{minute:02}", days.join(" "))
        }
        SchedulePayload::Monthly {
            ordinals,
            hour,
            minute,
        } => format!(
            "every {} of month at {hour:02}:{minute:02}",
            ordinals.join(" ")
        ),
        // Unquoted — the canonical form (the old bridge force-quoted it).
        SchedulePayload::Once { at } => format!("once at {at}"),
        SchedulePayload::Disabled => "disabled".into(),
    }
}

/// Parse the loose line (wrapped in a synthetic job) and re-emit the
/// schedule via the canonical formatter. `None` if it doesn't parse.
fn canonical_schedule_line(loose: &str) -> Option<String> {
    let wrapped = format!("job preview:line {{\n  {loose}\n}}\n");
    let ast = Parser::parse(&wrapped).ok()?;
    let node = ast.items.iter().find_map(|i| match i {
        croniq_config::ast::Item::Job(j) => j.schedule.clone(),
        _ => None,
    })?;
    Some(format_schedule_line(&node.kind))
}

fn format_schedule_inner(p: &SchedulePayload) -> String {
    let loose = schedule_payload_to_loose_line(p);
    // Fall back to the loose form verbatim if it doesn't parse (e.g. an
    // empty weekday set) — this export must never throw, the UI relies
    // on it always returning a string.
    canonical_schedule_line(&loose).unwrap_or(loose)
}

fn format_schedule_block_inner(p: &SchedulePayload, key: &str) -> Result<String, String> {
    // A schedule-only block is a job block with no options.
    format_job_block_inner(p, key, &JobOptions::default())
}

// ── Job options (the job block beyond the schedule line) ───────────

/// Retry strategy from the form. `strategy` is `exponential` (uses
/// base/cap/jitter) or `fixed` (uses delay); `max_attempts` applies to
/// both. Empty/None fields are simply omitted from the emitted block.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RetryPayload {
    #[serde(default)]
    pub strategy: String,
    #[serde(default)]
    pub max_attempts: Option<u32>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub cap: Option<String>,
    #[serde(default)]
    pub jitter: Option<f64>,
    #[serde(default)]
    pub delay: Option<String>,
}

/// Dead-letter config from the form. All fields optional — omitted ones
/// are left out of the emitted block, so the compiler's inherited defaults
/// (or a `defaults {}` block) fill them in. `enabled: Some(false)` turns
/// dead-lettering off (issue #348).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeadLetterPayload {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub retention: Option<String>,
    #[serde(default)]
    pub operator_hint: Option<String>,
    #[serde(default)]
    pub replay_max_age: Option<String>,
}

/// Structured job-level options mirroring the form. Every field is
/// optional — a default `JobOptions` yields a schedule-only job block.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct JobOptions {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub retry: Option<RetryPayload>,
    #[serde(default)]
    pub dead_letter: Option<DeadLetterPayload>,
    #[serde(default)]
    pub runner_require: Vec<String>,
    #[serde(default)]
    pub runner_prefer: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// `"singleton"`, a positive integer string (→ `max_concurrent N`),
    /// or `None`/empty for the default (unbounded) concurrency.
    #[serde(default)]
    pub concurrency: Option<String>,
    /// Name of a `concurrency_group <name> { }` block this job draws on
    /// (issue #546) — a budget shared with the other jobs naming it, on top
    /// of any per-job [`Self::concurrency`]. The editor does not create the
    /// block; an undefined name is a validation error, not a silent no-op.
    #[serde(default)]
    pub concurrency_group: Option<String>,

    // ── Schedule-options block (Phase 2) ──
    // These attach *inside* the schedule line (`every … { … }`) and are
    // only valid on `every …` schedules — the parser rejects a block on
    // `once`/`disabled`, so they are dropped for those modes.
    /// Reference to a `calendar <name>` block defined elsewhere.
    #[serde(default)]
    pub schedule_calendar: Option<String>,
    /// IANA timezone the schedule is evaluated in.
    #[serde(default)]
    pub schedule_timezone: Option<String>,
    /// RFC3339 lower bound — the schedule doesn't fire before this.
    #[serde(default)]
    pub not_before: Option<String>,
    /// RFC3339 upper bound — the schedule doesn't fire after this.
    #[serde(default)]
    pub not_after: Option<String>,

    // ── Job-level scheduling / execution directives (Phase 2) ──
    /// Time-of-day window `HH:MM..HH:MM` (a job-level directive, *not* a
    /// schedule option — see the grammar).
    #[serde(default)]
    pub window: Option<String>,
    /// `queued` (default) or `ephemeral`. Emitted as the `execution_mode`
    /// directive rather than the schedule prefix, because the canonical
    /// formatter drops the prefix (so the prefix wouldn't round-trip).
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// `all` | `latest` | `none`.
    #[serde(default)]
    pub catch_up: Option<String>,
    /// Duration or `none`.
    #[serde(default)]
    pub queue_ttl: Option<String>,
    #[serde(default)]
    pub max_queue_depth: Option<u32>,

    /// Runner execution payload — `runner shell { … }` / `runner exec
    /// { … }`. Independent of the `runner_require`/`runner_prefer`
    /// placement block (a job may carry both). (Phase 3c)
    #[serde(default)]
    pub runner_exec: Option<RunnerExecPayload>,
}

/// A single `KEY value` environment entry inside a runner exec block.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EnvPair {
    pub key: String,
    #[serde(default)]
    pub value: String,
}

/// The runner execution command. `mode` is `shell` (a single `command`
/// string) or `exec` (an `args` argv list); both may set `workdir`,
/// `user`, and `env` entries.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RunnerExecPayload {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub env: Vec<EnvPair>,
}

/// Render a schedule payload plus job-level options as a full,
/// copy-paste-ready `job <key> { … }` block. Options may be `null`/
/// `undefined` (→ schedule-only block). Rejects invalid input (e.g. a
/// malformed key or duration) with the parser error as a thrown value.
#[wasm_bindgen(js_name = formatJobBlock)]
pub fn format_job_block(value: JsValue, key: &str, options: JsValue) -> Result<String, JsValue> {
    let payload: SchedulePayload =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let opts: JobOptions = if options.is_undefined() || options.is_null() {
        JobOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(|e| JsValue::from_str(&e.to_string()))?
    };
    format_job_block_inner(&payload, key, &opts).map_err(|e| JsValue::from_str(&e))
}

fn format_job_block_inner(
    p: &SchedulePayload,
    key: &str,
    o: &JobOptions,
) -> Result<String, String> {
    let key = key.trim();
    let key = if key.is_empty() {
        "namespace:name"
    } else {
        key
    };

    let mut lines: Vec<String> = Vec::new();
    // `description` floats to the top in the canonical formatter anyway;
    // emit order here is not significant.
    if let Some(d) = opt_str(&o.description) {
        lines.push(format!("  description \"{}\"", escape_dquote(d)));
    }
    lines.push(format!("  {}", schedule_line_with_options(p, o)));
    if let Some(t) = opt_str(&o.timeout) {
        lines.push(format!("  timeout {t}"));
    }
    // Job-level scheduling / execution directives.
    if let Some(w) = opt_str(&o.window) {
        // `HH:MM..HH:MM` — a bare range token; never quote it.
        lines.push(format!("  window {w}"));
    }
    if let Some(m) = opt_str(&o.execution_mode) {
        lines.push(format!("  execution_mode {m}"));
    }
    if let Some(c) = opt_str(&o.catch_up) {
        lines.push(format!("  catch_up {c}"));
    }
    if let Some(q) = opt_str(&o.queue_ttl) {
        lines.push(format!("  queue_ttl {q}"));
    }
    if let Some(d) = o.max_queue_depth {
        lines.push(format!("  max_queue_depth {d}"));
    }
    if let Some(line) = retry_loose_line(o.retry.as_ref()) {
        lines.push(format!("  {line}"));
    }
    if let Some(line) = dead_letter_loose_line(o.dead_letter.as_ref()) {
        lines.push(format!("  {line}"));
    }
    if let Some(line) = runner_loose_line(&o.runner_require, &o.runner_prefer) {
        lines.push(format!("  {line}"));
    }
    if let Some(line) = runner_exec_loose_line(o.runner_exec.as_ref()) {
        lines.push(format!("  {line}"));
    }
    let tags: Vec<String> = o
        .tags
        .iter()
        .filter(|t| !t.trim().is_empty())
        .map(|t| quote_if_needed(t.trim()))
        .collect();
    if !tags.is_empty() {
        lines.push(format!("  tags {}", tags.join(" ")));
    }
    if let Some(c) = opt_str(&o.concurrency) {
        if c == "singleton" {
            lines.push("  singleton".into());
        } else {
            lines.push(format!("  max_concurrent {c}"));
        }
    }
    if let Some(g) = opt_str(&o.concurrency_group) {
        lines.push(format!("  concurrency_group {}", quote_if_needed(g)));
    }

    let src = format!("job {key} {{\n{}\n}}\n", lines.join("\n"));
    let ast = Parser::parse(&src).map_err(|e| e.to_string())?;
    Ok(croniq_config::format::format(&ast))
}

/// Build the schedule line, appending an `{ … }` schedule-options block
/// when any option is set. The block is only valid on `every …`
/// schedules — the parser rejects it on `once`/`disabled`, so options
/// are silently dropped there (the UI hides them for those modes).
fn schedule_line_with_options(p: &SchedulePayload, o: &JobOptions) -> String {
    let base = schedule_payload_to_loose_line(p);
    let supports_block = matches!(
        p,
        SchedulePayload::Interval { .. }
            | SchedulePayload::Daily { .. }
            | SchedulePayload::Weekdays { .. }
            | SchedulePayload::Monthly { .. }
    );
    if !supports_block {
        return base;
    }
    let mut inner: Vec<String> = Vec::new();
    if let Some(c) = opt_str(&o.schedule_calendar) {
        inner.push(format!("calendar {}", quote_if_needed(c)));
    }
    if let Some(t) = opt_str(&o.schedule_timezone) {
        inner.push(format!("timezone {}", quote_if_needed(t)));
    }
    if let Some(nb) = opt_str(&o.not_before) {
        inner.push(format!("not_before {nb}"));
    }
    if let Some(na) = opt_str(&o.not_after) {
        inner.push(format!("not_after {na}"));
    }
    if inner.is_empty() {
        base
    } else {
        format!("{base} {{ {} }}", inner.join("; "))
    }
}

/// Trim + treat empty as absent.
fn opt_str(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn escape_dquote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Emit a token bare when the lexer accepts it as an ident, else quote
/// it. Bare-ident chars per the lexer: alphanumerics + `:/*?-._+@`.
/// Tags like `env=prod` contain `=` (not an ident char) so they quote.
///
/// A `{…}` placeholder (`{env.X}`, `{vars.Y}`, …) is left **bare** — the
/// lexer only recognises placeholders unquoted; quoting one would turn
/// it into a literal string with braces.
fn quote_if_needed(s: &str) -> String {
    if is_placeholder(s) {
        return s.to_string();
    }
    let bare_ok = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || ":/*?-._+@".contains(c));
    if bare_ok {
        s.to_string()
    } else {
        format!("\"{}\"", escape_dquote(s))
    }
}

/// Whether `s` is a single `{…}` placeholder token.
fn is_placeholder(s: &str) -> bool {
    s.len() >= 2
        && s.starts_with('{')
        && s.ends_with('}')
        && !s[1..s.len() - 1].contains(['{', '}'])
}

/// `retry <strategy> { max_attempts N; base …; cap …; jitter … }` or
/// `… { max_attempts N; delay … }` for the fixed strategy. `None` if
/// the retry payload has no usable fields.
fn retry_loose_line(r: Option<&RetryPayload>) -> Option<String> {
    let r = r?;
    let strat = {
        let s = r.strategy.trim();
        if s.is_empty() { "exponential" } else { s }
    };
    let mut inner: Vec<String> = Vec::new();
    if let Some(n) = r.max_attempts {
        inner.push(format!("max_attempts {n}"));
    }
    if strat == "fixed" {
        if let Some(d) = opt_str(&r.delay) {
            inner.push(format!("delay {d}"));
        }
    } else {
        if let Some(b) = opt_str(&r.base) {
            inner.push(format!("base {b}"));
        }
        if let Some(c) = opt_str(&r.cap) {
            inner.push(format!("cap {c}"));
        }
        if let Some(j) = r.jitter {
            inner.push(format!("jitter {j}"));
        }
    }
    if inner.is_empty() {
        return None;
    }
    Some(format!("retry {strat} {{ {} }}", inner.join("; ")))
}

/// `dead_letter { enabled …; retention …; operator_hint "…" }` — only the
/// fields the form set, `None` if none. `enabled` emits the bare
/// `true`/`false` the compiler's `parse_bool` accepts; `operator_hint` is
/// quoted (free text). Fields left out inherit the compiler's defaults /
/// a `defaults {}` block via the field-merge in compile.rs (issue #348).
fn dead_letter_loose_line(dl: Option<&DeadLetterPayload>) -> Option<String> {
    let dl = dl?;
    let mut inner: Vec<String> = Vec::new();
    if let Some(enabled) = dl.enabled {
        inner.push(format!("enabled {enabled}"));
    }
    if let Some(r) = opt_str(&dl.retention) {
        inner.push(format!("retention {r}"));
    }
    if let Some(h) = opt_str(&dl.operator_hint) {
        inner.push(format!("operator_hint \"{}\"", escape_dquote(h)));
    }
    if let Some(a) = opt_str(&dl.replay_max_age) {
        inner.push(format!("replay_max_age {a}"));
    }
    if inner.is_empty() {
        return None;
    }
    Some(format!("dead_letter {{ {} }}", inner.join("; ")))
}

/// `runner { require …; prefer … }` from the capability lists, or
/// `None` if both are empty.
fn runner_loose_line(require: &[String], prefer: &[String]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for r in require.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        parts.push(format!("require {r}"));
    }
    for p in prefer.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        parts.push(format!("prefer {p}"));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("runner {{ {} }}", parts.join("; ")))
}

/// `runner shell { command "…"; workdir …; user …; env { K V } }` or
/// `runner exec { args …; … }`. `None` if there's no command/args to run.
fn runner_exec_loose_line(r: Option<&RunnerExecPayload>) -> Option<String> {
    let r = r?;
    let mode = if r.mode.trim() == "exec" {
        "exec"
    } else {
        "shell"
    };
    let mut inner: Vec<String> = Vec::new();
    if mode == "shell" {
        // `command` is required for a shell runner — without it there's
        // nothing to emit.
        let cmd = opt_str(&r.command)?;
        inner.push(format!("command \"{}\"", escape_dquote(cmd)));
    } else {
        let args: Vec<String> = r
            .args
            .iter()
            .map(|a| a.trim())
            .filter(|a| !a.is_empty())
            .map(quote_if_needed)
            .collect();
        if args.is_empty() {
            return None;
        }
        inner.push(format!("args {}", args.join(" ")));
    }
    if let Some(w) = opt_str(&r.workdir) {
        inner.push(format!("workdir {}", quote_if_needed(w)));
    }
    if let Some(u) = opt_str(&r.user) {
        inner.push(format!("user {}", quote_if_needed(u)));
    }
    let env_pairs: Vec<String> = r
        .env
        .iter()
        .filter_map(|p| {
            let k = p.key.trim();
            if k.is_empty() {
                return None;
            }
            Some(format!("{k} {}", quote_if_needed(p.value.trim())))
        })
        .collect();
    if !env_pairs.is_empty() {
        inner.push(format!("env {{ {} }}", env_pairs.join("; ")));
    }
    Some(format!("runner {mode} {{ {} }}", inner.join("; ")))
}

// ── next_fires preview ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NextFiresResult {
    pub ok: bool,
    pub fires: Vec<String>,
    pub error: Option<String>,
}

/// Compute the next `count` firing times for a schedule line.
/// `now_iso` must be RFC 3339 / ISO 8601 with timezone (e.g.
/// `2026-04-25T18:00:00Z`). Returns ISO strings in UTC.
#[wasm_bindgen(js_name = nextFires)]
pub fn next_fires(dsl: &str, now_iso: &str, count: u32) -> Result<JsValue, JsValue> {
    let result = next_fires_inner(dsl, now_iso, count);
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn next_fires_inner(dsl: &str, now_iso: &str, count: u32) -> NextFiresResult {
    let parsed = parse_schedule_inner(dsl);
    if !parsed.ok {
        return NextFiresResult {
            ok: false,
            fires: vec![],
            error: parsed.error,
        };
    }
    // Re-parse via the wrapped DSL to get a real ScheduleNode for the
    // scheduler runtime to evaluate. This duplicates a tiny bit of work
    // but keeps the FFI surface clean — `SchedulePayload` is the public
    // shape; `ScheduleNode` is internal.
    let trimmed = dsl.trim();
    // Schedule lines parse inline inside a job block — no nested
    // `schedule {}`. Synthetic key namespaces avoid clashes if anyone
    // reads the wrapped output for diagnostics.
    let wrapped = format!("job preview:line {{\n  {trimmed}\n}}\n");
    let ast = match Parser::parse(&wrapped) {
        Ok(a) => a,
        Err(e) => {
            return NextFiresResult {
                ok: false,
                fires: vec![],
                error: Some(e.to_string()),
            };
        }
    };
    let node = ast
        .items
        .iter()
        .find_map(|i| match i {
            croniq_config::ast::Item::Job(j) => j.schedule.clone(),
            _ => None,
        })
        .expect("preview wrapping always emits one job with one schedule");

    let now = match chrono::DateTime::parse_from_rfc3339(now_iso) {
        Ok(t) => t.with_timezone(&chrono::Utc),
        Err(e) => {
            return NextFiresResult {
                ok: false,
                fires: vec![],
                error: Some(format!("invalid now_iso: {e}")),
            };
        }
    };

    let mut fires = Vec::with_capacity(count as usize);
    let mut cursor = now;
    for _ in 0..count {
        match next_fire_utc(&node.kind, cursor) {
            Some(next) => {
                fires.push(next.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
                // Use the just-emitted fire as the new cursor — for
                // interval schedules `next_fire` returns `cursor + N`,
                // so passing `next` gives `next + N` next iteration.
                // For daily/weekday/monthly the strict-greater check
                // inside `next_fire_utc` handles the increment.
                cursor = next;
            }
            None => break,
        }
    }
    NextFiresResult {
        ok: true,
        fires,
        error: None,
    }
}

/// UTC-only next-fire computation. We deliberately don't depend on
/// `croniq-scheduler` (which pulls `chrono-tz` and ~150 KB of timezone
/// data) — for the in-browser preview UTC is acceptable and the UI
/// converts to local time client-side. Timezone-aware preview can come
/// as a follow-up if there's demand.
fn next_fire_utc(
    kind: &ScheduleKind,
    after: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
    match kind {
        ScheduleKind::Interval { count, unit } => {
            let secs = match unit {
                IntervalUnit::Seconds => *count as i64,
                IntervalUnit::Minutes => *count as i64 * 60,
                IntervalUnit::Hours => *count as i64 * 3600,
            };
            if secs <= 0 {
                return None;
            }
            Some(after + Duration::seconds(secs))
        }
        ScheduleKind::Daily { time } => {
            let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)?;
            let today = after.date_naive();
            // Try today first; if that's already past, roll to tomorrow.
            let candidate_today = chrono::Utc.from_utc_datetime(&today.and_time(t));
            if candidate_today > after {
                return Some(candidate_today);
            }
            let tomorrow = today + Duration::days(1);
            Some(chrono::Utc.from_utc_datetime(&tomorrow.and_time(t)))
        }
        ScheduleKind::Weekdays { days, time } => {
            let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)?;
            let target_set: Vec<chrono::Weekday> = days.iter().map(weekday_to_chrono).collect();
            // Walk forward day-by-day for at most 7 days — guaranteed
            // to find a match if any weekday is in the set.
            for offset in 0..=7 {
                let day = after.date_naive() + Duration::days(offset);
                if !target_set.contains(&day.weekday()) {
                    continue;
                }
                let candidate = chrono::Utc.from_utc_datetime(&day.and_time(t));
                if candidate > after {
                    return Some(candidate);
                }
            }
            None
        }
        ScheduleKind::Monthly { ordinals, time } => {
            let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)?;
            // Try this month, next month, month-after — three is enough to
            // satisfy any well-formed monthly schedule.
            let mut year = after.year();
            let mut month = after.month();
            for _ in 0..3 {
                for ord in ordinals {
                    let day = match ord {
                        MonthOrdinal::Day(n) => *n as u32,
                        MonthOrdinal::Last => last_day_of_month(year, month),
                    };
                    if let Some(d) = NaiveDate::from_ymd_opt(year, month, day) {
                        let candidate = chrono::Utc.from_utc_datetime(&d.and_time(t));
                        if candidate > after {
                            return Some(candidate);
                        }
                    }
                }
                // Roll to next month.
                if month == 12 {
                    month = 1;
                    year += 1;
                } else {
                    month += 1;
                }
            }
            None
        }
        ScheduleKind::Once { at } => {
            // The AST stores the once-time as a string; parse it as
            // RFC3339 and skip if we're already past.
            let parsed = chrono::DateTime::parse_from_rfc3339(&at.value).ok()?;
            let utc = parsed.with_timezone(&chrono::Utc);
            if utc > after { Some(utc) } else { None }
        }
        ScheduleKind::Disabled => None,
    }
}

fn weekday_to_chrono(w: &Weekday) -> chrono::Weekday {
    match w {
        Weekday::Monday => chrono::Weekday::Mon,
        Weekday::Tuesday => chrono::Weekday::Tue,
        Weekday::Wednesday => chrono::Weekday::Wed,
        Weekday::Thursday => chrono::Weekday::Thu,
        Weekday::Friday => chrono::Weekday::Fri,
        Weekday::Saturday => chrono::Weekday::Sat,
        Weekday::Sunday => chrono::Weekday::Sun,
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    use chrono::{Datelike, NaiveDate};
    // Step into the next month then back one day.
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next = NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap();
    (first_of_next - chrono::Duration::days(1)).day()
}

// ── Calendar rules: parse + format ─────────────────────────────────

/// A single calendar rule, mirroring the parser's loose shape: the
/// rule_type is a free string (`"weekly"`, `"window"`, `"annual"`,
/// `"monthly"`, `"timezone"`) and `args` holds the rest of the line
/// verbatim (each token is a `StringValue`). The scheduler validates
/// the combination at compile-time; we surface it raw so the form
/// builder can extend without recompiling the wasm bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarRulePayload {
    pub action: String, // "include" | "exclude"
    pub rule_type: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseCalendarResult {
    pub ok: bool,
    pub rules: Vec<CalendarRulePayload>,
    pub diagnostics: Vec<String>,
}

/// Parse a multi-line calendar rule block. Wraps in a calendar block
/// to reuse the existing parser.
#[wasm_bindgen(js_name = parseCalendarRules)]
pub fn parse_calendar_rules(dsl: &str) -> Result<JsValue, JsValue> {
    let result = parse_calendar_rules_inner(dsl);
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn parse_calendar_rules_inner(dsl: &str) -> ParseCalendarResult {
    let trimmed = dsl.trim();
    if trimmed.is_empty() {
        return ParseCalendarResult {
            ok: true,
            rules: vec![],
            diagnostics: vec![],
        };
    }
    let wrapped = format!("calendar \"preview\" {{\n{trimmed}\n}}\n");
    match Parser::parse(&wrapped) {
        Ok(ast) => {
            let cal = ast.items.iter().find_map(|i| match i {
                croniq_config::ast::Item::Calendar(c) => Some(c),
                _ => None,
            });
            let rules = cal
                .map(|c| c.rules.iter().map(rule_to_payload).collect())
                .unwrap_or_default();
            ParseCalendarResult {
                ok: true,
                rules,
                diagnostics: vec![],
            }
        }
        Err(e) => ParseCalendarResult {
            ok: false,
            rules: vec![],
            diagnostics: vec![e.to_string()],
        },
    }
}

fn rule_to_payload(r: &croniq_config::ast::CalendarRule) -> CalendarRulePayload {
    let action = match r.kind {
        croniq_config::ast::CalendarRuleKind::Include => "include",
        croniq_config::ast::CalendarRuleKind::Exclude => "exclude",
    }
    .into();
    CalendarRulePayload {
        action,
        rule_type: r.rule_type.value.clone(),
        args: r.args.iter().map(|a| a.value.clone()).collect(),
    }
}

/// Format a list of structured rules as the canonical rule *lines*
/// (no wrapping `calendar { … }`). For a paste-ready block use
/// [`format_calendar_block`].
#[wasm_bindgen(js_name = formatCalendarRules)]
pub fn format_calendar_rules(value: JsValue) -> Result<String, JsValue> {
    let rules: Vec<CalendarRulePayload> =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(rules.iter().map(format_rule).collect::<Vec<_>>().join("\n"))
}

/// Format a list of structured rules as a full, copy-paste-ready
/// calendar block: `calendar <name> {\n  <rules>\n}`. A parse failure
/// (e.g. a malformed rule) is surfaced as a thrown JsValue so the UI
/// can show the diagnostic.
#[wasm_bindgen(js_name = formatCalendarBlock)]
pub fn format_calendar_block(value: JsValue, name: &str) -> Result<String, JsValue> {
    let rules: Vec<CalendarRulePayload> =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    format_calendar_block_inner(&rules, name).map_err(|e| JsValue::from_str(&e))
}

fn format_rule(r: &CalendarRulePayload) -> String {
    format_calendar_rule_parts(&r.action, &r.rule_type, &r.args)
}

fn format_calendar_block_inner(
    rules: &[CalendarRulePayload],
    name: &str,
) -> Result<String, String> {
    let name = name.trim();
    let name = if name.is_empty() {
        "calendar-name"
    } else {
        name
    };
    let body: String = rules
        .iter()
        .map(|r| format!("  {}", format_rule(r)))
        .collect::<Vec<_>>()
        .join("\n");
    let src = format!("calendar {name} {{\n{body}\n}}\n");
    let ast = Parser::parse(&src).map_err(|e| e.to_string())?;
    Ok(croniq_config::format::format(&ast))
}

// ── Top-level config blocks (Phase 3a) ─────────────────────────────

/// One directive inside a top-level block. Either a leaf `key arg arg …`
/// or, when `children` is non-empty, a nested sub-block
/// `key [qualifier] { …children… }` (e.g. `retry exponential { … }`,
/// `log { … }`). Args/qualifiers are quoted only when the lexer wouldn't
/// accept them bare. A leaf with no args and no children is skipped
/// (blank field).
#[derive(Debug, Clone, Deserialize)]
pub struct DirectivePayload {
    pub key: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub qualifier: Option<String>,
    /// Force-quote the qualifier (used for `alerts` channel/rule names,
    /// which are string-qualified by convention). Bare qualifiers like a
    /// `retry exponential` strategy leave this false.
    #[serde(default)]
    pub quote_qualifier: bool,
    #[serde(default)]
    pub children: Vec<DirectivePayload>,
}

/// Top-level blocks this emitter supports. `server`/`smtp`/… are flat
/// directive lists; `observability`/`defaults`/`alerts` carry nested
/// sub-blocks (handled generically via `DirectivePayload::children`,
/// with `alerts` channel/rule names force-quoted via `quote_qualifier`).
const TOP_LEVEL_BLOCKS: &[&str] = &[
    "server",
    "pull_api",
    "mcp",
    "policy",
    "smtp",
    "vars",
    "oidc",
    "observability",
    "defaults",
    "alerts",
];

/// Render a top-level block (`server { … }`, `observability { … }`, …)
/// from a directive tree. Empty directives/args/sub-blocks are skipped;
/// unknown block names and parse failures are surfaced as thrown values.
#[wasm_bindgen(js_name = formatTopLevelBlock)]
pub fn format_top_level_block(name: &str, directives: JsValue) -> Result<String, JsValue> {
    let dirs: Vec<DirectivePayload> = serde_wasm_bindgen::from_value(directives)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    format_top_level_block_inner(name, &dirs).map_err(|e| JsValue::from_str(&e))
}

/// Render one directive (leaf or nested sub-block) at the given indent.
/// Returns `None` when the directive is effectively empty (blank field /
/// empty sub-block), so it is omitted from the output.
fn emit_directive(d: &DirectivePayload, indent: usize) -> Option<String> {
    let key = d.key.trim();
    if key.is_empty() {
        return None;
    }
    let pad = "  ".repeat(indent);

    let child_lines: Vec<String> = d
        .children
        .iter()
        .filter_map(|c| emit_directive(c, indent + 1))
        .collect();
    if !child_lines.is_empty() {
        let mut s = format!("{pad}{key}");
        if let Some(q) = d
            .qualifier
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
        {
            s.push(' ');
            if d.quote_qualifier {
                s.push_str(&format!("\"{}\"", escape_dquote(q)));
            } else {
                s.push_str(&quote_if_needed(q));
            }
        }
        s.push_str(" {\n");
        s.push_str(&child_lines.join("\n"));
        s.push('\n');
        s.push_str(&pad);
        s.push('}');
        return Some(s);
    }

    // Leaf: `key arg…`. A key with no value means the field was left
    // blank — skip it (none of the supported directives are valueless).
    let args: Vec<String> = d
        .args
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .map(quote_if_needed)
        .collect();
    if args.is_empty() {
        return None;
    }
    Some(format!("{pad}{key} {}", args.join(" ")))
}

fn format_top_level_block_inner(name: &str, dirs: &[DirectivePayload]) -> Result<String, String> {
    let name = name.trim();
    if !TOP_LEVEL_BLOCKS.contains(&name) {
        return Err(format!("unsupported top-level block: '{name}'"));
    }
    let lines: Vec<String> = dirs.iter().filter_map(|d| emit_directive(d, 1)).collect();
    if lines.is_empty() {
        return Err("no settings — fill at least one field".into());
    }
    let src = format!("{name} {{\n{}\n}}\n", lines.join("\n"));
    let ast = Parser::parse(&src).map_err(|e| e.to_string())?;
    Ok(croniq_config::format::format(&ast))
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval() {
        let r = parse_schedule_inner("every 5 minutes");
        assert!(r.ok, "{:?}", r.error);
        match r.schedule.unwrap() {
            SchedulePayload::Interval { count, unit } => {
                assert_eq!(count, 5);
                assert_eq!(unit, "minutes");
            }
            other => panic!("expected interval, got {other:?}"),
        }
    }

    #[test]
    fn parse_daily() {
        let r = parse_schedule_inner("every day at 09:30");
        assert!(r.ok, "{:?}", r.error);
        match r.schedule.unwrap() {
            SchedulePayload::Daily { hour, minute } => {
                assert_eq!(hour, 9);
                assert_eq!(minute, 30);
            }
            other => panic!("expected daily, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_interval() {
        let p = SchedulePayload::Interval {
            count: 15,
            unit: "minutes".into(),
        };
        assert_eq!(format_schedule_inner(&p), "every 15 minutes");
    }

    #[test]
    fn round_trip_weekday_collapse() {
        let p = SchedulePayload::Weekdays {
            days: vec![
                "monday".into(),
                "tuesday".into(),
                "wednesday".into(),
                "thursday".into(),
                "friday".into(),
            ],
            hour: 9,
            minute: 0,
        };
        assert_eq!(format_schedule_inner(&p), "every weekday at 09:00");
    }

    #[test]
    fn parse_garbage_returns_error() {
        let r = parse_schedule_inner("not a schedule");
        assert!(!r.ok);
        assert!(r.error.is_some());
    }

    #[test]
    fn parse_calendar_rules_empty_ok() {
        let r = parse_calendar_rules_inner("");
        assert!(r.ok);
        assert!(r.rules.is_empty());
    }

    #[test]
    fn next_fires_interval_steps_correctly() {
        let r = next_fires_inner("every 5 minutes", "2026-04-25T18:00:00Z", 3);
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(
            r.fires,
            vec![
                "2026-04-25T18:05:00Z".to_string(),
                "2026-04-25T18:10:00Z".to_string(),
                "2026-04-25T18:15:00Z".to_string(),
            ]
        );
    }

    #[test]
    fn next_fires_daily_rolls_to_tomorrow_when_past() {
        let r = next_fires_inner("every day at 09:00", "2026-04-25T18:00:00Z", 2);
        assert!(r.ok);
        // 09:00 today is past 18:00, so first fire is tomorrow.
        assert_eq!(
            r.fires,
            vec![
                "2026-04-26T09:00:00Z".to_string(),
                "2026-04-27T09:00:00Z".to_string(),
            ]
        );
    }

    #[test]
    fn format_weekday_collapses_three_to_range() {
        let p = SchedulePayload::Weekdays {
            days: vec!["monday".into(), "tuesday".into(), "wednesday".into()],
            hour: 9,
            minute: 0,
        };
        assert_eq!(format_schedule_inner(&p), "every Mon..Wed at 09:00");
    }

    #[test]
    fn format_calendar_weekly_uses_weekday_alias_via_helper() {
        let rule = CalendarRulePayload {
            action: "include".into(),
            rule_type: "weekly".into(),
            args: vec![
                "Mon".into(),
                "Tue".into(),
                "Wed".into(),
                "Thu".into(),
                "Fri".into(),
            ],
        };
        assert_eq!(format_rule(&rule), "include weekly weekday");
    }

    #[test]
    fn next_fires_weekdays_picks_next_match() {
        // 2026-04-25 is a Saturday. Asking for Mon/Wed/Fri should land
        // on Monday 2026-04-27.
        let r = next_fires_inner(
            "every monday wednesday friday at 09:00",
            "2026-04-25T18:00:00Z",
            1,
        );
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.fires, vec!["2026-04-27T09:00:00Z".to_string()]);
    }

    #[test]
    fn schedule_once_line_is_unquoted() {
        // Bug #1: the bridge used to force-quote the datetime.
        let p = SchedulePayload::Once {
            at: "2026-12-31T23:00:00Z".into(),
        };
        assert_eq!(format_schedule_inner(&p), "once at 2026-12-31T23:00:00Z");
    }

    #[test]
    fn schedule_interval_singular_count_emits_singular() {
        // Issue #336: a count of 1 uses the grammatical singular
        // (`every 1 minute`), matching `croniq fmt`. The unit is pluralised
        // only for counts other than 1. Both forms parse, so this round-trips.
        let p = SchedulePayload::Interval {
            count: 1,
            unit: "minutes".into(),
        };
        assert_eq!(format_schedule_inner(&p), "every 1 minute");
    }

    #[test]
    fn schedule_block_wraps_in_job() {
        let p = SchedulePayload::Interval {
            count: 5,
            unit: "minutes".into(),
        };
        let out = format_schedule_block_inner(&p, "reports:daily").unwrap();
        assert_eq!(out, "job reports:daily {\n  every 5 minutes\n}\n");
        // And it must be re-parseable at top level.
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn schedule_block_once_unquoted_and_valid() {
        let p = SchedulePayload::Once {
            at: "2026-12-31T23:00:00Z".into(),
        };
        let out = format_schedule_block_inner(&p, "migration:v2").unwrap();
        assert!(out.contains("once at 2026-12-31T23:00:00Z"), "{out}");
        assert!(!out.contains('"'), "once should be unquoted: {out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn schedule_block_rejects_invalid_key() {
        let p = SchedulePayload::Interval {
            count: 5,
            unit: "minutes".into(),
        };
        // Missing namespace separator — the parser rejects it.
        assert!(format_schedule_block_inner(&p, "not-a-valid-key").is_err());
    }

    #[test]
    fn calendar_rule_timezone_is_bare_directive() {
        // Bug #2: timezone was emitted as `include timezone …`.
        let r = CalendarRulePayload {
            action: "include".into(),
            rule_type: "timezone".into(),
            args: vec!["Europe/Vienna".into()],
        };
        assert_eq!(format_rule(&r), "timezone \"Europe/Vienna\"");
        assert!(!format_rule(&r).contains("include"));
    }

    #[test]
    fn calendar_block_wraps_and_canonicalises() {
        let rules = vec![
            CalendarRulePayload {
                action: "include".into(),
                rule_type: "timezone".into(),
                args: vec!["Europe/Vienna".into()],
            },
            CalendarRulePayload {
                action: "include".into(),
                rule_type: "weekly".into(),
                args: vec![
                    "Mon".into(),
                    "Tue".into(),
                    "Wed".into(),
                    "Thu".into(),
                    "Fri".into(),
                ],
            },
            CalendarRulePayload {
                action: "exclude".into(),
                rule_type: "annual".into(),
                args: vec!["12-25".into()],
            },
        ];
        let out = format_calendar_block_inner(&rules, "business-days").unwrap();
        assert_eq!(
            out,
            "calendar business-days {\n  timezone \"Europe/Vienna\"\n  include weekly weekday\n  exclude annual 12-25\n}\n"
        );
        Parser::parse(&out).unwrap();
    }

    // ── Job options (Phase 1) ──────────────────────────────────────

    fn interval5() -> SchedulePayload {
        SchedulePayload::Interval {
            count: 5,
            unit: "minutes".into(),
        }
    }

    #[test]
    fn job_block_empty_options_equals_schedule_only() {
        let out =
            format_job_block_inner(&interval5(), "reports:daily", &JobOptions::default()).unwrap();
        assert_eq!(out, "job reports:daily {\n  every 5 minutes\n}\n");
    }

    #[test]
    fn job_block_description_floats_to_top_and_quotes() {
        let o = JobOptions {
            description: Some("Nightly ETL".into()),
            timeout: Some("15m".into()),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "etl:sync", &o).unwrap();
        assert_eq!(
            out,
            "job etl:sync {\n  description \"Nightly ETL\"\n\n  every 5 minutes\n  timeout 15m\n}\n"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_retry_exponential_inline() {
        let o = JobOptions {
            retry: Some(RetryPayload {
                strategy: "exponential".into(),
                max_attempts: Some(5),
                base: Some("5s".into()),
                cap: Some("2m".into()),
                jitter: Some(0.3),
                delay: None,
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "ops:check", &o).unwrap();
        assert!(
            out.contains("retry exponential { max_attempts 5; base 5s; cap 2m; jitter 0.3 }"),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_retry_fixed_uses_delay_not_base() {
        let o = JobOptions {
            retry: Some(RetryPayload {
                strategy: "fixed".into(),
                max_attempts: Some(2),
                delay: Some("10s".into()),
                base: Some("SHOULD_BE_IGNORED".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "ops:check", &o).unwrap();
        assert!(
            out.contains("retry fixed { max_attempts 2; delay 10s }"),
            "{out}"
        );
        assert!(!out.contains("SHOULD_BE_IGNORED"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_runner_require_prefer() {
        let o = JobOptions {
            runner_require: vec!["health-check".into(), "eu-west".into()],
            runner_prefer: vec!["gpu".into()],
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "ops:check", &o).unwrap();
        assert!(
            out.contains("runner { require health-check; require eu-west; prefer gpu }"),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_tags_are_quoted_because_of_equals() {
        let o = JobOptions {
            tags: vec!["env=prod".into(), "team=billing".into()],
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "billing:run", &o).unwrap();
        assert!(out.contains("tags \"env=prod\" \"team=billing\""), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_singleton_and_max_concurrent() {
        let o1 = JobOptions {
            concurrency: Some("singleton".into()),
            ..Default::default()
        };
        let out1 = format_job_block_inner(&interval5(), "a:b", &o1).unwrap();
        assert!(out1.contains("\n  singleton\n"), "{out1}");
        Parser::parse(&out1).unwrap();

        let o2 = JobOptions {
            concurrency: Some("3".into()),
            ..Default::default()
        };
        let out2 = format_job_block_inner(&interval5(), "a:b", &o2).unwrap();
        assert!(out2.contains("max_concurrent 3"), "{out2}");
        Parser::parse(&out2).unwrap();
    }

    #[test]
    fn job_block_dead_letter_enabled_false() {
        let o = JobOptions {
            dead_letter: Some(DeadLetterPayload {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "ops:sweep", &o).unwrap();
        assert!(out.contains("dead_letter { enabled false }"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_dead_letter_full() {
        let o = JobOptions {
            dead_letter: Some(DeadLetterPayload {
                enabled: Some(true),
                retention: Some("60d".into()),
                operator_hint: Some("check billing db".into()),
                replay_max_age: Some("7d".into()),
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "billing:invoice", &o).unwrap();
        assert!(
            out.contains(
                "dead_letter { enabled true; retention 60d; operator_hint \"check billing db\"; replay_max_age 7d }"
            ),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_no_dead_letter_omits_block() {
        let out = format_job_block_inner(&interval5(), "a:b", &JobOptions::default()).unwrap();
        assert!(!out.contains("dead_letter"), "{out}");
    }

    #[test]
    fn job_block_rejects_invalid_key() {
        assert!(format_job_block_inner(&interval5(), "nokey", &JobOptions::default()).is_err());
    }

    #[test]
    fn job_block_full_house_round_trips() {
        let o = JobOptions {
            description: Some("Full example".into()),
            timeout: Some("30m".into()),
            retry: Some(RetryPayload {
                strategy: "exponential".into(),
                max_attempts: Some(3),
                base: Some("2s".into()),
                ..Default::default()
            }),
            runner_require: vec!["billing".into()],
            runner_prefer: vec![],
            tags: vec!["env=prod".into()],
            concurrency: Some("singleton".into()),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "billing:invoice", &o).unwrap();
        // Must round-trip through the parser cleanly.
        Parser::parse(&out).unwrap();
    }

    // ── Schedule-options + scheduling directives (Phase 2) ─────────

    #[test]
    fn job_block_schedule_options_attach_to_every() {
        let o = JobOptions {
            schedule_calendar: Some("business-days".into()),
            schedule_timezone: Some("Europe/Vienna".into()),
            not_before: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "reports:daily", &o).unwrap();
        // The block canonicalises to a multi-line schedule block.
        assert!(out.contains("every 5 minutes {"), "{out}");
        assert!(out.contains("calendar business-days"), "{out}");
        assert!(out.contains("timezone Europe/Vienna"), "{out}");
        assert!(out.contains("not_before 2026-01-01T00:00:00Z"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_schedule_options_dropped_on_once() {
        // `once` schedules can't carry a schedule-options block.
        let once = SchedulePayload::Once {
            at: "2026-12-31T23:00:00Z".into(),
        };
        let o = JobOptions {
            schedule_calendar: Some("business-days".into()),
            ..Default::default()
        };
        let out = format_job_block_inner(&once, "migration:v2", &o).unwrap();
        assert!(
            !out.contains("calendar"),
            "options must be dropped on once: {out}"
        );
        assert!(out.contains("once at 2026-12-31T23:00:00Z"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_window_and_execution_mode() {
        let o = JobOptions {
            window: Some("02:00..06:00".into()),
            execution_mode: Some("ephemeral".into()),
            catch_up: Some("latest".into()),
            max_queue_depth: Some(100),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "ops:sweep", &o).unwrap();
        assert!(out.contains("window 02:00..06:00"), "{out}");
        assert!(out.contains("execution_mode ephemeral"), "{out}");
        assert!(out.contains("catch_up latest"), "{out}");
        assert!(out.contains("max_queue_depth 100"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn job_block_queue_ttl_none() {
        let o = JobOptions {
            queue_ttl: Some("none".into()),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "a:b", &o).unwrap();
        assert!(out.contains("queue_ttl none"), "{out}");
        Parser::parse(&out).unwrap();
    }

    // ── Top-level config blocks (Phase 3a) ─────────────────────────

    fn dir(key: &str, args: &[&str]) -> DirectivePayload {
        DirectivePayload {
            key: key.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            qualifier: None,
            quote_qualifier: false,
            children: vec![],
        }
    }

    fn block(
        key: &str,
        qualifier: Option<&str>,
        children: Vec<DirectivePayload>,
    ) -> DirectivePayload {
        DirectivePayload {
            key: key.into(),
            args: vec![],
            qualifier: qualifier.map(|s| s.to_string()),
            quote_qualifier: false,
            children,
        }
    }

    /// Sub-block with a force-quoted qualifier (alerts channel/rule name).
    fn qblock(key: &str, name: &str, children: Vec<DirectivePayload>) -> DirectivePayload {
        DirectivePayload {
            key: key.into(),
            args: vec![],
            qualifier: Some(name.into()),
            quote_qualifier: true,
            children,
        }
    }

    #[test]
    fn alerts_channels_and_rules() {
        let dirs = vec![
            qblock(
                "channel",
                "oncall",
                vec![dir("shell", &["/usr/bin/page-oncall.sh"])],
            ),
            qblock(
                "rule",
                "prod-failures",
                vec![
                    dir("when", &["job_failed"]),
                    dir("job_key", &["billing:*"]),
                    dir("channels", &["oncall"]),
                ],
            ),
        ];
        let out = format_top_level_block_inner("alerts", &dirs).unwrap();
        assert!(out.contains("channel \"oncall\" {"), "{out}");
        assert!(out.contains("rule \"prod-failures\" {"), "{out}");
        assert!(out.contains("when job_failed"), "{out}");
        assert!(out.contains("job_key billing:*"), "{out}");
        assert!(out.contains("channels oncall"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn alerts_webhook_channel_email_channel() {
        let dirs = vec![
            qblock(
                "channel",
                "hook",
                vec![
                    dir("webhook", &["https://hooks.example.com/x"]),
                    dir("timeout", &["10s"]),
                ],
            ),
            qblock(
                "channel",
                "team",
                vec![dir("email", &["a@example.com", "b@example.com"])],
            ),
        ];
        let out = format_top_level_block_inner("alerts", &dirs).unwrap();
        assert!(out.contains("channel \"hook\" {"), "{out}");
        assert!(out.contains("webhook https://hooks.example.com/x"), "{out}");
        assert!(out.contains("channel \"team\" {"), "{out}");
        assert!(out.contains("email a@example.com b@example.com"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn top_level_server_block() {
        let dirs = vec![
            dir("listen", &[":4000"]),
            dir("data_dir", &["/var/lib/croniq"]),
            dir("db", &["sqlite"]),
        ];
        let out = format_top_level_block_inner("server", &dirs).unwrap();
        assert_eq!(
            out,
            "server {\n  listen :4000\n  data_dir /var/lib/croniq\n  db sqlite\n}\n"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn top_level_smtp_from_is_quoted() {
        // `from` has spaces + <> → must be quoted.
        let dirs = vec![
            dir("host", &["smtp.example.com"]),
            dir("port", &["587"]),
            dir("security", &["starttls"]),
            dir("from", &["Croniq <noreply@example.com>"]),
        ];
        let out = format_top_level_block_inner("smtp", &dirs).unwrap();
        assert!(
            out.contains("from \"Croniq <noreply@example.com>\""),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn top_level_mcp_multi_arg_and_bool() {
        let dirs = vec![
            dir("enabled", &["true"]),
            dir("allowed_hosts", &["localhost:8443", "[::1]:8443"]),
        ];
        let out = format_top_level_block_inner("mcp", &dirs).unwrap();
        // `[::1]:8443` contains `[` `]` → quoted; the plain host stays bare.
        assert!(
            out.contains("allowed_hosts localhost:8443 \"[::1]:8443\""),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn top_level_vars_arbitrary_entries() {
        let dirs = vec![
            dir("default_tz", &["Europe/Vienna"]),
            dir("region", &["eu"]),
        ];
        let out = format_top_level_block_inner("vars", &dirs).unwrap();
        assert!(out.contains("default_tz Europe/Vienna"), "{out}");
        assert!(out.contains("region eu"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn top_level_skips_empty_and_errors_when_all_empty() {
        // Empty keys/args are skipped.
        let dirs = vec![
            dir("listen", &[":4000"]),
            dir("", &["ignored"]),
            dir("db", &[""]),
        ];
        let out = format_top_level_block_inner("server", &dirs).unwrap();
        assert_eq!(out, "server {\n  listen :4000\n}\n");
        // Nothing set at all → error (the UI shows a hint instead).
        assert!(format_top_level_block_inner("server", &[]).is_err());
    }

    #[test]
    fn top_level_rejects_unknown_block() {
        assert!(format_top_level_block_inner("not_a_block", &[dir("x", &["y"])]).is_err());
    }

    // ── Nested config blocks (Phase 3b) ────────────────────────────

    #[test]
    fn observability_nested_sub_blocks() {
        let dirs = vec![
            block(
                "log",
                None,
                vec![
                    dir("level", &["info"]),
                    dir("format", &["json"]),
                    dir("output", &["stderr"]),
                ],
            ),
            block(
                "metrics",
                None,
                vec![dir("listen", &[":9900"]), dir("path", &["/metrics"])],
            ),
        ];
        let out = format_top_level_block_inner("observability", &dirs).unwrap();
        assert!(
            out.contains("log { level info; format json; output stderr }"),
            "{out}"
        );
        assert!(
            out.contains("metrics { listen :9900; path /metrics }"),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn observability_empty_sub_block_is_skipped() {
        // A sub-block whose children are all blank must not emit an empty `{}`.
        let dirs = vec![
            block("log", None, vec![dir("level", &["info"])]),
            block("metrics", None, vec![dir("listen", &[""])]),
        ];
        let out = format_top_level_block_inner("observability", &dirs).unwrap();
        assert!(out.contains("log {"), "{out}");
        assert!(
            !out.contains("metrics"),
            "empty metrics sub-block must be skipped: {out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn defaults_flat_plus_nested_retry() {
        let dirs = vec![
            dir("timezone", &["Europe/Vienna"]),
            dir("timeout", &["5m"]),
            block(
                "retry",
                Some("exponential"),
                vec![dir("max_attempts", &["3"]), dir("base", &["2s"])],
            ),
            block("dead_letter", None, vec![dir("retention", &["30d"])]),
        ];
        let out = format_top_level_block_inner("defaults", &dirs).unwrap();
        assert!(out.contains("timezone Europe/Vienna"), "{out}");
        assert!(
            out.contains("retry exponential { max_attempts 3; base 2s }"),
            "{out}"
        );
        assert!(out.contains("dead_letter { retention 30d }"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn oidc_flat_block_quotes_urls_only_when_needed() {
        let dirs = vec![
            dir("issuer", &["https://id.example.com"]),
            dir("client_id", &["croniq"]),
            dir("default_role", &["viewer"]),
        ];
        let out = format_top_level_block_inner("oidc", &dirs).unwrap();
        assert!(out.contains("issuer https://id.example.com"), "{out}");
        assert!(out.contains("client_id croniq"), "{out}");
        Parser::parse(&out).unwrap();
    }

    // ── Placeholder quoting + runner shell/exec (Phase 3c) ─────────

    #[test]
    fn placeholders_stay_bare() {
        assert!(is_placeholder("{env.X}"));
        assert!(is_placeholder("{vars.default_tz}"));
        assert!(!is_placeholder("plain"));
        assert!(!is_placeholder("{a}{b}"));
        // A placeholder arg must NOT be quoted (else it becomes a literal).
        assert_eq!(
            quote_if_needed("{env.CRONIQ_JWT_SECRET}"),
            "{env.CRONIQ_JWT_SECRET}"
        );
        // A pull_api directive with a placeholder arg round-trips as a placeholder.
        let dirs = vec![dir("listen", &["{env.CRONIQ_PULL_LISTEN}"])];
        let out = format_top_level_block_inner("pull_api", &dirs).unwrap();
        assert!(out.contains("listen {env.CRONIQ_PULL_LISTEN}"), "{out}");
        assert!(
            !out.contains("\"{env"),
            "placeholder must not be quoted: {out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn runner_exec_shell_with_env_placeholder() {
        let o = JobOptions {
            runner_exec: Some(RunnerExecPayload {
                mode: "shell".into(),
                command: Some("pg_dump -U app app > /backups/app.sql".into()),
                workdir: Some("/opt".into()),
                env: vec![EnvPair {
                    key: "PGPASSWORD".into(),
                    value: "{env.PGPASSWORD}".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "db:backup", &o).unwrap();
        assert!(out.contains("runner shell {"), "{out}");
        assert!(
            out.contains("command \"pg_dump -U app app > /backups/app.sql\""),
            "{out}"
        );
        assert!(out.contains("workdir /opt"), "{out}");
        // env placeholder stays bare inside the env sub-block.
        assert!(out.contains("env { PGPASSWORD {env.PGPASSWORD} }"), "{out}");
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn runner_exec_exec_args() {
        let o = JobOptions {
            runner_exec: Some(RunnerExecPayload {
                mode: "exec".into(),
                args: vec!["/usr/sbin/logrotate".into(), "/etc/logrotate.conf".into()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "ops:logrotate", &o).unwrap();
        assert!(
            out.contains("runner exec { args /usr/sbin/logrotate /etc/logrotate.conf }"),
            "{out}"
        );
        Parser::parse(&out).unwrap();
    }

    #[test]
    fn runner_exec_shell_without_command_is_omitted() {
        let o = JobOptions {
            runner_exec: Some(RunnerExecPayload {
                mode: "shell".into(),
                workdir: Some("/opt".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = format_job_block_inner(&interval5(), "a:b", &o).unwrap();
        assert!(
            !out.contains("runner"),
            "no command → no runner block: {out}"
        );
        Parser::parse(&out).unwrap();
    }
}
