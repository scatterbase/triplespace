-- The `log` schema (ADR 0013 §2, as amended by 0015 §7 and 0006 A14).
-- Idempotent: every statement tolerates having run before.

CREATE SCHEMA IF NOT EXISTS log;

CREATE TABLE IF NOT EXISTS log.partition (
  partition     bigint PRIMARY KEY,            -- 64 random bits, fixed at creation (0018 §2); 0 is the instance config
  tenant        text,                          -- tenant slug, or NULL for an instance partition (0018 §2)
  name          text NOT NULL,                 -- "local", "mirror/wikidata", "pages", …
  graph_iri     text NOT NULL,                 -- {base}/graph/{name} (0015 §5)
  history       text NOT NULL,                 -- full | latest            (0002 §2)
  integrity     text NOT NULL,                 -- logged | hashed          (0006 §4)
  export        text NOT NULL,                 -- public | internal | private (0005 §4.1, 0007 §8)
  segment_k     smallint NOT NULL,             -- segments hold 2^k records (0006 §5)
  hash          text NOT NULL,                 -- named in the genesis record (0006 §2, 0015 §3)
  next_offset   bigint NOT NULL DEFAULT 0,
  UNIQUE NULLS NOT DISTINCT (tenant, name)     -- the registry key of 0018 §2
);

CREATE TABLE IF NOT EXISTS log.record (
  partition     bigint   NOT NULL REFERENCES log.partition,
  "offset"      bigint   NOT NULL,
  appended_at   bigint   NOT NULL,             -- µs since the epoch, UTC   (header 3)
  payload_type  text     NOT NULL,             --                          (header 4)
  key           text,                          -- identifier, never content (header 5)
  commitment    bytea    NOT NULL,             -- root over the part leaves, 32 bytes (header 6, 0015 §1)
  revid         bigint,                        -- global or provider-ranged revision ID (header 7, 0015 §2)
  logid         bigint,                        -- global log ID (header 8, 0015 §2)
  page_id       bigint,                        -- page ID of the key, carried forward (header 9, 0015 §2)
  header        bytea    NOT NULL,             -- the canonical CBOR header, the Merkle preimage
  leaf          bytea    NOT NULL,             -- H(0x00 ‖ header)
  body          bytea    NOT NULL,             -- canonical CBOR; an erased part is [null, leaf] (0015 §1)
  erased        smallint NOT NULL DEFAULT 0,   -- bitmask of erased parts (0015 §7)
  erased_by     bigint,                        -- offset of the latest erase record touching this row
  PRIMARY KEY (partition, "offset")
) PARTITION BY LIST (partition);

ALTER TABLE log.record ALTER COLUMN body SET COMPRESSION lz4;

-- A compacted offset keeps its leaf (0006 A14): the row is gone from log.record and the
-- leaf stays in log.merkle_node at level 0, so the tree over every offset still folds.
CREATE TABLE IF NOT EXISTS log.merkle_node (
  partition bigint   NOT NULL,
  level     smallint NOT NULL,                 -- 0 is the leaf level
  index     bigint   NOT NULL,
  hash      bytea    NOT NULL,
  PRIMARY KEY (partition, level, index)
);

CREATE TABLE IF NOT EXISTS log.checkpoint (
  partition   bigint   NOT NULL,
  tree_size   bigint   NOT NULL,
  root        bytea    NOT NULL,
  key_id      text     NOT NULL,               -- the signed-note key hash of the signing key, 8 hex digits
  signed_note text     NOT NULL,               -- the C2SP note, verbatim
  written_at  timestamptz NOT NULL,
  PRIMARY KEY (partition, tree_size)
);

CREATE TABLE IF NOT EXISTS log.segment_manifest (
  partition   bigint   NOT NULL,
  segment     bigint   NOT NULL,
  root        bytea    NOT NULL,
  replaces    bigint[] NOT NULL DEFAULT '{}',  -- manifests this one supersedes; empty since 0006 A14
  signed_note text     NOT NULL,
  sealed_at   timestamptz NOT NULL,
  PRIMARY KEY (partition, segment, sealed_at)
);
