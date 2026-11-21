-- The fetch queue (ADR 0070 §2.2, §3, §10; 0078 §4; 0092 §4; 03 §4.12): entities waiting
-- to be read from their provider or source and written to its mirror. `tenant` is '' for
-- a registry provider and the tenant for its entity source; `provider` is the provider's
-- slug or the source's name; `entity_id` is the ID as the upstream wiki knows it (`Q42`),
-- so that a busy entity is queued once whatever form named it. The follower of the
-- provider or source drains it with its polls (0070 A9), deleting the rows a batch fetched
-- in the transaction that commits the batch. `depth` is how many closure steps led to the
-- entity: 0 for one queued for itself, n + 1 for a closure value of one at n, so that
-- the walk stops at `entities.closure_depth` (0070 §2.3). Idempotent.

CREATE TABLE IF NOT EXISTS ops.entity_fetch (
  tenant    text        NOT NULL DEFAULT '',
  provider  text        NOT NULL,
  entity_id text        NOT NULL,
  reason    text        NOT NULL CHECK (reason IN ('read', 'linked', 'closure', 'event', 'sweep', 'manual', 'crawl')),
  enqueued  timestamptz NOT NULL DEFAULT now(),
  attempts  integer     NOT NULL DEFAULT 0,
  depth     smallint    NOT NULL DEFAULT 0,
  PRIMARY KEY (tenant, provider, entity_id)
);

CREATE INDEX IF NOT EXISTS entity_fetch_by_age ON ops.entity_fetch (tenant, provider, enqueued);

-- Queued work: carried, so that a restore fetches what was waiting, as `job_queue` is (0103 A2).
COMMENT ON TABLE ops.entity_fetch IS 'backup: carried';
