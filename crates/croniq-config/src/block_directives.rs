//! Known-directive tables for the operator-facing configuration blocks
//! (`server { }`, `pull_api { }`, `defaults { }`, …) plus the checks that
//! reject anything not in them (issue #403).
//!
//! Why this exists: [`crate::compile`] matches directive keys per block with a
//! trailing `_ => {}`, so a typo'd key used to compile clean and leave the
//! server running with the default. That is the worst failure mode for knobs
//! whose absence is invisible in the short term — a mistyped
//! `execution_retention` keeps run history forever and nothing misbehaves.
//! Unknown *top-level* blocks and unknown calendar rule types are already hard
//! parse errors, so a typo one nesting level in should not be silent either.
//!
//! The tables below are the second half of a pair: **every key `compile.rs`
//! matches must be listed here, and nothing else.** A key added to `compile.rs`
//! without being added here becomes a hard config error — the
//! `example_croniqfile_has_no_unknown_directives` test in this module guards
//! that direction by validating the shipped `Croniqfile.example`.
//!
//! `job { }` joined the tables in issue #426: its body had the same hole one
//! level down — `timezone Europe/Vienna` written as a bare job directive
//! compiled to nothing, and so did every other typo, because
//! [`crate::validate`] only ever inspected *specific* job directives
//! (`singleton`, `max_concurrent`, the `runner` blocks) and never the key set
//! as a whole. Only the job body's own directives and sub-block *names* are
//! checked here; the bodies of `runner … { }` and `metadata { }` are not
//! (the first has per-qualifier rules in [`crate::validate`], the second takes
//! operator-chosen keys).
//!
//! Out of scope on purpose: `alerts { }`. Channel/rule bodies carry their own
//! validation in [`crate::validate`], and alert channel *kind* directives
//! (`shell`, `webhook`, `email`) are matched positionally rather than from a
//! fixed key set.

use crate::ast::*;
use crate::validate::{Diagnostic, Severity};

// ─── Known keys, per block ────────────────────────────────────────────────────

/// `server { }` — see `compile::compile`'s `Item::Server` arm.
const SERVER: &[&str] = &["listen", "data_dir", "db", "app_url", "execution_retention"];

/// `pull_api { }`. The runner-token signing secret is deliberately absent —
/// it lives in `CRONIQ_JWT_SECRET` / `$DATA_DIR/jwt.secret` (see [`REMOVED`]).
const PULL_API: &[&str] = &[
    "listen",
    "lease_ttl",
    "trigger_dedup_window",
    "runner_identity_binding",
];

/// `mcp { }`
const MCP: &[&str] = &["enabled", "allowed_hosts"];

/// `policy { }`
const POLICY: &[&str] = &["dsl_adopt_on_mutate", "strict_calendars"];

/// `oidc { }`. `client_secret` is not a directive by design — secrets stay in
/// the environment — so a `client_secret` line here is an unknown directive.
const OIDC: &[&str] = &[
    "issuer",
    "client_id",
    "redirect_url",
    "default_role",
    "provider_name",
    "post_login_redirect",
];

/// `smtp { }`. Credentials stay in `CRONIQ_SMTP_USERNAME` / `_PASSWORD`.
const SMTP: &[&str] = &["host", "port", "security", "from"];

/// `defaults { }` scalar directives (its sub-blocks are in [`DEFAULTS_BLOCKS`]).
const DEFAULTS: &[&str] = &[
    "timezone",
    "timeout",
    "execution_mode",
    "catch_up",
    "queue_ttl",
    "max_queue_depth",
    "keep_last",
];

/// `defaults { retry … { } }` / `defaults { dead_letter { } }`.
const DEFAULTS_BLOCKS: &[(&str, &[&str])] = &[
    (
        "retry",
        &["max_attempts", "base", "cap", "delay", "step", "jitter"],
    ),
    (
        "dead_letter",
        &["enabled", "retention", "operator_hint", "replay_max_age"],
    ),
];

