-- Binds a `runner_id` in the pull-based work protocol to the authenticated
-- credential that first claimed it (first-writer-wins). Without this table
-- the work handlers trust the `runner_id` in the request body, so any holder
-- of a `work:*` scope can act under another runner's identity.
--
-- `owner_id` is the caller's `client_id` (API keys) or user id (JWT/PAT), so
-- deployments that share one runner key across many runners keep working:
-- every runner resolves to the same owner. Rows are dropped when an operator
-- deregisters the runner via `DELETE /v1/runners/{id}`, which is how a
-- runner_id is handed to a different credential.

CREATE TABLE IF NOT EXISTS runner_identities (
    runner_id TEXT PRIMARY KEY,
    owner_id  TEXT NOT NULL,
    bound_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runner_identities_owner
    ON runner_identities(owner_id);
