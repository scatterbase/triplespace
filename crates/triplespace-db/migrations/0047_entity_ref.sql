-- The reverse index of entity references and its counter (03 §4.3; 0013 A108).
--
-- `view.entity_ref` was created by 0002 in an earlier shape, a row per statement and role,
-- and nothing has ever written it: it is made again here in the shape composition writes.
-- A row says that the entity `source_id` refers to `target_id` somewhere in its resolved
-- statements; `roles` says where. Both IDs are in source form. The target is an ID and not
-- a page ID because a referenced entity that no graph holds yet has no page.

DROP TABLE IF EXISTS view.entity_ref;

CREATE TABLE view.entity_ref (
  tenant    text     NOT NULL DEFAULT '',
  target_id text     NOT NULL,
  source_id text     NOT NULL,
  roles     smallint NOT NULL,                    -- bitmask: 1 main snak, 2 qualifier, 4 reference
  PRIMARY KEY (tenant, target_id, source_id)
);

-- A source's rows, replaced whenever it is composed.
CREATE INDEX entity_ref_source ON view.entity_ref (tenant, source_id);

-- How many entities of a tenant refer to a target: the backlink count, and what entity
-- search ranks by (03 §11.6). The count of a tenant's own rows; a reader adds the shared
-- rows' count (`tenant = ''`).
CREATE TABLE view.entity_ref_count (
  tenant    text   NOT NULL DEFAULT '',
  target_id text   NOT NULL,
  refs      bigint NOT NULL,
  PRIMARY KEY (tenant, target_id)
);

-- Changes to the count that are not folded into it yet. Composition appends here and
-- never updates a count, so two writers that refer to the same target do not wait on one
-- another; the worker folds a tenant's rows into `view.entity_ref_count` in one statement.
CREATE TABLE view.entity_ref_delta (
  tenant    text   NOT NULL DEFAULT '',
  target_id text   NOT NULL,
  d         bigint NOT NULL
);

CREATE INDEX entity_ref_delta_tenant ON view.entity_ref_delta (tenant);
