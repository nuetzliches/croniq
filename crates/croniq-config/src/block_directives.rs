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
//! Out of scope on purpose: `job { }` and `alerts { }`. Job-level directives
//! and alert channel/rule bodies carry their own validation in
//! [`crate::validate`], and alert channel *kind* directives (`shell`,
//! `webhook`, `email`) are matched positionally rather than from a fixed key
//! set.

use crate::ast::*;
use crate::validate::{Diagnostic, Severity};

// ─── Known keys, per block ────────────────────────────────────────────────────

/// `server { }` — see `compile::compile`'s `Item::Server` arm.
const SERVER: &[&str] = &["listen", "data_dir", "db", "app_url", "execution_retention"];

/// `pull_api { }`. The runner-token signing secret is deliberately absent —
/// it lives in `CRONIQ_JWT_SECRET` / `$DATA_DIR/jwt.secret` (see [`REMOVED`]).
const PULL_API: &[&str] = &["listen", "lease_ttl", "trigger_dedup_window"];

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
            // `vars { }` entries are operator-chosen names; `job { }`,
            // `alerts { }`, `calendar { }` and `import` are validated elsewhere.
            _ => {}
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
fn edit_distance(a: &str, b: &str) -> usize {
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
                 (known: lease_ttl, listen, trigger_dedup_window)"
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

    #[test]
    fn example_croniqfile_has_no_unknown_directives() {
        // Guards the compile.rs → table direction: a directive added to
        // `compile.rs` and documented in the shipped example, but missing from
        // the tables above, fails here instead of in an operator's boot log.
        let src = include_str!("../../../Croniqfile.example");
        let msgs = errors(src);
        assert!(msgs.is_empty(), "Croniqfile.example rejected: {msgs:?}");
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
