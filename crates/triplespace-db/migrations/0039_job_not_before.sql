-- A queue row that is not to be claimed yet (ADR 0104 A9): a follower's row, put back
-- after a pass to be claimed again for the next one once `follow.interval` has passed.
-- NULL for every other row, which is claimed as soon as its partition is free.
-- Idempotent.

ALTER TABLE ops.job_queue ADD COLUMN IF NOT EXISTS not_before timestamptz;
