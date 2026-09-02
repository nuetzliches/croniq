//! `run_on_register`: fire a job once when its definition is adopted
//! (issue #555).
//!
//! # The gap this closes
//!
//! A job that reconciles state which changes *at deploy time* — a rotating
//! credential pushed to an external component, a cache warmed from new
//! config — has a blind window between the deploy and its next scheduled
//! fire. Moving such a job out of the consuming app's start-up and into
//! Croniq fixes expiry (a start-only check cannot notice a credential ageing
//! out under a long-running container) but trades it for that window: a deploy
//! at 10:00 and `every day at 04:20` means 18 hours of the receiving component
//! holding the wrong value.
//!
//! `catch_up` does not help — it replays *missed* fires of an
//! already-registered trigger, and a newly registered job has none.
//!
//! # What "adoption" means
//!
//! Two events, and deliberately only these two:
//!
//! - the job key is seen for the first time, and
//! - the job's compiled [`JobConfig::config_hash`] changes.
//!
//! Explicitly **not** every reload or server restart. Otherwise a restart
//! storms every such job at once and so does every `--watch` save, which is
//! why the last-fired hash is persisted per job key
//! ([`croniq_store::models::JobRegisterFire`]) instead of tracked in memory.
//!
//! *Any* behavioural field counts, not just the schedule: the point of the
//! directive is "the definition changed, reconcile now", and a changed
//! `timeout` or runner requirement is a weaker signal of that but not a
//! harmful one. The fingerprint's deny-list is where the exceptions live
//! (prose and labels do not fire anything) — see
//! `croniq-config/src/fingerprint.rs`.
//!
//! # Gates
//!
//! A calendar, a `window` or `not_before` does not suppress the adoption
//! fire — it **defers** it to the next instant that gate permits, matching
//! what #391 established for scheduled fires ("run as soon as permitted"
//! rather than "skip"). A reconciler that must not run outside business hours
//! still has to run, and the first allowed instant is the answer the operator
//! wants. `not_after` is the one bound that suppresses: past it the job is
//! over, and there is no later instant to defer to.
//!
//! # Ordering and durability
//!
//! Planning is pure ([`plan`]) and runs on every config load — boot and each
//! reload. Dispatch happens in the scheduler tick, so a deferred fire survives
//! as an armed [`PendingRegisterFire`] until its instant arrives, and the fire
//! itself takes the normal path: an execution row, then a work item, so
//! `singleton` / `max_concurrent` are enforced at claim time exactly as they
//! are for a scheduled fire.
//!
//! The store row is written **after** dispatch. A crash in between therefore
//! leaves the job un-reconciled and the next boot fires it again: adoption is
//! at-least-once, which for a job whose whole purpose is to reconcile external
//! state is the safe direction to fail in.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use croniq_config::compile::JobConfig;
use croniq_scheduler::trigger::{Trigger, TriggerState};
use croniq_store::models::JobRegisterFire;

/// Why a job owes an adoption fire. Carried into the log line so an operator
/// reading it after a deploy can tell a new job from a changed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adoption {
    /// No record for this job key — first time Croniq has seen it (or the
    /// first boot after the directive was added).
    FirstRegistration,
    /// A record exists, for a different compiled definition.
    ConfigChanged,
}

impl Adoption {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstRegistration => "first_registration",
            Self::ConfigChanged => "config_changed",
        }
    }
}

/// An adoption fire that has been decided but not yet dispatched.
#[derive(Debug, Clone)]
pub struct PendingRegisterFire {
    pub job_key: String,
    /// The hash to record once the fire goes out. Captured at plan time so a
    /// concurrent reload cannot make the recorded hash disagree with the
    /// definition that actually fired.
    pub config_hash: String,
    /// The earliest instant the job's gates permit. `now` for an ungated job.
    pub due_at: DateTime<Utc>,
    pub reason: Adoption,
}

