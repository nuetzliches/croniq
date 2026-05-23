-- Users with role-based access control.
--
-- Before this migration, password_credentials was the single source of
-- truth for "is this username allowed in?" and every authenticated user
-- was implicitly granted the admin scope by the login handler. That
-- worked for single-admin deploys but blocks Multi-User, OIDC, PATs,
-- and TOTP — all of which need a stable user_id that survives
-- credential changes.
--
-- The split is:
--   users               — identity (user_id, username, email, role)
--   password_credentials — one auth method bound to a user
--   personal_access_tokens / oidc_identities — added later, also bound to user_id
--
-- Roles map to scope sets in croniq-auth::context::Role::default_scopes:
--   admin    — wildcard (same as today)
--   operator — read everything + write jobs/schedules/calendars + trigger
--   viewer   — read-only across the board
--
-- Backfill: every existing password_credentials row becomes a users row
-- with role=admin (preserves current behaviour). The user_id is
-- preserved, so existing refresh_tokens.user_id references stay valid.

CREATE TABLE users (
    user_id       TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT,
    display_name  TEXT,
    role          TEXT NOT NULL CHECK (role IN ('admin','operator','viewer')),
    is_active     INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    last_login_at TEXT
);

CREATE INDEX idx_users_username ON users(username);
CREATE INDEX idx_users_email ON users(email);

-- Backfill from existing password_credentials. Idempotent via
-- INSERT OR IGNORE so re-running the migration on a partially-migrated
-- DB (e.g. after a failed apply) is safe.
INSERT OR IGNORE INTO users (user_id, username, role, is_active, created_at, updated_at)
SELECT user_id, username, 'admin', 1, created_at, created_at
FROM password_credentials;
