-- Cache purges (03 §9.6; 0056 §7; 0014 §5): what a visibility change, a deletion or a
-- hiding makes stale in the layers that hold public forms. The projections append rows
-- here; the web tier, a CDN hook or an operator's script reads and acknowledges them
-- (`GET /purges`). Nothing in the API serves from a cache yet, so the queue is the
-- record of what would have to go; a layer that appears consumes it.

CREATE TABLE IF NOT EXISTS ops.purge (
  id        bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant    text   NOT NULL DEFAULT '',
  tag       text   NOT NULL,                      -- a Cache-Tag: entity:Q42, page:17, tenant:slug, ns:4, set:12, actor:key
  reason    text   NOT NULL,                      -- acl | erase | hide
  partition bigint NOT NULL,                      -- the record that caused it
  "offset"  bigint NOT NULL,
  at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS purge_recent ON ops.purge (id);
CREATE INDEX IF NOT EXISTS purge_tenant ON ops.purge (tenant, id);

-- The per-tenant visibility epoch (03 §9.6): bumped by a read ACL on a namespace or the
-- tenant, part of every cache key for the tenant, so that every entry under the
-- enclosing target lapses at once.
CREATE TABLE IF NOT EXISTS ops.visibility_epoch (
  tenant text PRIMARY KEY,
  epoch  bigint NOT NULL DEFAULT 0
);
