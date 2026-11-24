-- Identity clusters (ADR 0004; chapter 03 §4.4, 04 §4). A link, applied or held, is a row of
-- `view.link`; a `different-from` a row of `view.different_from`; the clusters the links form
-- are `view.cluster` with their members in `view.cluster_member`, whose primary key is the
-- one-to-one rule (0004 §2). Every table carries `tenant`: a tenant's own links are its rows,
-- and the shared rows (`tenant = ''`) hold what the shared graphs link.
--
-- `partition` and `offset` name the record that wrote a local link, so that a reset or an
-- erasure removes it; a mirror-derived link names its statement in `source` and the entity
-- whose statement it is in `subject`, so that the entity's next state replaces its links.
-- An inferred link (tier 3) names the property and the value key two entities share, as
-- `{property}={value key}`, in `source`.
--
-- A tenant's `declined` row, with a shared link's key, leaves that link out of the tenant's
-- clusters: it deprecates or suppresses the statement the link comes from (0004 §3).
--
-- `view.retired` holds the IDs that no longer stand for an entity of their own: one a
-- redirect retired is an alias of its `target` (0004 §1), and one upstream deleted has none
-- and leaves its clusters unless the tenant retains it (0004 §4). A mirror's rows are shared.
CREATE TABLE IF NOT EXISTS view.cluster (
  tenant       text   NOT NULL DEFAULT '',
  cluster_id   bigint NOT NULL,
  canonical_id text   NOT NULL,                    -- the highest-ranked member (0004 §4)
  version      bigint NOT NULL,                    -- bumped each time the cluster is recomputed
  inferred     boolean NOT NULL DEFAULT false,     -- an inferred link (tier 3) joins its members
  PRIMARY KEY (tenant, cluster_id)
);

CREATE TABLE IF NOT EXISTS view.cluster_member (
  tenant     text   NOT NULL DEFAULT '',
  cluster_id bigint NOT NULL,
  namespace  text   NOT NULL,                      -- local, a provider code, a keyed type, a source
  entity_id  text   NOT NULL,
  PRIMARY KEY (tenant, cluster_id, namespace),     -- one member per namespace (0004 §2)
  UNIQUE (tenant, entity_id)
);

CREATE TABLE IF NOT EXISTS view.link (
  tenant      text     NOT NULL DEFAULT '',
  a           text     NOT NULL,                   -- the lesser ID in text order
  b           text     NOT NULL,
  tier        smallint NOT NULL,                   -- 1 asserted, 2 mirror-derived, 3 inferred
  source      text     NOT NULL,                   -- statement ID, or the local record's offset
  status      text     NOT NULL,                   -- applied | held | inactive | declined
  conflict_id bigint,                              -- the conflict a held link is in (0004 §10)
  "partition" bigint,                              -- the record that wrote a local link
  "offset"    bigint,
  subject     text,                                -- a mirror-derived link's statement's entity
  PRIMARY KEY (tenant, a, b, tier, source)
);
CREATE INDEX IF NOT EXISTS link_subject ON view.link (tenant, subject) WHERE subject IS NOT NULL;
CREATE INDEX IF NOT EXISTS link_b ON view.link (tenant, b);
CREATE INDEX IF NOT EXISTS link_partition ON view.link ("partition");
CREATE INDEX IF NOT EXISTS link_held ON view.link (tenant, conflict_id) WHERE status = 'held';

CREATE TABLE IF NOT EXISTS view.different_from (
  tenant      text   NOT NULL DEFAULT '',
  a           text   NOT NULL,                     -- the lesser ID in text order
  b           text   NOT NULL,
  "partition" bigint NOT NULL,
  "offset"    bigint NOT NULL,
  PRIMARY KEY (tenant, a, b)
);
CREATE INDEX IF NOT EXISTS different_from_b ON view.different_from (tenant, b);
CREATE INDEX IF NOT EXISTS different_from_partition ON view.different_from ("partition");

CREATE TABLE IF NOT EXISTS view.retired (
  tenant      text   NOT NULL DEFAULT '',
  entity_id   text   NOT NULL,
  target      text,                                -- what a redirect made it an alias of; NULL when deleted
  "partition" bigint NOT NULL,                     -- the record that retired it
  "offset"    bigint NOT NULL,
  PRIMARY KEY (tenant, entity_id)
);
CREATE INDEX IF NOT EXISTS retired_target ON view.retired (tenant, target) WHERE target IS NOT NULL;
CREATE INDEX IF NOT EXISTS retired_partition ON view.retired ("partition");

-- The interim table of 0078 A10, which the clusters replace.
DROP TABLE IF EXISTS view.same_as;
