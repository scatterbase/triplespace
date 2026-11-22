-- A follower's consecutive failed passes (ADR 0104 A12): a `follow` row whose pass fails
-- is put back to wait twice as long as the last, up to an hour, and fails as other jobs
-- do once `follow.max_failures` passes in a row have failed. A pass that succeeds sets it
-- back to 0.
ALTER TABLE ops.job_queue ADD COLUMN IF NOT EXISTS failures integer NOT NULL DEFAULT 0;
