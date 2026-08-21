//! Which jobs the running configuration actually defines (issue #470).
//!
//! `job_states` rows outlive the jobs that created them — nothing deletes one,
//! deliberately, because the loader cannot tell "removed" from "commented out
//! for a week". Every consumer that reads those rows therefore has to decide
//! what to do about a row whose job the scheduler no longer knows, and the
//! answer is the same for all of them: do not report it.
//!
//! It was not the same, though, because each consumer expressed the rule
//! itself. The metrics exporter got it right (#470) and the states API, the
//! watchdog's missed-fire sweep and the MCP listing kept emitting phantoms
//! (#506). The rule lives here now, in one type, mostly so the *fail-open*
//! half of it cannot be got backwards: "cannot tell" has to mean "report
//! everything", never "report nothing".

use std::collections::{HashMap, HashSet};

use crate::trigger::Trigger;

/// The jobs the scheduler currently knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveJobs {
    /// No trigger map to consult — a server wired up without one (some tests,
    /// any future embedding). Everything is reported: losing every per-job
    /// signal is a far worse failure than reporting a stale one, and it would
    /// be silent.
    Unknown,
    /// The keys the running configuration defines.
    Known(HashSet<String>),
}

impl LiveJobs {
    /// Read the live set out of a trigger snapshot, `None` meaning there is no
    /// snapshot to read.
    ///
    /// The caller takes its own read guard and passes the map, so this stays
    /// runtime-agnostic — `croniq-scheduler` deliberately does not depend on
    /// tokio.
    pub fn from_snapshot(triggers: Option<&HashMap<String, Trigger>>) -> Self {
        match triggers {
            Some(map) => Self::Known(map.keys().cloned().collect()),
            None => Self::Unknown,
        }
    }

    /// Whether `job_key` may be reported.
    ///
    /// [`LiveJobs::Unknown`] answers `true` for everything — see the variant's
    /// note on why that direction is the safe one.
    pub fn includes(&self, job_key: &str) -> bool {
        match self {
            Self::Unknown => true,
            Self::Known(keys) => keys.contains(job_key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Schedule;

    fn snapshot(keys: &[&str]) -> HashMap<String, Trigger> {
        keys.iter()
            .map(|k| {
                (
                    (*k).to_string(),
                    Trigger::new(
                        (*k).to_string(),
                        Schedule::Interval { seconds: 60 },
                        chrono_tz::UTC,
                        None,
                        None,
                        crate::misfire::MisfirePolicy::default(),
                        chrono::Utc::now(),
                    ),
                )
            })
            .collect()
    }

    #[test]
    fn a_known_set_reports_only_its_own_keys() {
        let live = LiveJobs::from_snapshot(Some(&snapshot(&["a:job"])));
        assert!(live.includes("a:job"));
        assert!(
            !live.includes("gone:job"),
            "a job the configuration no longer defines must not be reported"
        );
    }

    #[test]
    fn no_snapshot_reports_everything() {
        // The fail-open direction, and the reason this is a type rather than a
        // predicate repeated at four call sites: getting it backwards would
        // silently drop every per-job signal on a server with no trigger map.
        let live = LiveJobs::from_snapshot(None);
        assert!(live.includes("anything:at:all"));
        assert_eq!(live, LiveJobs::Unknown);
    }

    #[test]
    fn an_empty_snapshot_is_not_the_same_as_no_snapshot() {
        // A configuration with no jobs is a real answer: report nothing.
        let live = LiveJobs::from_snapshot(Some(&HashMap::new()));
        assert!(!live.includes("a:job"));
    }
}
