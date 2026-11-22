-- The SPARQL Update stream's storage (ADR 0032 §3, §4; 0083 §7; 03 §4.11; 08 §10.5).
--
-- `view.rdf_delta` holds the deleted and inserted triples of every change to a public
-- graph, as sorted N-Triples lines. `seq` is assigned by the one delta consumer under a
-- single lock, so insert order is commit order and a reader of `seq > cursor` never skips
-- a row that committed late. A row with `tenant = ''` is a shared-row delta, stored once
-- and read by every tenant whose providers include `provider`; it is rendered under the
-- reserved base `https://tenant.invalid/`, which a tenant's stream replaces with its own
-- (0032 A16). `txn` is the transaction that composed the change, stamped when the work is
-- queued (0032 A15). Both sides are lz4-compressed by TOAST. Idempotent.

CREATE SEQUENCE IF NOT EXISTS view.rdf_delta_seq;

CREATE TABLE IF NOT EXISTS view.rdf_delta (
  seq              bigint      PRIMARY KEY,
  tenant           text        NOT NULL DEFAULT '',
  graph            text        NOT NULL,
  provider         text,
  entity_id        text,
  partition        bigint,
  "offset"         bigint,
  resolved_version bigint,
  deleted          bytea       NOT NULL,
  inserted         bytea       NOT NULL,
  txn              bigint      NOT NULL,
  time             timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE view.rdf_delta ALTER COLUMN deleted SET COMPRESSION lz4;
ALTER TABLE view.rdf_delta ALTER COLUMN inserted SET COMPRESSION lz4;

CREATE INDEX IF NOT EXISTS rdf_delta_txn ON view.rdf_delta (txn, seq);
CREATE INDEX IF NOT EXISTS rdf_delta_tenant ON view.rdf_delta (tenant, seq);
CREATE INDEX IF NOT EXISTS rdf_delta_entity ON view.rdf_delta (entity_id, seq);
CREATE INDEX IF NOT EXISTS rdf_delta_record ON view.rdf_delta (partition, "offset");

-- What the stream last showed of each entity in each graph (0032 A15): the old side of
-- the next delta. A tenant's row for a shared entity exists while the tenant overlays it.
CREATE TABLE IF NOT EXISTS view.rdf_shown (
  tenant    text  NOT NULL DEFAULT '',
  graph     text  NOT NULL,
  entity_id text  NOT NULL,
  provider  text,
  triples   bytea NOT NULL,
  PRIMARY KEY (tenant, graph, entity_id)
);

ALTER TABLE view.rdf_shown ALTER COLUMN triples SET COMPRESSION lz4;

-- The spans of `seq` over which a tenant overlaid a shared entity (0032 A16): a shared
-- delta inside one is not part of that tenant's stream. `to_seq` is NULL while it lasts.
CREATE TABLE IF NOT EXISTS view.rdf_overlay (
  tenant    text   NOT NULL,
  entity_id text   NOT NULL,
  from_seq  bigint NOT NULL,
  to_seq    bigint,
  PRIMARY KEY (tenant, entity_id, from_seq)
);

-- How many entities of a tenant's graph show each reference or value node (0032 A17).
-- Such a node is named by the hash of its content and shared by every statement that
-- uses it, so its triples leave the graph only when the last entity showing it does.
CREATE TABLE IF NOT EXISTS view.rdf_node (
  tenant text    NOT NULL DEFAULT '',
  graph  text    NOT NULL,
  node   text    NOT NULL,
  uses   integer NOT NULL,
  PRIMARY KEY (tenant, graph, node)
);

-- The stream epoch per tenant, '' for the shared stream (03 §4.11). A row exists once
-- the stream has been turned on, and work is queued while `enabled`; `filled` is false
-- until the consumer has recorded what the tenant's graphs show, after which changes
-- produce deltas. `horizon` is the `seq` before which the tenant's stream cannot be
-- resumed: where its epoch began, moved forward by the retention sweep.
CREATE TABLE IF NOT EXISTS ops.tenant_epoch (
  tenant            text    PRIMARY KEY,
  epoch             bigint  NOT NULL DEFAULT 0,
  rendering_version integer NOT NULL,
  enabled           boolean NOT NULL DEFAULT true,
  filled            boolean NOT NULL DEFAULT false,
  horizon           bigint  NOT NULL DEFAULT 0
);

-- Carried: starting again from 0 would let a consumer's cursor from before the restore
-- pass as current, and a restore bumps it, as it does `visibility_epoch`.
COMMENT ON TABLE ops.tenant_epoch IS 'backup: carried';
