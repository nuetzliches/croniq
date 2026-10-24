-- Calendar names are unique (review 2026-10-24, P-12).
--
-- Triggers reference a calendar by name, but neither the schema nor the API
-- stopped two API calendars from sharing one, and `resolve_calendars` then let
-- whichever row it saw last win. The API now answers 409 on a name that is
-- taken; this index is what makes that answer race-free.
--
-- A database that already holds duplicates must still boot, so before the
-- index is built every duplicate except the oldest row (ties broken by
-- `calendar_id`) is renamed to `<name> (duplicate <calendar_id>)`. The oldest
-- row keeps the name and with it every trigger that references it; the
-- renamed rows stay visible in the API and can be inspected or deleted.
UPDATE calendar_definitions
SET name = name || ' (duplicate ' || calendar_id || ')'
WHERE EXISTS (
    SELECT 1 FROM calendar_definitions AS older
    WHERE older.name = calendar_definitions.name
      AND (older.created_at < calendar_definitions.created_at
           OR (older.created_at = calendar_definitions.created_at
               AND older.calendar_id < calendar_definitions.calendar_id))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_calendar_definitions_name
    ON calendar_definitions(name);
