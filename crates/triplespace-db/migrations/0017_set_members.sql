-- Sets (0056 §3; 09 §4.7; 03 §4.14): the sets a target is in, one indexed lookup per
-- read. Written by the ACL projection from each `set` ACL's `members`, and replaced
-- whole when the set's record changes.
CREATE TABLE IF NOT EXISTS view.set_member (
  tenant text   NOT NULL DEFAULT '',
  set_id bigint NOT NULL,
  kind   text   NOT NULL,                           -- page | entity
  id     text   NOT NULL,                           -- the page ID or the entity ID
  PRIMARY KEY (tenant, set_id, kind, id)
);
CREATE INDEX IF NOT EXISTS set_member_target ON view.set_member (tenant, kind, id);
