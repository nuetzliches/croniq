-- Auth tables for API clients, keys, credentials, and refresh tokens

CREATE TABLE api_clients (
    client_id   TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    scopes      TEXT NOT NULL DEFAULT '[]',
    is_active   INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL
);

CREATE TABLE api_keys (
    key_id      TEXT PRIMARY KEY,
    client_id   TEXT NOT NULL REFERENCES api_clients(client_id),
    key_hash    TEXT NOT NULL,
    key_prefix  TEXT NOT NULL,
    expires_at  TEXT,
    revoked_at  TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX idx_api_keys_client_id ON api_keys(client_id);

CREATE TABLE password_credentials (
    user_id         TEXT PRIMARY KEY,
    username        TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until    TEXT,
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_password_credentials_username ON password_credentials(username);

CREATE TABLE refresh_tokens (
    token_hash  TEXT PRIMARY KEY,
    client_id   TEXT NOT NULL,
    user_id     TEXT,
    expires_at  TEXT NOT NULL,
    revoked_at  TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX idx_refresh_tokens_client_id ON refresh_tokens(client_id);
