-- A follower's gap (ADR 0092 §3, A13, A15; 03 §4.12): the position it refused to start
-- from and the oldest recent change the wiki still had, with when it found them, for
-- `GET /provider/{code}/mirror` to report as `follow-gap`. Cleared by the next position
-- saved and by a load that records a new end. Idempotent.

ALTER TABLE ops.repo_cursor ADD COLUMN IF NOT EXISTS gap jsonb;
