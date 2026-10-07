-- Jobs an operator has starred, per user: the dashboard sorts them first and
-- the Runs screen can narrow to them.
--
--   user_id     the user who starred the job; the row goes with the user
--   job_key     the starred job
--   created_at  RFC3339
--
-- Keyed by job_key, not by the job's UUID: adopting a Croniqfile job mints a
-- new UUID, and a star should survive that. No foreign key on job_key either,
-- for the same reason `job_notes` has none — a job that is deleted and
-- declared again keeps its star. A favorite is the operator's view state, not
-- part of the job's definition, so the Croniqfile does not own it (ADR-0006).

CREATE TABLE IF NOT EXISTS user_favorite_jobs (
    user_id    TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    job_key    TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, job_key)
);
