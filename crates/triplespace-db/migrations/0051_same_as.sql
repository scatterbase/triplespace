-- The `same-as` links a tenant has written, until identity clusters are projected (ADR 0078
-- A10). A row says that the tenant's own graphs link `id` and `other` as the same item;
-- each link has a row in each direction, so either member finds the other. Only the
-- resolution of an entity source's ID reads it (05 §6.5): `MHC:Q1` reaches the local item
-- minted for it (05 §6.6). Clusters replace it when they are built.
CREATE TABLE IF NOT EXISTS view.same_as (
  tenant      text   NOT NULL DEFAULT '',
  id          text   NOT NULL,
  other       text   NOT NULL,
  "partition" bigint NOT NULL,                     -- the graph that wrote the link
  "offset"    bigint NOT NULL,                     -- the record that wrote it
  PRIMARY KEY (tenant, id, other)
);

CREATE INDEX IF NOT EXISTS same_as_partition ON view.same_as ("partition");