/// Decide which `run_on_register` jobs owe a fire, and when.
///
/// `recorded` is the store's last-fired hash per job key. Jobs without the
/// directive are ignored; so is a job whose trigger is paused — a job failed
/// closed on an unresolved calendar reference (#361) must not fire un-gated
/// here either, and it re-plans on the reload that fixes the fault, because
/// nothing has been recorded for it in the meantime.
///
/// Returns one entry per job that owes a fire, ordered by job key so a boot
/// log reads deterministically.
pub fn plan(
    jobs: &[JobConfig],
    triggers: &HashMap<String, Trigger>,
    recorded: &HashMap<String, String>,
    now: DateTime<Utc>,
) -> Vec<PendingRegisterFire> {
    let mut planned: Vec<PendingRegisterFire> = Vec::new();

    for job in jobs.iter().filter(|j| j.run_on_register) {
        let config_hash = job.config_hash();
        let reason = match recorded.get(&job.key) {
            None => Adoption::FirstRegistration,
            Some(recorded_hash) if *recorded_hash != config_hash => Adoption::ConfigChanged,
            // Already reconciled for exactly this definition. This is the
            // branch that makes a restart storm impossible.
            Some(_) => continue,
        };

        let Some(trigger) = triggers.get(&job.key) else {
            // A job with no trigger is not schedulable at all (the loader
            // builds one per job, so this means the two views disagreed);
            // firing it would be firing something the scheduler does not know.
            tracing::warn!(
                job_key = %job.key,
                "run_on_register: no trigger for this job — skipping the adoption fire"
            );
            continue;
        };

        if trigger.state == TriggerState::Paused {
            tracing::info!(
                job_key = %job.key,
                "run_on_register: job is paused — adoption fire deferred until it is resumed"
            );
            continue;
        }

        match due_at(trigger, now) {
            Some(due) => {
                if due > now {
                    tracing::info!(
                        job_key = %job.key,
                        due_at = %due,
                        gate = %trigger
                            .gate_closed_reason(now)
                            .unwrap_or_else(|| "not_before".into()),
                        reason = reason.as_str(),
                        "run_on_register: adoption fire deferred to the next permitted instant"
                    );
                }
                planned.push(PendingRegisterFire {
                    job_key: job.key.clone(),
                    config_hash,
                    due_at: due,
                    reason,
                });
            }
            None => {
                // Nothing recorded, so this re-evaluates on the next load: an
                // extended `not_after` or a fixed calendar makes it fire.
                tracing::warn!(
                    job_key = %job.key,
                    reason = reason.as_str(),
                    "run_on_register: no permitted instant for the adoption fire \
                     (past `not_after`, or a calendar/window that never opens) — not fired"
                );
            }
        }
    }

    planned.sort_by(|a, b| a.job_key.cmp(&b.job_key));
    planned
}

/// The earliest instant `trigger` may fire at or after `now`, honouring
/// `not_before`, the calendar/window gate, and `not_after`.
///
/// `None` = no such instant: the job is past `not_after`, or its gates never
/// open inside the scan horizon.
fn due_at(trigger: &Trigger, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let from = match trigger.not_before {
        Some(nb) if now < nb => nb,
        _ => now,
    };

    // Returns `from` unchanged when the job has neither calendar nor window.
    let open = trigger.next_gate_open(from)?;

    match trigger.not_after {
        Some(na) if open > na => None,
        _ => Some(open),
    }
}

