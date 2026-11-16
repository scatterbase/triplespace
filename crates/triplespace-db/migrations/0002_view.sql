-- The `view` schema (ADR 0013 §5): every table the API and UI read, rebuildable from
-- `log` at any time. The milestone-1 set (projections 1–5 of 0013 §7 minus clusters,
-- statement provenance, constraints, RDF deltas, pages' links, threads); the rest are
-- added by later migrations as their projections are written.
--
-- `tenant text NOT NULL DEFAULT ''` leads every primary key (0013 §5, A11): '' is the
-- instance, and for `entity` and the tables that hang off it the row with tenant = '' is
-- the shared row computed from the shared source graphs. `partition` columns are bigint,
-- as in `log` (0013 §2). Times that are header fields stay µs-since-epoch bigints; times
-- the projection derives for display are timestamptz.

CREATE SCHEMA IF NOT EXISTS view;

-- 5.5 configuration ----------------------------------------------------------------

CREATE TABLE IF NOT EXISTS view.registry (        -- current configuration (0015 §3; 0018 §3)
  tenant   text  NOT NULL DEFAULT '',             -- '' for instance-level kinds
  kind     text  NOT NULL,
  code     text  NOT NULL,
  config   jsonb NOT NULL,                        -- the entry; a retired entry has no row
  "offset" bigint NOT NULL,                       -- the config record that set it
  PRIMARY KEY (tenant, kind, code)
);

-- 5.4 keyed types ------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS view.keyed_surrogate ( -- 0009 §7; key is NULLed on erasure
  tenant     text   NOT NULL DEFAULT '',
  keyed_type text   NOT NULL,
  surrogate  bigint NOT NULL,
  key        text,
  PRIMARY KEY (tenant, keyed_type, surrogate),
  UNIQUE NULLS NOT DISTINCT (tenant, keyed_type, key)
);

CREATE TABLE IF NOT EXISTS view.keyed_map (       -- 0009 §9: upstream item <-> key
  tenant      text NOT NULL DEFAULT '',
  provider    text NOT NULL,
  upstream_id text NOT NULL,
  keyed_type  text NOT NULL,
  key         text NOT NULL,
  PRIMARY KEY (tenant, provider, upstream_id),
  UNIQUE (tenant, keyed_type, key, provider)
);

-- 5.4 actors; 0016 §9 groups, memberships, blocks, ACLs ----------------------------

CREATE TABLE IF NOT EXISTS view.actor (           -- 0007 §4; name and raw are NULLed on erasure
  tenant    text NOT NULL DEFAULT '',             -- the farm's accounts have tenant = '' (0028 §12)
  actor_key text NOT NULL,
  issuer    text NOT NULL,
  subject   text NOT NULL,
  kind      text NOT NULL,                        -- registered | temporary | bot | anonymous | imported | provider | federated
  name      text,
  status    text NOT NULL,                        -- active | renamed | vanished | hidden | retired | pending
  raw       text,                                 -- a surrogate's raw value: IP, imported name, remote actor IRI
  operator  text,                                 -- the operator's actor key for a subsidiary (0024 §1)
  groups    text[] NOT NULL DEFAULT '{}',         -- 0016 §9
  iri       text NOT NULL,
  "offset"  bigint NOT NULL,                      -- the latest actor record
  PRIMARY KEY (tenant, actor_key)
);
CREATE UNIQUE INDEX IF NOT EXISTS actor_local_name
  ON view.actor (tenant, lower(name)) WHERE issuer = 'local' AND name IS NOT NULL;

CREATE TABLE IF NOT EXISTS view.account_link (    -- public links only (0007 §7); deleted on unlink
  tenant        text NOT NULL DEFAULT '',
  local_actor   text NOT NULL,
  foreign_actor text NOT NULL,
  "offset"      bigint NOT NULL,
  PRIMARY KEY (tenant, local_actor, foreign_actor),
  UNIQUE (tenant, foreign_actor)
);

CREATE TABLE IF NOT EXISTS view."group" (         -- 0016 §3: a permission group's current definition
  tenant      text   NOT NULL DEFAULT '',         -- '' for a global group (0028 §8)
  name        text   NOT NULL,
  permissions text[] NOT NULL DEFAULT '{}',
  scope       text,                               -- a global group's scope, else NULL
  "offset"    bigint NOT NULL,
  PRIMARY KEY (tenant, name)
);

CREATE TABLE IF NOT EXISTS view.membership (      -- 0016 §3: current memberships, by layer
  tenant    text   NOT NULL DEFAULT '',
  actor_key text   NOT NULL,
  "group"   text   NOT NULL,
  layer     text   NOT NULL,                      -- tenant | instance (0040 §5: the instance layer is a floor)
  expires   bigint,                               -- µs since the epoch, or NULL
  "offset"  bigint NOT NULL,
  PRIMARY KEY (tenant, actor_key, "group", layer)
);

CREATE TABLE IF NOT EXISTS view.block (           -- 0016 §3: current blocks, by layer
  tenant    text   NOT NULL DEFAULT '',
  actor_key text   NOT NULL,
  layer     text   NOT NULL,                      -- tenant | instance
  removes   text[],                               -- the permissions removed; NULL is all-but-read
  expires   bigint,
  "offset"  bigint NOT NULL,
  PRIMARY KEY (tenant, actor_key, layer)
);

CREATE TABLE IF NOT EXISTS view.acl (             -- 0016 §4; 0023 §3; 0056 §14: the current ACL of a target
  tenant       text  NOT NULL DEFAULT '',
  target       text  NOT NULL,                    -- acl:page:7, acl:entity:Q5, acl:record:3:99, acl:actor:…, acl:set:8, acl:tenant:…
  restrictions jsonb NOT NULL,                    -- {action: {group, expires}}
  read_kind    text,                              -- confidential | moderation, when `read` is restricted
  extra        jsonb,                             -- talk, parts, reserved, name, members (payloads-actors.md)
  "offset"     bigint NOT NULL,
  PRIMARY KEY (tenant, target)
);

-- 5.1 entities --------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS view.entity (
  tenant           text    NOT NULL DEFAULT '',   -- '' is the shared row (0018 §6)
  id               text    NOT NULL,              -- local, foreign or keyed ID in the form of 0017 §1
  type             text    NOT NULL,              -- item, property, domain, …
  page_id          bigint  NOT NULL,              -- 0013 §6
  surrogate        bigint,                        -- keyed types only (0009 §7)
  cluster_id       bigint,                        -- NULL when the entity is in no cluster
  canonical_id     text    NOT NULL,              -- itself unless a non-canonical member (0004 §4)
  retention        text,                          -- cascade | orphan | retain (0002 §5)
  first_seen       timestamptz NOT NULL,
  local_offset     bigint,                        -- latest local-graph record for this key
  local_revid      bigint,                        -- lastrevid for the Action API
  resolved_version bigint  NOT NULL,              -- monotonic; bumped on every re-resolution (0014 §3)
  generation       integer NOT NULL DEFAULT 0,    -- bumped on erasure and hiding (0014 §5)
  resolved_kind    text    NOT NULL,              -- source | materialized
  resolved         bytea,                         -- canonical JSON, compressed; NULL when source
  resolved_size    integer NOT NULL,
  deleted          boolean NOT NULL DEFAULT false,-- derived from a read ACL on the entity (0023 §4, §10)
  PRIMARY KEY (tenant, id),
  UNIQUE (tenant, page_id)
);

CREATE TABLE IF NOT EXISTS view.entity_source (   -- the version cursor of 0002 §8.4, per graph
  tenant           text   NOT NULL DEFAULT '',
  entity_id        text   NOT NULL,
  graph            text   NOT NULL,               -- local, mirror/wikidata, …
  "offset"         bigint NOT NULL,               -- the graph's current record for the entity
  upstream_version text,                          -- lastrevid, updated_date, …
  prev_upstream    text,                          -- 0012 §2.2
  content_hash     bytea  NOT NULL,               -- H(0x03 ‖ payload)
  size             integer NOT NULL,              -- 0012 §2.4
  synced_at        timestamptz NOT NULL,
  job_id           bigint,
  provider_revid   text,                          -- 0022 §2
  verified_at_size bigint,                        -- 0022 §2
  PRIMARY KEY (tenant, entity_id, graph)
);

-- 5.2 terms, identifiers, references ---------------------------------------------

CREATE TABLE IF NOT EXISTS view.term (            -- the resolved fingerprint (wikibase-compat §2)
  tenant    text     NOT NULL DEFAULT '',
  entity_id text     NOT NULL,
  kind      smallint NOT NULL,                    -- 1 label, 2 description, 3 alias
  lang      text     NOT NULL,
  ordinal   smallint NOT NULL DEFAULT 0,
  text      text     NOT NULL,
  PRIMARY KEY (tenant, entity_id, kind, lang, ordinal)
);
-- Prefix search without OpenSearch (0014 §8): the small profile's fallback.
CREATE INDEX IF NOT EXISTS term_prefix ON view.term (tenant, lang, kind, lower(text) text_pattern_ops);

CREATE TABLE IF NOT EXISTS view.identifier (      -- normalized external-id values (0004 §7)
  tenant       text NOT NULL DEFAULT '',
  property     text NOT NULL,
  value_key    text NOT NULL,
  entity_id    text NOT NULL,
  graph        text NOT NULL,
  statement_id text NOT NULL,
  PRIMARY KEY (tenant, property, value_key, entity_id, graph)
);

CREATE TABLE IF NOT EXISTS view.match_key (       -- uniqueness for designated properties (0002 §8.5)
  tenant    text NOT NULL DEFAULT '',
  property  text NOT NULL,
  value_key text NOT NULL,
  entity_id text NOT NULL,
  PRIMARY KEY (tenant, property, value_key)
);

CREATE TABLE IF NOT EXISTS view.entity_ref (      -- the reverse index (0004 Consequences, 0008 §10)
  tenant       text     NOT NULL DEFAULT '',
  target_id    text     NOT NULL,
  source_id    text     NOT NULL,
  statement_id text     NOT NULL,
  role         smallint NOT NULL,                 -- 1 main, 2 qualifier, 3 reference
  PRIMARY KEY (tenant, target_id, source_id, statement_id, role)
);

-- 5.4 pages ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS view.page (            -- 0008 §4
  tenant        text    NOT NULL DEFAULT '',
  page_id       bigint  NOT NULL,
  ns            integer NOT NULL,
  title         text    NOT NULL,
  content_model text    NOT NULL,                 -- a content model registry ID (0041 §3, §10)
  owner_actor   text,
  latest_offset bigint,                           -- NULL while the page ID is only reserved (0023 §2)
  latest_revid  bigint,
  reserved      boolean NOT NULL DEFAULT false,   -- create-protection holds the ID (0023 §2, §10)
  deleted       boolean NOT NULL DEFAULT false,   -- derived from a read ACL on the page (0023 §4)
  is_redirect   boolean NOT NULL DEFAULT false,   -- 0051 §5
  generation    integer NOT NULL DEFAULT 0,
  PRIMARY KEY (tenant, page_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS page_title ON view.page (tenant, ns, title) WHERE NOT deleted;

-- 5.5 activity and jobs ------------------------------------------------------------

CREATE TABLE IF NOT EXISTS view.activity (        -- 0010 §13, 0012 §3: one row per local record, job or event
  tenant        text   NOT NULL DEFAULT '',
  time          timestamptz NOT NULL,
  partition     bigint NOT NULL,
  "offset"      bigint NOT NULL,
  kind          text   NOT NULL,                  -- edit | sync | log | job | erased
  source        text   NOT NULL,
  revid         bigint,
  logid         bigint,
  target_kind   smallint,
  target_id     text,
  target_member text,
  actor_key     text,
  operator      text,
  requested_by  text,
  job_id        bigint,
  summary_op    text,
  summary_args  text,
  comment       text,
  tags          text[] NOT NULL DEFAULT '{}',     -- from the attestation part (0030 §5)
  minor         boolean,
  bot           boolean,
  new           boolean,
  size          integer,
  delta         integer,
  visibility    smallint NOT NULL DEFAULT 0,      -- record ACLs and the erased bitmask (0023 §5)
  patrolled     boolean NOT NULL DEFAULT false,   -- 0023 §6
  read_groups   text[],                           -- 0056 §14; NULL is public
  log_type      text,
  log_action    text,
  params        jsonb,
  changes       jsonb,                            -- sync summaries (0012 §2.2)
  PRIMARY KEY (tenant, partition, "offset")
);
CREATE INDEX IF NOT EXISTS activity_time   ON view.activity (tenant, time DESC, partition, "offset");
CREATE INDEX IF NOT EXISTS activity_actor  ON view.activity (tenant, actor_key, time DESC);
CREATE INDEX IF NOT EXISTS activity_target ON view.activity (tenant, target_kind, target_id, time DESC);
CREATE INDEX IF NOT EXISTS activity_job    ON view.activity (tenant, job_id) WHERE job_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS activity_unpatrolled ON view.activity (tenant, time DESC) WHERE NOT patrolled;
CREATE UNIQUE INDEX IF NOT EXISTS activity_revid ON view.activity (tenant, revid) WHERE revid IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS activity_logid ON view.activity (tenant, logid) WHERE logid IS NOT NULL;

CREATE TABLE IF NOT EXISTS view.job (             -- projected from the local log (0011 §6.3)
  tenant          text   NOT NULL DEFAULT '',
  job_id          bigint NOT NULL,
  actor_key       text   NOT NULL,
  requested_by    text,
  source          text   NOT NULL,
  source_version  text,
  adapter_version text,
  mode            text,
  graph           text   NOT NULL,
  params          jsonb  NOT NULL,
  status          text   NOT NULL,
  started         timestamptz NOT NULL,
  finished        timestamptz,
  counts          jsonb,
  checkpoint_size bigint,
  sweep_count     integer,
  sweep_threshold integer,
  start_offset    bigint NOT NULL,
  finish_offset   bigint,
  PRIMARY KEY (tenant, job_id)
);

CREATE TABLE IF NOT EXISTS view.job_reject (      -- the first N rejects for the job page (0010 §9)
  tenant    text   NOT NULL DEFAULT '',
  job_id    bigint NOT NULL,
  line      bigint NOT NULL,
  match_key text,
  reason    text   NOT NULL,
  PRIMARY KEY (tenant, job_id, line)
);
