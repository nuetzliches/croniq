-- Last definition each `run_on_register` job was fired for (issue #555).
--
-- The directive fires a job once when it is adopted: the first time the key is
-- seen, and again whenever its compiled config hash changes. That "again on
-- change, never on a plain restart" rule is the whole reason this table
-- exists — without a persisted hash the only implementable semantics are
-- "fire on every boot", which storms every such job on a restart and on every
-- `--watch` save.
--
--   job_key      the job that fired
--   config_hash  JobConfig::config_hash() of the definition that fired
--                (hex SHA-256 over the compiled job, minus its cosmetic and
--                identity fields — see croniq-config/src/fingerprint.rs)
--   fired_at     RFC3339 of the dispatch
--
-- Written only after the fire is dispatched, so a crash in between leaves the
-- job un-reconciled and the next boot fires again — the safe direction for a
-- job whose point is to reconcile external state.
--
-- Separate from `job_states` on purpose: that row is per-tick scheduler state
-- rewritten on every fire, this one changes only when a definition does, and
-- must survive a job being temporarily absent from the Croniqfile.

CREATE TABLE IF NOT EXISTS job_register_fires (
    job_key     TEXT PRIMARY KEY,
    config_hash TEXT NOT NULL,
    fired_at    TEXT NOT NULL
);