/// Recorded rows whose job is present in the configuration but no longer
/// declares `run_on_register`.
///
/// The row records a contract that no longer exists, so it is dropped: if the
/// directive is added back later, that is a fresh adoption and has to fire.
/// Keeping the row would silently swallow the first fire after a re-add
/// whenever nothing else about the job changed.
///
/// Rows for jobs *absent* from the configuration are deliberately left alone,
/// for the same reason `loader::restore_trigger_states` keeps orphan
/// `job_states`: this pass cannot tell "deleted" from "temporarily commented
/// out", and a job that comes back a week later unchanged has not changed.
pub fn stale_records(jobs: &[JobConfig], recorded: &HashMap<String, String>) -> Vec<String> {
    let still_declared: HashSet<&str> = jobs
        .iter()
        .filter(|j| j.run_on_register)
        .map(|j| j.key.as_str())
        .collect();
    let defined: HashSet<&str> = jobs.iter().map(|j| j.key.as_str()).collect();

    let mut stale: Vec<String> = recorded
        .keys()
        .filter(|key| defined.contains(key.as_str()) && !still_declared.contains(key.as_str()))
        .cloned()
        .collect();
    stale.sort();
    stale
}

/// Fold the store's rows into the `job_key → hash` map [`plan`] and
/// [`stale_records`] take.
pub fn recorded_hashes(rows: Vec<JobRegisterFire>) -> HashMap<String, String> {
    rows.into_iter()
        .map(|row| (row.job_key, row.config_hash))
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use croniq_config::{compile::compile, parser::Parser};
    use croniq_scheduler::{
        calendar::Calendar, misfire::MisfirePolicy, schedule::Schedule, trigger::TimeWindow,
    };
    use pretty_assertions::assert_eq;

    use super::*;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        // A Wednesday, so weekday calendars are open unless told otherwise.
        Utc.with_ymd_and_hms(2026, 9, 2, h, m, 0).unwrap()
    }

    fn jobs_from(src: &str) -> Vec<JobConfig> {
        compile(&Parser::parse(src).expect("parses")).jobs
    }

    /// An armed, ungated trigger for every job in `jobs`.
    fn triggers_for(jobs: &[JobConfig], now: DateTime<Utc>) -> HashMap<String, Trigger> {
        jobs.iter()
            .map(|j| {
                (
                    j.key.clone(),
                    Trigger::new(
                        j.key.clone(),
                        Schedule::Interval { seconds: 900 },
                        chrono_tz::UTC,
                        None,
                        None,
                        MisfirePolicy::FireNow,
                        now,
                    ),
                )
            })
            .collect()
    }

    const DECLARED: &str = r#"
        job integration:credential-sync {
          every day at 04:20
          run_on_register
        }
    "#;

    #[test]
    fn first_registration_fires_immediately() {
        let jobs = jobs_from(DECLARED);
        let triggers = triggers_for(&jobs, at(10, 0));

        let planned = plan(&jobs, &triggers, &HashMap::new(), at(10, 0));
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].job_key, "integration:credential-sync");
        assert_eq!(planned[0].reason, Adoption::FirstRegistration);
        assert_eq!(planned[0].due_at, at(10, 0));
        assert_eq!(planned[0].config_hash, jobs[0].config_hash());
    }

    #[test]
    fn a_job_without_the_directive_never_fires() {
        let jobs = jobs_from(r#"job etl:sync { every 15 minutes }"#);
        let triggers = triggers_for(&jobs, at(10, 0));
        assert!(plan(&jobs, &triggers, &HashMap::new(), at(10, 0)).is_empty());
    }

    #[test]
    fn an_unchanged_definition_does_not_fire_again() {
        // The restart case: this is what separates the directive from "fire on
        // every boot", which would storm every such job at once.
        let jobs = jobs_from(DECLARED);
        let triggers = triggers_for(&jobs, at(10, 0));
        let recorded = HashMap::from([(jobs[0].key.clone(), jobs[0].config_hash())]);

        assert!(plan(&jobs, &triggers, &recorded, at(10, 0)).is_empty());
    }

    #[test]
    fn a_changed_definition_fires_again() {
        let jobs = jobs_from(DECLARED);
        let triggers = triggers_for(&jobs, at(10, 0));
        let recorded = HashMap::from([(jobs[0].key.clone(), "hash-of-the-old-definition".into())]);

        let planned = plan(&jobs, &triggers, &recorded, at(10, 0));
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].reason, Adoption::ConfigChanged);
        assert_eq!(
            planned[0].config_hash,
            jobs[0].config_hash(),
            "the hash to record is the one that fires, captured at plan time"
        );
    }

    #[test]
    fn a_paused_trigger_does_not_fire() {
        // Fail-closed jobs (#361: unresolved calendar reference) load paused.
        // Nothing is recorded for them, so the reload that fixes the fault
        // plans the fire then.
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(10, 0));
        triggers.get_mut(&jobs[0].key).unwrap().pause();

        assert!(plan(&jobs, &triggers, &HashMap::new(), at(10, 0)).is_empty());
    }

    #[test]
    fn an_exhausted_trigger_still_fires() {
        // `once at <past>` and a `disabled` schedule leave the trigger
        // exhausted. That is a statement about the *schedule*, not about the
        // job being switched off — the operator asked for an adoption fire, so
        // it happens.
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(10, 0));
        let t = triggers.get_mut(&jobs[0].key).unwrap();
        t.state = TriggerState::Exhausted;
        t.next_fire_at = None;

        assert_eq!(plan(&jobs, &triggers, &HashMap::new(), at(10, 0)).len(), 1);
    }

    #[test]
    fn a_missing_trigger_does_not_fire() {
        let jobs = jobs_from(DECLARED);
        assert!(plan(&jobs, &HashMap::new(), &HashMap::new(), at(10, 0)).is_empty());
    }

    // ── Gates defer rather than suppress ─────────────────────────────────────

    #[test]
    fn a_closed_window_defers_to_its_next_opening() {
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(6, 0));
        triggers.get_mut(&jobs[0].key).unwrap().window = TimeWindow::parse("08:00..18:00");

        let planned = plan(&jobs, &triggers, &HashMap::new(), at(6, 0));
        assert_eq!(planned.len(), 1, "deferred, not dropped");
        assert_eq!(planned[0].due_at, at(8, 0));
    }

    #[test]
    fn an_open_window_fires_now() {
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(10, 0));
        triggers.get_mut(&jobs[0].key).unwrap().window = TimeWindow::parse("08:00..18:00");

        let planned = plan(&jobs, &triggers, &HashMap::new(), at(10, 0));
        assert_eq!(planned[0].due_at, at(10, 0));
    }

    #[test]
    fn not_before_defers_to_the_boundary() {
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(10, 0));
        triggers.get_mut(&jobs[0].key).unwrap().not_before = Some(at(12, 30));

        let planned = plan(&jobs, &triggers, &HashMap::new(), at(10, 0));
        assert_eq!(planned[0].due_at, at(12, 30));
    }

    #[test]
    fn not_before_and_a_window_compose() {
        // The boundary lands outside the window, so the fire waits for the
        // window's opening *after* the boundary, not before it.
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(6, 0));
        let t = triggers.get_mut(&jobs[0].key).unwrap();
        t.not_before = Some(at(19, 0));
        t.window = TimeWindow::parse("08:00..18:00");

        let planned = plan(&jobs, &triggers, &HashMap::new(), at(6, 0));
        assert_eq!(planned[0].due_at, at(8, 0) + Duration::days(1));
    }

    #[test]
    fn past_not_after_suppresses_the_fire() {
        // The one bound with no later instant to defer to.
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(10, 0));
        triggers.get_mut(&jobs[0].key).unwrap().not_after = Some(at(9, 0));

        assert!(plan(&jobs, &triggers, &HashMap::new(), at(10, 0)).is_empty());
    }

    #[test]
    fn a_deferral_past_not_after_suppresses_the_fire() {
        let jobs = jobs_from(DECLARED);
        let mut triggers = triggers_for(&jobs, at(6, 0));
        let t = triggers.get_mut(&jobs[0].key).unwrap();
        t.window = TimeWindow::parse("08:00..18:00");
        t.not_after = Some(at(7, 0));

        assert!(plan(&jobs, &triggers, &HashMap::new(), at(6, 0)).is_empty());
    }

    #[test]
    fn a_closed_calendar_defers_to_its_next_opening() {
        let src = r#"
            calendar weekdays {
              timezone UTC
              include weekly monday tuesday wednesday thursday friday
            }
            job integration:credential-sync {
              every day at 04:20 { calendar weekdays }
              run_on_register
            }
        "#;
        let runtime = compile(&Parser::parse(src).expect("parses"));
        let jobs = runtime.jobs.clone();
        // A Saturday: the calendar is closed until Monday 00:00.
        let saturday = Utc.with_ymd_and_hms(2026, 9, 5, 10, 0, 0).unwrap();
        let mut triggers = triggers_for(&jobs, saturday);
        triggers.get_mut(&jobs[0].key).unwrap().calendar =
            Some(Calendar::from_config(&runtime.calendars[0]).expect("calendar compiles"));

        let planned = plan(&jobs, &triggers, &HashMap::new(), saturday);
        assert_eq!(planned.len(), 1);
        assert_eq!(
            planned[0].due_at,
            Utc.with_ymd_and_hms(2026, 9, 7, 0, 0, 0).unwrap(),
            "Monday 00:00, not the schedule's own 04:20 — the adoption fire is \
             not a scheduled tick, it runs as soon as the gate permits"
        );
    }

    // ── Ordering ─────────────────────────────────────────────────────────────

    #[test]
    fn plan_is_ordered_by_job_key() {
        let jobs = jobs_from(
            r#"
            job zeta:sync { every 15 minutes; run_on_register }
            job alpha:sync { every 15 minutes; run_on_register }
            "#,
        );
        let triggers = triggers_for(&jobs, at(10, 0));

        let planned = plan(&jobs, &triggers, &HashMap::new(), at(10, 0));
        let keys: Vec<&str> = planned.iter().map(|p| p.job_key.as_str()).collect();
        assert_eq!(keys, vec!["alpha:sync", "zeta:sync"]);
    }

    // ── stale_records ────────────────────────────────────────────────────────

    #[test]
    fn dropping_the_directive_makes_the_record_stale() {
        // …so that re-adding it later fires again, instead of being swallowed
        // because nothing else about the job changed.
        let jobs = jobs_from(r#"job etl:sync { every 15 minutes }"#);
        let recorded = HashMap::from([("etl:sync".to_string(), "hash-a".to_string())]);

        assert_eq!(stale_records(&jobs, &recorded), vec!["etl:sync"]);
    }

    #[test]
    fn a_job_that_still_declares_the_directive_keeps_its_record() {
        let jobs = jobs_from(DECLARED);
        let recorded = HashMap::from([(jobs[0].key.clone(), "hash-a".to_string())]);

        assert!(stale_records(&jobs, &recorded).is_empty());
    }

    #[test]
    fn a_job_absent_from_the_config_keeps_its_record() {
        // This pass cannot tell "deleted" from "commented out for a week", and
        // a job that comes back unchanged has not changed. Same reason
        // `restore_trigger_states` keeps orphan `job_states` rows.
        let jobs = jobs_from(DECLARED);
        let recorded = HashMap::from([("long:gone".to_string(), "hash-a".to_string())]);

        assert!(stale_records(&jobs, &recorded).is_empty());
    }

    #[test]
    fn recorded_hashes_folds_store_rows_by_job_key() {
        let rows = vec![
            JobRegisterFire {
                job_key: "a:job".into(),
                config_hash: "hash-a".into(),
                fired_at: at(9, 0),
            },
            JobRegisterFire {
                job_key: "b:job".into(),
                config_hash: "hash-b".into(),
                fired_at: at(9, 0),
            },
        ];
        let map = recorded_hashes(rows);
        assert_eq!(map.get("a:job").map(String::as_str), Some("hash-a"));
        assert_eq!(map.get("b:job").map(String::as_str), Some("hash-b"));
    }
}
