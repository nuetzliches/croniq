-- Free-form tags for jobs (filter-only, NOT routing-relevant).
--
-- Tags are stored as a JSON array of strings (`["env=prod", "team=ops"]`).
-- Filtering is done by exact-match against array elements; no key/value
-- semantics are enforced at the DB level — by convention `key=value` is
-- recommended but `release-candidate` is equally valid.
--
-- Distinct from `runner.capabilities`: tags do not influence routing.

ALTER TABLE job_definitions ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
