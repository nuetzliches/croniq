//! SQL fragments shared by the two store backends' retention paths.
//!
//! Both prune paths, in both backends, have to answer the same question about
//! a `dead` execution, and the answer is a correlated subquery rather than a
//! column — so it was copy-pasted eight times (four queries per backend, one
//! per delete of a parent and its logs). Any change to what "unreferenced"
//! means had to land in all eight, in two dialects, and the comment explaining
//! why the rule exists only ever made it into one of them.
//!
//! The text is dialect-identical: it names no placeholders and uses no
//! backend-specific syntax, so the same `&str` composes into both the `?N`
//! and the `$N` queries.

/// Restricts a retention sweep to the executions it is allowed to delete.
///
/// `dead` rows are included only when no `dead_letters` row references them.
/// Dead-letter retention governs the ones that have a letter, as documented —
/// but a dead execution that never produced one, or whose letter has already
/// been purged, had no governing retention at all and grew without bound
/// (issue #470).
///
/// Expects the `executions` table aliased as `e`. Paired with the index from
/// migration 027, without which the probe scans `dead_letters` once per
/// candidate row on every watchdog tick (issue #485).
pub(crate) const DELETABLE_EXECUTION: &str = "(e.state <> 'dead'
     OR NOT EXISTS (SELECT 1 FROM dead_letters dl WHERE dl.execution_id = e.id))";
