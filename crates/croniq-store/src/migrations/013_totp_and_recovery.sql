-- TOTP/2FA secrets + single-use recovery codes.
--
-- TOTP secret is wrapped at-rest with AES-256-GCM. The wrap key is
-- derived from CRONIQ_JWT_SECRET via HKDF-SHA256 with the static info
-- string "croniq-totp-v1" (see croniq_auth::crypto::derive_totp_key).
-- Reusing the JWT secret keeps the threat model simple — anyone with
-- read access to it can already mint admin tokens, so they can
-- separately unwrap TOTP secrets too. The wrap is mainly a defence
-- against DB-only exfiltration (a leaked SQLite file shouldn't
-- immediately leak working 2FA codes).
--
-- enabled = 0 during the setup window (between /totp/setup and
-- /totp/confirm). Login requires enabled = 1 to step up.
--
-- recovery_codes are 8-char lowercase alphanumeric strings, SHA-256
-- hashed at storage time. used_at is set on consumption and the row
-- never re-used (single-use).

CREATE TABLE totp_secrets (
    user_id      TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    secret_enc   TEXT NOT NULL,            -- base64(nonce || ciphertext+tag)
    enabled      INTEGER NOT NULL DEFAULT 0,
    confirmed_at TEXT,
    created_at   TEXT NOT NULL
);

CREATE TABLE recovery_codes (
    code_id    TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    code_hash  TEXT NOT NULL,              -- SHA-256 (hex) of the raw code
    used_at    TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_recovery_codes_user ON recovery_codes(user_id);
CREATE INDEX idx_recovery_codes_hash ON recovery_codes(code_hash);
