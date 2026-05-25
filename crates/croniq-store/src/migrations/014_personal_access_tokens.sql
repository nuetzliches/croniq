-- Personal Access Tokens (PATs) — user-bound tokens for CLI / API use.
--
-- Distinct from api_keys (which belong to api_clients and represent a
-- service identity). A PAT has a stable user_id, a human-readable name
-- ("laptop", "ci-personal"), a scope subset of the owning user's role,
-- and an optional expiry. The raw token is shown once at creation; only
-- the SHA-256 hash is persisted. Prefix is kept for display (the UI
-- never has the raw token, only the prefix to identify "which one").
--
-- last_used_at is updated by the auth middleware on every successful
-- request — best-effort, not transactional, so the Settings UI can
-- show "in use" / "abandoned" without contention.

CREATE TABLE personal_access_tokens (
    token_id      TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    token_prefix  TEXT NOT NULL,
    scopes        TEXT NOT NULL DEFAULT '[]',
    expires_at    TEXT,
    revoked_at    TEXT,
    last_used_at  TEXT,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_pat_hash ON personal_access_tokens(token_hash);
CREATE INDEX idx_pat_user ON personal_access_tokens(user_id);
