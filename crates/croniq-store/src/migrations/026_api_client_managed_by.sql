-- Add managed_by to api_clients so a client declared in the environment
-- (CRONIQ_API_CLIENT_<NAME>_KEY, issue #471) is distinguishable from one an
-- operator created through the API or the dashboard.
--
-- Ownership decides who wins on conflict. For managed_by='env' the
-- environment is the source of truth: the reconciler syncs name, scopes and
-- key on every explicit reload, and the API refuses edits to the row. Without
-- the marker a dashboard scope change would be silently reverted at the next
-- reconcile, which is drift no operator could explain from either side.
--
-- Mirrors the trigger_definitions (003) and calendar_definitions (006)
-- pattern. Existing rows predate env declaration and stay 'api'.

ALTER TABLE api_clients
    ADD COLUMN managed_by TEXT NOT NULL DEFAULT 'api';
