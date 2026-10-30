-- The worker's attempts (0013 §7; 0083 §3; 03 §6.1): a queued item is leased by a worker
-- and removed when the work that applied it commits, so a worker that dies mid-batch
-- loses nothing. When applying it fails, the attempt is recorded here; an item that has
-- failed too often is left out of every lease and reported as failed, so one poisoned key
-- stops neither its tenant's queue nor anyone else's.
ALTER TABLE ops.projection_work
  ADD COLUMN IF NOT EXISTS attempts   integer     NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS last_error text,
  ADD COLUMN IF NOT EXISTS failed_at  timestamptz;
