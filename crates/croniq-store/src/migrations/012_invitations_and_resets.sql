-- Invitations and password-reset tokens.
--
-- Both follow the same pattern as api_keys: the raw token is generated
-- once, only its SHA-256 hash is stored. On consumption the caller
-- presents the raw token, the server hashes it, looks up the row, and
-- validates expiry + single-use.
--
-- The `email` column on invitations is used both for SMTP delivery
-- (PR-A6, when configured) and for the admin-side audit log. Email is
-- optional only because users can also be created directly by the
-- admin endpoint without going through invitation.

CREATE TABLE invitations (
    invitation_id TEXT PRIMARY KEY,
    email         TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('admin','operator','viewer')),
    token_hash    TEXT NOT NULL UNIQUE,
    invited_by    TEXT NOT NULL REFERENCES users(user_id),
    expires_at    TEXT NOT NULL,
    accepted_at   TEXT,
    revoked_at    TEXT,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_invitations_token ON invitations(token_hash);
CREATE INDEX idx_invitations_email ON invitations(email);
CREATE INDEX idx_invitations_invited_by ON invitations(invited_by);

CREATE TABLE password_resets (
    reset_id    TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL UNIQUE,
    expires_at  TEXT NOT NULL,
    used_at     TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_password_resets_token ON password_resets(token_hash);
CREATE INDEX idx_password_resets_user ON password_resets(user_id);
