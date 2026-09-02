//! Stable fingerprint of a compiled job definition (issue #555).
//!
//! `run_on_register` needs one durable question answered across restarts:
//! *has this exact definition already been reconciled?* A boolean cannot
//! answer it — the directive must fire again when the definition changes, but
//! not on every reload — so the server persists the hash it last fired for,
//! per job key, and compares.
//!
//! The hash is taken over the **compiled** job, not the source text: a
//! reformatted Croniqfile, a moved `import`, a re-ordered directive list and a
//! comment edit all compile to the same `JobConfig` and must not fire
//! anything. Two properties therefore matter more than the choice of digest:
//!
//! - **Deterministic.** [`JobConfig::metadata`] is a `HashMap`, whose
//!   iteration order varies per process, so the JSON is canonicalised (object
//!   keys sorted, recursively) before hashing. Without that, every restart
//!   would look like a config change to half the jobs.
//! - **Complete by default.** The fingerprint serialises the whole
//!   `JobConfig` and then removes an explicit deny-list, rather than listing
//!   the fields it covers. A field added to `JobConfig` later is covered
//!   automatically; excluding it has to be a decision someone writes down
//!   here. (Contrast `reload::job_changed`, which is deliberately a brittle
//!   allow-list — it decides what to *log* as changed, not whether to run
//!   someone's credential rotation.)
//!
//! Everything that shapes *what the job does or when* is in: schedule,
//! timezone, calendar, window, bounds, runner placement and exec payload,
//! retry, timeout, dead-letter, metadata, execution mode, catch-up, queue
//! knobs, concurrency guard. See [`IGNORED_FIELDS`] for what is not, and why.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::compile::JobConfig;

/// Compiled-job fields the fingerprint ignores.
///
/// Two groups, both deliberate:
///
/// - **Identity** (`key`, `namespace`, `name`, `variant`): the stored hash is
///   already keyed by job key, so identity cannot differ between the two
///   values being compared. Renaming a job produces a *new* key, which has no
///   row yet and therefore fires as a first registration — the identity
///   fields play no part in that either.
/// - **Cosmetic** (`description`, `schedule_summary`, `tags`): a job's prose,
///   the rendered form of a schedule that is itself hashed, and labels
///   documented as non-routing-relevant. Rewording a `description` must not
///   re-run a credential rotation; that is the kind of surprise fire that
///   makes operators stop trusting the directive and go back to
///   hand-triggering.
///
/// `run_on_register` itself is excluded because it decides *whether* the hash
/// is consulted at all: a job that just gained the directive has no stored row
/// (so it fires), and one that just lost it has its row dropped (so re-adding
/// it fires again). Hashing the flag would change nothing in either case.
const IGNORED_FIELDS: &[&str] = &[
    "key",
    "namespace",
    "name",
    "variant",
    "description",
    "schedule_summary",
    "tags",
    "run_on_register",
];

impl JobConfig {
    /// Hex SHA-256 of this job's compiled definition — the value
    /// `run_on_register` compares against the last-fired hash in the store.
    ///
    /// Stable across processes and across cosmetic edits to the Croniqfile;
    /// changes whenever any behavioural field does. See the module docs.
    pub fn config_hash(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(map) = value.as_object_mut() {
            for field in IGNORED_FIELDS {
                map.remove(*field);
            }
        }

        let mut canonical = String::new();
        write_canonical(&value, &mut canonical);

        let digest = Sha256::digest(canonical.as_bytes());
        format!("{digest:x}")
    }
}

