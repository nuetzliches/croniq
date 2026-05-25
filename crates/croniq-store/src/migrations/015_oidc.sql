-- OIDC/SSO support — JIT-provisioned user identities + pending-login
-- state-param table for the authorization-code exchange.
--
-- Each external OIDC subject is bound to one Croniq user via the
-- (provider, subject) composite key. On first sign-in the user is
-- JIT-created with role=viewer (admin must promote manually). On
-- subsequent sign-ins the existing user row is used — preserving
-- role + last_login_at history.
--
-- oidc_pending_logins is a short-TTL store for the random `state`
-- param that the IdP echoes back to /oidc/callback. Without this we
-- can't distinguish a legitimate callback from a CSRF attempt.
-- Rows older than `expires_at` (default 10 minutes) are purged
-- opportunistically on the next lookup.

CREATE TABLE oidc_identities (
    provider      TEXT NOT NULL,
    subject       TEXT NOT NULL,
    user_id       TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    email         TEXT,
    linked_at     TEXT NOT NULL,
    last_login_at TEXT,
    PRIMARY KEY (provider, subject)
);
CREATE INDEX idx_oidc_identities_user ON oidc_identities(user_id);

CREATE TABLE oidc_pending_logins (
    state         TEXT PRIMARY KEY,
    nonce         TEXT NOT NULL,
    redirect_to   TEXT,                    -- optional post-login UI path
    created_at    TEXT NOT NULL,
    expires_at    TEXT NOT NULL
);
CREATE INDEX idx_oidc_pending_expires ON oidc_pending_logins(expires_at);
