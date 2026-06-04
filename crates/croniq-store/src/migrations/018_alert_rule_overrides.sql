-- Operational overrides for DSL-managed alert rules (issue #231, Phase 1).
--
-- A second persistence layer next to Adopt: carries *runtime state*, not
-- *definition*. The Croniqfile stays the canonical source of truth for what
-- a rule is; this table lets an operator temporarily snooze, disable, or
-- re-throttle a rule during an incident without a Croniqfile commit or a
-- permanent Adopt.
--
-- One row per rule, keyed by the DSL rule name (FK-by-name — DSL rules live
-- in the Croniqfile, not a table, so the loader prunes orphan rows at boot
-- when a rule is removed). Columns:
--   enabled        NULL = use DSL default, 0 = force-disabled, 1 = force-enabled
--   snooze_until   RFC3339 — rule is suppressed until this instant
--   throttle_secs  replaces the DSL throttle window when set
--   note           mandatory incident context, captured at write time
--   expires_at     optional auto-clear deadline; the watchdog sweep deletes
--                  the row once now >= expires_at, so a "snooze 4h" evaporates
--                  without operator follow-up
--
-- Evaluation merges DSL state with this row at every decision point; an
-- expired row (expires_at <= now) is inert until the sweep removes it.

CREATE TABLE IF NOT EXISTS alert_rule_overrides (
    rule_name       TEXT PRIMARY KEY,
    enabled         INTEGER,
    snooze_until    TEXT,
    throttle_secs   INTEGER,
    note            TEXT NOT NULL,
    set_by_user_id  TEXT NOT NULL,
    set_at          TEXT NOT NULL,
    expires_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_alert_rule_overrides_expires_at
    ON alert_rule_overrides(expires_at);