/// Render `value` with every object's keys in sorted order.
///
/// `serde_json::to_string` is already deterministic *given* a `Value`, but a
/// `Value::Object` is only key-sorted while serde_json is built without its
/// `preserve_order` feature — a feature any crate in the dependency graph can
/// turn on for everyone. Sorting here does not depend on that: the digest
/// cannot start moving because an unrelated dependency was added.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        // Scalars: serde_json's own escaping/number formatting is canonical
        // enough — it is a pure function of the value.
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::compile::compile;
    use crate::parser::Parser;
    use pretty_assertions::assert_eq;

    /// Compile `src` and return the single job it defines.
    fn job(src: &str) -> crate::compile::JobConfig {
        let ast = Parser::parse(src).expect("parses");
        let runtime = compile(&ast);
        assert_eq!(runtime.jobs.len(), 1, "fixture must define exactly one job");
        runtime.jobs.into_iter().next().unwrap()
    }

    fn hash(src: &str) -> String {
        job(src).config_hash()
    }

    #[test]
    fn hash_is_stable_across_calls() {
        let j = job(r#"job etl:sync { every 15 minutes }"#);
        assert_eq!(j.config_hash(), j.config_hash());
    }

    #[test]
    fn hash_is_stable_across_metadata_iteration_order() {
        // The compiler stores metadata in a `HashMap`, so this is the one
        // field whose serialisation order genuinely varies between processes
        // — and between two maps built in a different insert order in the
        // same process. Both must hash the same or every restart looks like a
        // config change.
        let mut a = job(r#"job etl:sync { every 15 minutes }"#);
        let mut b = a.clone();
        for (k, v) in [("alpha", "1"), ("beta", "2"), ("gamma", "3")] {
            a.metadata.insert(k.into(), v.into());
        }
        for (k, v) in [("gamma", "3"), ("alpha", "1"), ("beta", "2")] {
            b.metadata.insert(k.into(), v.into());
        }
        assert_eq!(a.config_hash(), b.config_hash());
    }

    #[test]
    fn hash_is_a_64_char_lowercase_hex_digest() {
        let h = hash(r#"job etl:sync { every 15 minutes }"#);
        assert_eq!(h.len(), 64, "{h}");
        assert!(
            h.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{h}"
        );
    }

    #[test]
    fn formatting_only_edits_do_not_change_the_hash() {
        // Same compiled job, three source spellings: inline vs. block body,
        // a comment, and a different directive order.
        let inline = hash(r#"job etl:sync { every 15 minutes; timeout 9m; singleton }"#);
        let block = hash(
            r#"
            job etl:sync {
              every 15 minutes
              # rotate the upstream credential
              singleton
              timeout 9m
            }
            "#,
        );
        assert_eq!(inline, block);
    }

    #[test]
    fn description_change_does_not_change_the_hash() {
        let before = hash(r#"job etl:sync { every 15 minutes; description "syncs" }"#);
        let after = hash(r#"job etl:sync { every 15 minutes; description "syncs upstream" }"#);
        assert_eq!(before, after);
    }

    #[test]
    fn tags_change_does_not_change_the_hash() {
        let before = hash(r#"job etl:sync { every 15 minutes; tags "env=prod" }"#);
        let after = hash(r#"job etl:sync { every 15 minutes; tags "env=prod" "team=data" }"#);
        assert_eq!(before, after);
    }

    #[test]
    fn job_key_does_not_change_the_hash() {
        // Two different jobs with the same body hash alike: the stored hash is
        // keyed by job key, so identity never distinguishes the two values
        // that get compared.
        let a = hash(r#"job etl:sync { every 15 minutes }"#);
        let b = hash(r#"job etl:other { every 15 minutes }"#);
        assert_eq!(a, b);
    }

    #[test]
    fn adding_run_on_register_does_not_itself_change_the_hash() {
        let without = hash(r#"job etl:sync { every 15 minutes }"#);
        let with = hash(r#"job etl:sync { every 15 minutes; run_on_register }"#);
        assert_eq!(without, with);
    }

    // ── Behavioural fields that must move the hash ───────────────────────────

    #[test]
    fn schedule_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes }"#),
            hash(r#"job etl:sync { every 5 minutes }"#)
        );
    }

    #[test]
    fn timezone_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every day at 04:20; timezone UTC }"#),
            hash(r#"job etl:sync { every day at 04:20; timezone Europe/Vienna }"#)
        );
    }

    #[test]
    fn timeout_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; timeout 5m }"#),
            hash(r#"job etl:sync { every 15 minutes; timeout 2h }"#)
        );
    }

    #[test]
    fn runner_requirement_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; runner { require alpha } }"#),
            hash(r#"job etl:sync { every 15 minutes; runner { require beta } }"#)
        );
    }

    #[test]
    fn metadata_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; metadata { env prod } }"#),
            hash(r#"job etl:sync { every 15 minutes; metadata { env staging } }"#)
        );
    }

    #[test]
    fn shell_command_change_changes_the_hash() {
        // The whole point of the directive: the thing the job *does* changed,
        // so reconcile now rather than at 04:20 tomorrow.
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; runner shell { command "sync.sh" } }"#),
            hash(r#"job etl:sync { every 15 minutes; runner shell { command "sync.sh --v2" } }"#)
        );
    }

    #[test]
    fn window_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; window 08:00..18:00 }"#),
            hash(r#"job etl:sync { every 15 minutes; window 09:00..18:00 }"#)
        );
    }

    #[test]
    fn concurrency_guard_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; singleton }"#),
            hash(r#"job etl:sync { every 15 minutes; max_concurrent 3 }"#)
        );
    }

    #[test]
    fn retry_change_changes_the_hash() {
        assert_ne!(
            hash(r#"job etl:sync { every 15 minutes; retry exponential { max_attempts 3 } }"#),
            hash(r#"job etl:sync { every 15 minutes; retry exponential { max_attempts 7 } }"#)
        );
    }
}
