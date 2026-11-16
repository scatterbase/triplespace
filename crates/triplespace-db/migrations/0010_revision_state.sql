-- The tier-1 state of ADR 0083 §4: the current state of every delta graph per key, and
-- the local-graph state after every local revision, both written in the appending
-- transaction beside the record (0083 A1 retires the fold that stood in for them).
-- Idempotent: every statement tolerates having run before.

CREATE TABLE IF NOT EXISTS view.graph_state (     -- 0083 §4: the "old state" composition and the diff read
  tenant    text   NOT NULL DEFAULT '',
  entity_id text   NOT NULL,
  graph     text   NOT NULL,                      -- local, derived/*, source/*
  state     bytea  NOT NULL,                      -- the stored form of the graph's state (LocalState::to_storage_json)
  "offset"  bigint NOT NULL,                      -- the graph's latest record for the key
  version   bigint NOT NULL,                      -- bumped on every write
  PRIMARY KEY (tenant, entity_id, graph)
);

CREATE TABLE IF NOT EXISTS view.entity_revision ( -- 0083 §4: the local-graph state after every local revision
  tenant    text   NOT NULL DEFAULT '',
  revid     bigint NOT NULL,
  entity_id text   NOT NULL,
  partition bigint NOT NULL,
  "offset"  bigint NOT NULL,
  state     bytea  NOT NULL,
  PRIMARY KEY (tenant, revid)
);
CREATE UNIQUE INDEX IF NOT EXISTS entity_revision_record ON view.entity_revision (partition, "offset");
CREATE INDEX IF NOT EXISTS entity_revision_entity ON view.entity_revision (tenant, entity_id, "offset");