/// `job { }` scalar directives — see `compile::compile_job`'s directive match.
///
/// `timezone` is in the list because issue #426 made the job-level spelling
/// real: it is what operators reach for, and `defaults { }` already accepts the
/// same bare keyword.
///
/// Not here, and not typos: `calendar`, `not_before`, `not_after` and
/// `timezone` are also *schedule options* — `every day at 02:00 { calendar biz }`
/// — which live on the schedule line, not in the job body. Of those only
/// `timezone` is meaningful as a job directive; the other three stay
/// schedule-only, so writing them bare in the body is an unknown directive with
/// a message that says where they belong.
const JOB: &[&str] = &[
    "description",
    "timezone",
    "timeout",
    "window",
    "execution_mode",
    "catch_up",
    "queue_ttl",
    "max_queue_depth",
    "keep_last",
    "singleton",
    "max_concurrent",
    "concurrency_group",
    "tags",
    "run_on_register",
];

/// `concurrency_group <name> { }` (issue #546). One directive, and it is not
/// optional — a group without a limit has no budget to share, so `validate`
/// rejects the empty body rather than defaulting it.
const CONCURRENCY_GROUP: &[&str] = &["max_concurrent"];

/// Sub-blocks a `job { }` accepts. Bodies are checked only where the key set is
/// fixed: `retry` / `dead_letter` share their tables with `defaults { }`,
/// `metadata { }` takes arbitrary operator keys, and `runner … { }` is
/// validated per qualifier in [`crate::validate`].
const JOB_BLOCKS: &[&str] = &["runner", "retry", "dead_letter", "metadata"];

/// Schedule-only option keys, for a diagnostic that points at the schedule line
/// instead of reading like a typo.
const SCHEDULE_ONLY: &[&str] = &["calendar", "not_before", "not_after"];

/// `observability { }` sub-blocks. The outer block takes no directives — the
/// parser already rejects those — so only the names and bodies need checking.
const OBSERVABILITY_BLOCKS: &[(&str, &[&str])] = &[
    ("log", &["level", "format", "output"]),
    ("metrics", &["listen", "path"]),
];

/// `auth { }` sub-blocks. Same shape as `observability { }`.
const AUTH_BLOCKS: &[(&str, &[&str])] = &[("password", &["enabled"]), ("totp", &["required"])];

// ─── Directives that were removed, and must not be silently ignored ──────────

/// A directive an older Croniqfile may still carry. Left unlisted it would be
/// reported as a plain typo; the tailored message names the migration instead.
struct RemovedDirective {
    /// Block name as it appears in the DSL.
    block: &'static str,
    key: &'static str,
    /// Full diagnostic message, including where the value has to go now.
    message: &'static str,
}

