-- Audit log — append-only record of who-did-what-to-which-resource.
--
-- Powers the Activity Feed on the Dashboard, the per-job Audit tab,
-- and Settings → Audit. Writes are append-only at the application
-- layer; no DELETE except via explicit retention purge (not part of
-- PR-B1, tracked separately).
--
-- actor_type / actor_id let consumers tell apart:
--   user      — human; actor_id is users.user_id
--   api_key   — service; actor_id is api_keys.key_id
--   pat       — personal access token; actor_id is users.user_id
--   oidc      — fresh OIDC sign-in; actor_id is users.user_id
--   system    — internal (DSL sync, scheduler, watchdog); actor_id is None
--
-- target_type / target_id name the touched resource:
--   job, runner, execution, dead_letter, calendar, schedule,
--   user, invitation, api_client, api_key, pat, totp, oidc, auth.

CREATE TABLE audit_log (
    event_id    TEXT PRIMARY KEY,
    actor_type  TEXT NOT NULL,
    actor_id    TEXT,
    action      TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id   TEXT,
    -- Optional JSON diff / context blob. Use sparingly — small payloads
    -- only (rename diffs, schedule changes). Bulk diffs go to dedicated
    -- stores.
    diff_json   TEXT,
    ip_address  TEXT,
    user_agent  TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_audit_log_created ON audit_log(created_at DESC);
CREATE INDEX idx_audit_log_target ON audit_log(target_type, target_id);
CREATE INDEX idx_audit_log_actor ON audit_log(actor_type, actor_id);
