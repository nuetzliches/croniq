-- How far each user has read the notes inbox: one instant per user. Notes by
-- someone else created after it are unread; opening the inbox moves it forward.
--
--   user_id  the reader; the row goes with the user
--   seen_at  RFC3339, the newest note the inbox showed them
--
-- No row means the inbox was never opened, and every note by someone else is
-- unread. Kept on the server, like the favorites of 032, so having read the
-- notes on one browser counts on every other.
--
-- Its own table rather than a column on `users`: it is written on every visit
-- to the inbox, and the users row is the one the auth middleware reads on
-- every request — view state should not churn it, nor its `updated_at`.
--
-- The index is for the unread count behind the navigation badge, polled by
-- every open dashboard: `created_at > seen_at` is the selective half of it.
-- It carries the other three columns that count reads so it answers it alone.
-- On `created_at` by itself the planner preferred scanning the whole of
-- idx_job_notes_job_created — it orders the GROUP BY's first column — and
-- fetching every row from the table.

CREATE TABLE IF NOT EXISTS user_notes_seen (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    seen_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_job_notes_created_at
    ON job_notes(created_at, author_id, job_key, execution_id);
