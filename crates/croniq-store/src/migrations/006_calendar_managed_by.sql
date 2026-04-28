-- Add managed_by column to calendar_definitions so the loader can shadow
-- DSL-defined calendars (managed_by='dsl') alongside API-managed ones,
-- mirroring the trigger_definitions pattern (003_definitions.sql:23).
--
-- Existing rows are pre-migration API creations and stay 'api'.

ALTER TABLE calendar_definitions
    ADD COLUMN managed_by TEXT NOT NULL DEFAULT 'api';