/// `pull_api { auth … }` (issues #371, #408). Removed in 0.29.0 and ignored
/// since — which is unsafe, because that value was the JWT signing secret and
/// therefore also the HKDF input for the key that wraps stored TOTP secrets at
/// rest. Dropping the line silently rotates the wrap key and makes every
/// enrolled TOTP secret undecryptable, so this fails closed with the migration
/// spelled out instead.
const REMOVED: &[RemovedDirective] = &[RemovedDirective {
    block: "pull_api",
    key: "auth",
    message: "`pull_api { auth … }` was removed in 0.29.0 — it was never an auth on/off switch, \
              its value was used verbatim as the JWT signing secret. Move that exact value to \
              the CRONIQ_JWT_SECRET env var (or $DATA_DIR/jwt.secret) and delete this line. \
              Leaving it here and letting the secret change also changes the key that wraps \
              stored TOTP secrets at rest, which makes them undecryptable (recovery codes keep \
              working). If no user has enrolled TOTP, any new secret is safe.",
}];

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Check every operator-facing block for unknown / removed directive keys and
/// unknown sub-block names. Called from [`crate::validate::validate`].
pub(crate) fn validate_blocks(ast: &Croniqfile, diags: &mut Vec<Diagnostic>) {
    for item in &ast.items {
        match item {
            Item::Server(b) => check_directives("server", SERVER, &b.directives, diags),
            Item::PullApi(b) => check_directives("pull_api", PULL_API, &b.directives, diags),
            Item::Mcp(b) => check_directives("mcp", MCP, &b.directives, diags),
            Item::Policy(b) => check_directives("policy", POLICY, &b.directives, diags),
            Item::Oidc(b) => check_directives("oidc", OIDC, &b.directives, diags),
            Item::Smtp(b) => check_directives("smtp", SMTP, &b.directives, diags),
            Item::Defaults(b) => {
                for dob in &b.directives {
                    match dob {
                        DirectiveOrBlock::Directive(d) => {
                            check_directive("defaults", DEFAULTS, d, diags)
                        }
                        DirectiveOrBlock::Block(nb) => {
                            check_named_block("defaults", DEFAULTS_BLOCKS, nb, diags)
                        }
                        DirectiveOrBlock::Comment(_) => {}
                    }
                }
            }
            Item::Observability(b) => {
                for nb in &b.sub_blocks {
                    check_named_block("observability", OBSERVABILITY_BLOCKS, nb, diags);
                }
            }
            Item::Auth(b) => {
                for nb in &b.sub_blocks {
                    check_named_block("auth", AUTH_BLOCKS, nb, diags);
                }
            }
            Item::ConcurrencyGroup(b) => {
                let label = format!("concurrency_group {}", b.name.value);
                check_directives(&label, CONCURRENCY_GROUP, &b.directives, diags);
            }
            Item::Job(job) => check_job_body(job, diags),
            // `vars { }` entries are operator-chosen names; `alerts { }`,
            // `calendar { }` and `import` are validated elsewhere.
            _ => {}
        }
    }
}

/// Check a `job { }` body: directive keys against [`JOB`], sub-block names
/// against [`JOB_BLOCKS`], and the two sub-blocks whose key set is fixed.
fn check_job_body(job: &JobBlock, diags: &mut Vec<Diagnostic>) {
    let label = format!("job {}", job.key.raw);
    for dob in &job.directives {
        match dob {
            DirectiveOrBlock::Directive(d) => {
                let key = d.key.value.as_str();
                if JOB.contains(&key) {
                    continue;
                }
                if SCHEDULE_ONLY.contains(&key) {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!(
                            "`{key}` is a schedule option, not a job directive — move it into the \
                             schedule line's braces, e.g. `every day at 02:00 {{ {key} … }}` \
                             (job '{}')",
                            job.key.raw
                        ),
                        span: d.key.span.into(),
                    });
                    continue;
                }
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!(
                        "unknown directive '{key}' in `{label} {{ }}`{}",
                        hint(key, JOB)
                    ),
                    span: d.key.span.into(),
                });
            }
            DirectiveOrBlock::Block(nb) => {
                let name = nb.name.value.as_str();
                if !JOB_BLOCKS.contains(&name) {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!(
                            "unknown block '{name} {{ }}' in `{label} {{ }}`{}",
                            hint(name, JOB_BLOCKS)
                        ),
                        span: nb.name.span.into(),
                    });
                    continue;
                }
                // `retry` / `dead_letter` inherit their key sets from
                // `defaults { }` — the same compile helpers back both — so a
                // typo inside a job's block is caught exactly as it is there.
                if let Some((_, keys)) = DEFAULTS_BLOCKS.iter().find(|(n, _)| *n == name) {
                    let path = format!("{label}.{name}");
                    for inner in &nb.directives {
                        if let DirectiveOrBlock::Directive(d) = inner {
                            check_directive(&path, keys, d, diags);
                        }
                    }
                }
            }
            DirectiveOrBlock::Comment(_) => {}
        }
    }
}

fn check_directives(
    block: &str,
    known: &[&str],
    directives: &[Directive],
    diags: &mut Vec<Diagnostic>,
) {
    for d in directives {
        check_directive(block, known, d, diags);
    }
}

