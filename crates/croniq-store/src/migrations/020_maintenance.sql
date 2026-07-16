-- Global maintenance switch (single-row table).
--
-- When active, the scheduler stops emitting new work and the work-poll hands
-- out nothing — dispatch is frozen. In-flight executions still finish; queued
-- work and triggers accepted during the window resume once it clears. State is
-- either a manual toggle or a scheduled [window_start, window_end) window.
--
-- Singleton: id is pinned to 1 via CHECK so at most one row ever exists.
--   manual_active  1 = paused now until turned off; 0 = defer to the window
--   window_start   RFC3339 — optional lower bound (NULL = starts immediately)
--   window_end     RFC3339 — optional upper bound (NULL = open-ended)
--   note           optional operator message shown in the UI banner
--   updated_by     user_id / api_client_id that last changed the switch
--   updated_at     RFC3339 of the last change

CREATE TABLE IF NOT EXISTS maintenance (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    manual_active INTEGER NOT NULL DEFAULT 0,
    window_start  TEXT,
    window_end    TEXT,
    note          TEXT,
    updated_by    TEXT,
    updated_at    TEXT
);
