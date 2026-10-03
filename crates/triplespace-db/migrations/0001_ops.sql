-- The `ops` schema (ADR 0013 §4, §7): operational state that is neither the log nor a
-- projection of it. `ops.migration` itself is created by the runner before anything runs.

CREATE SCHEMA IF NOT EXISTS ops;

-- How far each projection has replayed each partition (0013 §7). Lag is the distance to
-- the partition's head.
CREATE TABLE IF NOT EXISTS ops.projection_state (
  projection     text   NOT NULL,
  partition      bigint NOT NULL,
  applied_offset bigint NOT NULL,              -- offsets below this are applied; 0 is nothing
  updated_at     timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (projection, partition)
);

-- Queued fan-out the synchronous budget did not cover (0013 §7): work for a key, applied
-- in append order by the projection worker.
CREATE TABLE IF NOT EXISTS ops.projection_work (
  id         bigserial PRIMARY KEY,
  projection text   NOT NULL,
  tenant     text   NOT NULL DEFAULT '',
  partition  bigint NOT NULL,
  "offset"   bigint NOT NULL,                  -- the record whose fan-out this is
  key        text   NOT NULL,                  -- the entity or page to re-project
  queued_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS projection_work_order ON ops.projection_work (projection, partition, "offset", id);
