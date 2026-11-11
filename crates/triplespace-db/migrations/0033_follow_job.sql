-- A follower's open job, the day's (ADR 0092 A6; 03 §4.12): its ID, its UTC day and its
-- counts so far, kept with the position so that the next batch, or the next run, writes
-- under it and finishes it when the day ends. Idempotent.

ALTER TABLE ops.repo_cursor ADD COLUMN IF NOT EXISTS job jsonb;
