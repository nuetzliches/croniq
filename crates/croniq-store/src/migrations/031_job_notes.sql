-- Operator notes on jobs and their runs: "seen and checked", questions, ideas.
--
--   job_key       the job the note is about; always set
--   execution_id  the run it was written about, or NULL for a note on the job
--   kind          ack | question | idea | note
--   body          plain text; empty only for an ack
--   author_id     caller user_id / api_client_id
--   author_name   display name at write time, so the note still reads right
--                 after the user is renamed or deleted
--   created_at    RFC3339
--
-- No foreign key on purpose, on either column. Retention deletes runs (see the
-- prune queries in sqlite.rs / pg.rs), and a note is meant to outlive the run
-- it describes: what an operator learned about a failure is still worth
-- reading after its log is gone. A FK would make every prune fail, or force a
-- child delete into each prune query that throws that knowledge away. The UI
-- shows a note whose run is gone as "run deleted". The same reasoning keeps
-- the notes of a deleted job as history.

CREATE TABLE IF NOT EXISTS job_notes (
    id           TEXT PRIMARY KEY,
    job_key      TEXT NOT NULL,
    execution_id TEXT,
    kind         TEXT NOT NULL CHECK (kind IN ('ack', 'question', 'idea', 'note')),
    body         TEXT NOT NULL DEFAULT '',
    author_id    TEXT NOT NULL,
    author_name  TEXT NOT NULL,
    created_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_job_notes_job_created
    ON job_notes(job_key, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_job_notes_execution
    ON job_notes(execution_id);
