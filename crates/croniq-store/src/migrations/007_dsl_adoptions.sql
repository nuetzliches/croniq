-- Tracks DSL keys that have been "adopted" into the API store. A row here
-- means: even though the Croniqfile defines this resource, the API copy
-- takes precedence and the loader skips the DSL definition on next reload.
--
-- Adopt: copy DSL definition into the API store with managed_by='api',
--   then INSERT a row here.
-- Unadopt: DELETE this row + DELETE the API row → next reload reinstates
--   the DSL version.
--
-- See `crates/croniq-server/src/api/calendars.rs::handle_adopt` and the
-- loader's exclude logic in `reload.rs`.

CREATE TABLE dsl_adoptions (
    resource_type TEXT NOT NULL,    -- 'calendar' | 'job' | 'trigger'
    resource_key  TEXT NOT NULL,    -- DSL identifier (calendar/job name)
    adopted_at    TEXT NOT NULL,
    adopted_by    TEXT,             -- caller user_id / api_client_id (nullable)
    PRIMARY KEY (resource_type, resource_key)
);