fn check_directive(block: &str, known: &[&str], d: &Directive, diags: &mut Vec<Diagnostic>) {
    let key = d.key.value.as_str();
    if known.contains(&key) {
        return;
    }
    if let Some(removed) = REMOVED.iter().find(|r| r.block == block && r.key == key) {
        diags.push(Diagnostic {
            severity: Severity::Error,
            message: removed.message.to_string(),
            span: d.key.span.into(),
        });
        return;
    }
    diags.push(Diagnostic {
        severity: Severity::Error,
        message: format!(
            "unknown directive '{key}' in `{block} {{ }}`{}",
            hint(key, known)
        ),
        span: d.key.span.into(),
    });
}

/// Check a sub-block's name against `known`, then its body against the key set
/// that name maps to.
fn check_named_block(
    parent: &str,
    known: &[(&str, &[&str])],
    nb: &NamedBlock,
    diags: &mut Vec<Diagnostic>,
) {
    let name = nb.name.value.as_str();
    let Some((_, keys)) = known.iter().find(|(n, _)| *n == name) else {
        let names: Vec<&str> = known.iter().map(|(n, _)| *n).collect();
        diags.push(Diagnostic {
            severity: Severity::Error,
            message: format!(
                "unknown block '{name} {{ }}' in `{parent} {{ }}`{}",
                hint(name, &names)
            ),
            span: nb.name.span.into(),
        });
        return;
    };

    // Sub-block bodies are one level deep everywhere today, so the path used in
    // messages ("defaults.retry") is enough to locate the offending line.
    let path = format!("{parent}.{name}");
    for dob in &nb.directives {
        match dob {
            DirectiveOrBlock::Directive(d) => check_directive(&path, keys, d, diags),
            DirectiveOrBlock::Block(inner) => diags.push(Diagnostic {
                severity: Severity::Error,
                message: format!(
                    "unknown block '{} {{ }}' in `{path} {{ }}` — this block takes directives only",
                    inner.name.value
                ),
                span: inner.name.span.into(),
            }),
            DirectiveOrBlock::Comment(_) => {}
        }
    }
}

// ─── Suggestions ─────────────────────────────────────────────────────────────

/// ` — did you mean 'x'?` when something is close enough, otherwise the full
/// key list so the operator can see what the block actually accepts.
fn hint(got: &str, known: &[&str]) -> String {
    match closest(got, known) {
        Some(best) => format!(" — did you mean '{best}'?"),
        None => {
            let mut sorted: Vec<&str> = known.to_vec();
            sorted.sort_unstable();
            format!(" (known: {})", sorted.join(", "))
        }
    }
}

/// Nearest known key within a small edit distance, scaled so short keys don't
/// match everything (`db` must not suggest itself for `on`).
fn closest<'a>(got: &str, known: &[&'a str]) -> Option<&'a str> {
    let budget = match got.chars().count() {
        0..=3 => 1,
        4..=8 => 2,
        _ => 3,
    };
    known
        .iter()
        .map(|k| (*k, edit_distance(got, k)))
        .filter(|(_, d)| *d <= budget)
        .min_by_key(|(k, d)| (*d, k.len()))
        .map(|(k, _)| k)
}

/// Levenshtein distance, two-row DP. Inputs here are short directive keys.
/// Shared with [`crate::timezone`], which suggests the intended IANA zone for
/// a typo'd `timezone` value the same way.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0usize; b_chars.len() + 1];

    for (i, ac) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, bc) in b_chars.iter().enumerate() {
            let cost = usize::from(ac != *bc);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::validate::validate;

    fn errors(src: &str) -> Vec<String> {
        let ast = Parser::parse(src).unwrap();
        validate(&ast)
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn typo_in_server_block_errors_with_suggestion() {
        let msgs = errors("server { listen :4000\n execution_retentionn 90d }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert_eq!(
            msgs[0],
            "unknown directive 'execution_retentionn' in `server { }` — \
             did you mean 'execution_retention'?"
        );
    }

    #[test]
    fn unrelated_key_lists_the_known_set() {
        let msgs = errors("pull_api { frobnicate yes }");
        assert_eq!(
            msgs,
            vec![
                "unknown directive 'frobnicate' in `pull_api { }` \
                 (known: lease_ttl, listen, runner_identity_binding, trigger_dedup_window)"
            ]
        );
    }

    #[test]
    fn known_directives_are_silent() {
        let msgs = errors(
            r#"
            server { listen :4000; data_dir /var/lib/croniq; db sqlite
                     app_url "https://x.test"; execution_retention 30d }
            pull_api { listen :9443; lease_ttl 60s; trigger_dedup_window 10m }
            mcp { enabled true; allowed_hosts a.test b.test }
            policy { dsl_adopt_on_mutate true; strict_calendars false }
            oidc { issuer "https://i.test"; client_id cid; redirect_url "https://r.test"
                   default_role viewer; provider_name authentik; post_login_redirect "/" }
            smtp { host smtp.test; port 587; security starttls; from "C <n@test>" }
            observability { log { level info; format json; output stderr }
                            metrics { listen :9900; path /metrics } }
            auth { password { enabled true } totp { required false } }
            defaults {
              timezone UTC; timeout 5m; execution_mode queued; catch_up all
              queue_ttl 1h; max_queue_depth 10; keep_last 50
              retry exponential { max_attempts 3; base 2s; cap 30s; delay 1s; step 2s; jitter 0.25 }
              dead_letter { enabled true; retention 30d; operator_hint "page oncall"
                            replay_max_age 7d }
            }
        "#,
        );
        assert!(msgs.is_empty(), "unexpected errors: {msgs:?}");
    }

    #[test]
    fn typo_in_defaults_and_its_sub_blocks_errors() {
        let msgs = errors("defaults { timeoutt 5m\n retry { max_attemptss 3 } }");
        assert_eq!(msgs.len(), 2, "got: {msgs:?}");
        assert!(msgs[0].contains("unknown directive 'timeoutt' in `defaults { }`"));
        assert!(
            msgs[1].contains("unknown directive 'max_attemptss' in `defaults.retry { }`"),
            "got: {}",
            msgs[1]
        );
    }

    #[test]
    fn unknown_sub_block_name_errors() {
        let msgs = errors("observability { logs { level info } }");
        assert_eq!(
            msgs,
            vec!["unknown block 'logs { }' in `observability { }` — did you mean 'log'?"]
        );
    }

    #[test]
    fn unknown_auth_sub_block_errors() {
        let msgs = errors("auth { totpp { required true } }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(msgs[0].contains("unknown block 'totpp { }' in `auth { }`"));
    }

    #[test]
    fn removed_pull_api_auth_names_the_migration() {
        let msgs = errors("pull_api { auth some-secret-value }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(msgs[0].contains("CRONIQ_JWT_SECRET"), "got: {}", msgs[0]);
        assert!(msgs[0].contains("TOTP"), "got: {}", msgs[0]);
        // Must not read as a typo — the value has to be migrated, not fixed.
        assert!(!msgs[0].contains("did you mean"), "got: {}", msgs[0]);
    }

    // ── job { } bodies (issue #426) ───────────────────────────────────────────

    #[test]
    fn job_level_timezone_is_accepted() {
        // The spelling issue #426 made real: bare `timezone` in a job body.
        let msgs = errors("job a:b { every day at 02:00\n timezone Europe/Vienna }");
        assert!(msgs.is_empty(), "unexpected errors: {msgs:?}");
    }

    #[test]
    fn run_on_register_is_accepted_as_a_bare_job_directive() {
        // Issue #555. Bare like `singleton` — presence is the whole signal.
        let msgs = errors("job a:b { every day at 04:20\n run_on_register }");
        assert!(msgs.is_empty(), "unexpected errors: {msgs:?}");
    }

    #[test]
    fn typo_in_job_body_errors_with_suggestion() {
        let msgs = errors("job a:b { every day at 02:00\n timezon Europe/Vienna }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert_eq!(
            msgs[0],
            "unknown directive 'timezon' in `job a:b { }` — did you mean 'timezone'?"
        );
    }

    #[test]
    fn unrelated_job_directive_lists_the_known_set() {
        let msgs = errors("job a:b { every 5 minutes\n frobnicate yes }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(
            msgs[0].contains("(known: catch_up, concurrency_group, description,"),
            "got: {}",
            msgs[0]
        );
    }

    #[test]
    fn schedule_option_written_as_a_job_directive_says_where_it_belongs() {
        // `calendar biz` in the body is a no-op, and reads like a typo of
        // nothing — the fix is to move it, so the message says so.
        let msgs = errors("job a:b { every day at 02:00 { timezone UTC }\n calendar biz }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(
            msgs[0].contains("is a schedule option, not a job directive"),
            "got: {}",
            msgs[0]
        );
        assert!(
            msgs[0].contains("every day at 02:00 { calendar"),
            "got: {}",
            msgs[0]
        );
    }

    #[test]
    fn known_job_directives_and_blocks_are_silent() {
        let msgs = errors(
            r#"
            calendar biz { include weekly monday }
            job billing:invoice {
              description "…"; timezone Europe/Vienna; timeout 15m
              every weekday at 02:00 { calendar biz; not_before 2026-01-01T00:00:00Z
                                       not_after 2027-01-01T00:00:00Z }
              window 02:00..06:00
              execution_mode queued; catch_up all; queue_ttl 1h
              max_queue_depth 10; keep_last 500; max_concurrent 2
              tags billing nightly
              runner { require billing; prefer eu-central; sticky }
              retry exponential { max_attempts 3; base 2s; cap 30s; delay 1s; step 2s; jitter 0.25 }
              dead_letter { enabled true; retention 30d; operator_hint "page"; replay_max_age 7d }
              metadata { team billing; priority high }
            }
        "#,
        );
        assert!(msgs.is_empty(), "unexpected errors: {msgs:?}");
    }

    #[test]
    fn unknown_job_sub_block_errors() {
        let msgs = errors("job a:b { every 5 minutes\n runners { require x } }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(
            msgs[0].contains("unknown block 'runners { }' in `job a:b { }`"),
            "got: {}",
            msgs[0]
        );
    }

    #[test]
    fn typo_inside_a_job_retry_block_errors() {
        // `retry` / `dead_letter` share their key sets with `defaults { }`.
        let msgs = errors("job a:b { every 5 minutes\n retry { max_attemptss 3 } }");
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(
            msgs[0].contains("unknown directive 'max_attemptss' in `job a:b.retry { }`"),
            "got: {}",
            msgs[0]
        );
    }

    #[test]
    fn metadata_keys_are_operator_chosen_and_not_checked() {
        let msgs = errors("job a:b { every 5 minutes\n metadata { anything goes } }");
        assert!(msgs.is_empty(), "unexpected errors: {msgs:?}");
    }

    #[test]
    fn example_croniqfile_has_no_unknown_directives() {
        // Guards the compile.rs → table direction: a directive added to
        // `compile.rs` and documented in the shipped example, but missing from
        // the tables above, fails here instead of in an operator's boot log.
        let src = include_str!("../../../Croniqfile.example");
        let msgs = errors(src);
        assert!(msgs.is_empty(), "Croniqfile.example rejected: {msgs:?}");
    }

    // ── site/generator.js's config tab ⇄ these tables ───────────────────────

    /// Every block the DSL generator's config tab offers, paired with the
    /// table above that defines what that block accepts.
    ///
    /// `alerts` and `vars` are deliberately absent: the generator has bespoke
    /// editors for them and neither has a fixed key set here (see this
    /// module's header).
    /// A block's own directives, plus the `(name, keys)` table of its
    /// sub-blocks — the two halves [`check_directives`] and
    /// [`check_named_block`] consult.
    type GeneratorBlock = (
        &'static str,
        &'static [&'static str],
        &'static [(&'static str, &'static [&'static str])],
    );

    const GENERATOR_BLOCKS: &[GeneratorBlock] = &[
        ("server", SERVER, &[]),
        ("pull_api", PULL_API, &[]),
        ("mcp", MCP, &[]),
        ("policy", POLICY, &[]),
        ("oidc", OIDC, &[]),
        ("smtp", SMTP, &[]),
        ("auth", &[], AUTH_BLOCKS),
        ("observability", &[], OBSERVABILITY_BLOCKS),
        ("defaults", DEFAULTS, DEFAULTS_BLOCKS),
        ("concurrency_group", CONCURRENCY_GROUP, &[]),
    ];

    /// The `CONFIG_SCHEMA` region of `site/generator.js`, one entry per block.
    ///
    /// Line-based on purpose: the file is machine-formatted with two-space
    /// indentation, and a hand-rolled scan that fails loudly when that changes
    /// beats pulling a JS parser into this crate. Line endings are normalised
    /// because git hands the file out with CRLF on a Windows checkout.
    fn generator_config_schema(js: &str) -> std::collections::BTreeMap<String, Vec<String>> {
        let js = js.replace("\r\n", "\n");
        let start = js
            .find("const CONFIG_SCHEMA = {")
            .expect("site/generator.js must still define CONFIG_SCHEMA");
        let region = &js[start..];
        let end = region
            .find("\n}\n")
            .expect("CONFIG_SCHEMA must be closed at column 0");
        let region = &region[..end];

        let mut blocks: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        let mut current: Option<String> = None;
        for line in region.lines().skip(1) {
            // A block starts at exactly two spaces of indentation:
            //   `  server: [`  /  `  concurrency_group: {`
            if let Some(rest) = line.strip_prefix("  ")
                && !rest.starts_with(' ')
                && let Some((name, tail)) = rest.split_once(':')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !name.is_empty()
            {
                let tail = tail.trim();
                current = if tail.starts_with('[') || tail.starts_with('{') {
                    blocks.entry(name.to_string()).or_default();
                    Some(name.to_string())
                } else {
                    // `alerts: 'alerts'` / `vars: 'freeform'` — bespoke
                    // editors, no key list to check.
                    None
                };
                continue;
            }
            // Leaf/sub-block keys, wherever they nest: `{ key: 'listen', … }`.
            if let Some(block) = current.as_deref() {
                for (idx, _) in line.match_indices("key: '") {
                    let after = &line[idx + "key: '".len()..];
                    if let Some(key) = after.split('\'').next() {
                        blocks
                            .entry(block.to_string())
                            .or_default()
                            .push(key.to_string());
                    }
                }
            }
        }
        blocks
    }

    /// The public DSL generator's config tab is a hand-maintained mirror of
    /// the tables in this module, and nothing connected the two: a directive
    /// added here simply never appeared in the form, and a key the form
    /// misspells produced a block that the server would later reject as an
    /// unknown directive — found, if at all, by whoever pasted the output.
    ///
    /// Five job directives had accumulated that way before issue #555's
    /// follow-up (`crates/croniq-config-wasm` carries the matching guard for
    /// the *job* form, next to the payload structs it compares against). This
    /// is the same check for the config blocks, and it lives here because
    /// these tables are what it compares against.
    ///
    /// Both directions are checked. A key the form offers that no table has
    /// is a typo or a removed directive; a table key the form never offers is
    /// a knob nobody can reach from the generator.
    #[test]
    fn generator_config_tab_matches_the_known_directive_tables() {
        let schema = generator_config_schema(include_str!("../../../site/generator.js"));

        assert_eq!(
            schema.len(),
            GENERATOR_BLOCKS.len(),
            "generator blocks {:?} vs. expected {:?} — add the new block to \
             GENERATOR_BLOCKS (with the table it mirrors), or drop it there",
            schema.keys().collect::<Vec<_>>(),
            GENERATOR_BLOCKS
                .iter()
                .map(|(n, _, _)| n)
                .collect::<Vec<_>>(),
        );

        for (block, scalars, sub_blocks) in GENERATOR_BLOCKS {
            let offered = schema
                .get(*block)
                .unwrap_or_else(|| panic!("site/generator.js has no `{block}` block"));

            // Everything the block accepts: its own directives plus the keys
            // of every sub-block it takes. The generator flattens both into
            // `key: '…'` entries, so the comparison is over the union.
            let mut known: Vec<&str> = scalars.to_vec();
            for (_, keys) in sub_blocks.iter() {
                known.extend_from_slice(keys);
            }

            let unknown: Vec<&String> = offered
                .iter()
                .filter(|k| !known.contains(&k.as_str()))
                .collect();
            assert!(
                unknown.is_empty(),
                "site/generator.js offers {unknown:?} in `{block} {{ }}`, which is not a known \
                 directive — the generator would emit config the server rejects. Known: {known:?}"
            );

            let missing: Vec<&&str> = known
                .iter()
                .filter(|k| !offered.iter().any(|o| o == *k))
                .collect();
            assert!(
                missing.is_empty(),
                "`{block} {{ }}` accepts {missing:?} but site/generator.js never offers them, so \
                 they are unreachable from the generator"
            );
        }
    }

    /// A block in `CONFIG_SCHEMA` with no `<option>` in the picker is
    /// unreachable however complete its field list is, and an `<option>` with
    /// no schema entry renders an empty form. Neither fails loudly in a
    /// browser, so it is checked here alongside the schema itself.
    #[test]
    fn generator_block_picker_offers_exactly_the_configured_blocks() {
        let html = include_str!("../../../site/generator.html").replace("\r\n", "\n");
        let select = html
            .split_once("<select id=\"cfg-block\">")
            .expect("generator.html must still have the cfg-block picker")
            .1
            .split_once("</select>")
            .expect("cfg-block picker must be closed")
            .0;

        let mut offered: Vec<&str> = Vec::new();
        for (idx, _) in select.match_indices("<option value=\"") {
            let rest = &select[idx + "<option value=\"".len()..];
            offered.push(rest.split('"').next().unwrap_or(""));
        }

        let schema = generator_config_schema(include_str!("../../../site/generator.js"));
        for block in schema.keys() {
            assert!(
                offered.contains(&block.as_str()),
                "`{block}` is in CONFIG_SCHEMA but not in the block picker, so no one can select it"
            );
        }
        // The picker also carries `alerts` and `vars`, which have bespoke
        // editors rather than a schema entry.
        let bespoke = ["alerts", "vars"];
        for block in &offered {
            assert!(
                schema.contains_key(*block) || bespoke.contains(block),
                "the block picker offers `{block}`, which has neither a CONFIG_SCHEMA entry nor a \
                 bespoke editor — selecting it renders an empty form"
            );
        }
    }

    #[test]
    fn demo_croniqfile_has_no_unknown_directives() {
        let src = include_str!("../../../Croniqfile.demo");
        let msgs = errors(src);
        assert!(msgs.is_empty(), "Croniqfile.demo rejected: {msgs:?}");
    }

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("listen", "listen"), 0);
        assert_eq!(edit_distance("listen", "listenn"), 1);
        assert_eq!(edit_distance("lsiten", "listen"), 2);
        assert_eq!(edit_distance("db", "from"), 4);
    }

    #[test]
    fn closest_ignores_far_keys() {
        assert_eq!(closest("listenn", SERVER), Some("listen"));
        assert_eq!(closest("frobnicate", SERVER), None);
        // Short keys must not collapse into each other.
        assert_eq!(closest("on", SERVER), None);
    }
}
