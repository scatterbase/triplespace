-- Document pages (0008 §4; 03 §4.5, §4.7; 0047 §13; 0051 §5; 0083 §6): the columns the
-- page projection maintains on view.page, and the redirect, link and category tables it
-- writes from a page's stored text. A page's revisions are rows of view.entity_revision
-- (0083 §4), keyed by the page ID in decimal, the key form of a page subject (03 §4.4),
-- with the revision's text as the state. Idempotent: every statement tolerates having
-- run before.

ALTER TABLE view.page ADD COLUMN IF NOT EXISTS len integer;                -- bytes of the latest text, as page_len
ALTER TABLE view.page ADD COLUMN IF NOT EXISTS latest_at timestamptz;
ALTER TABLE view.page ADD COLUMN IF NOT EXISTS revisions integer NOT NULL DEFAULT 0;
ALTER TABLE view.page ADD COLUMN IF NOT EXISTS random double precision NOT NULL DEFAULT random();  -- page_random
CREATE INDEX IF NOT EXISTS page_len       ON view.page (tenant, ns, len);
CREATE INDEX IF NOT EXISTS page_latest_at ON view.page (tenant, ns, latest_at);
CREATE INDEX IF NOT EXISTS page_revisions ON view.page (tenant, ns, revisions);
CREATE INDEX IF NOT EXISTS page_random    ON view.page (tenant, ns, random);

CREATE TABLE IF NOT EXISTS view.redirect (       -- MediaWiki's redirect table (0051 §1)
  tenant         text    NOT NULL DEFAULT '',
  page_id        bigint  NOT NULL,
  target_ns      integer,                         -- NULL for an external redirect
  target_title   text    NOT NULL,
  fragment       text,
  interwiki      text,                            -- set for an external redirect
  target_page_id bigint,                          -- resolved at projection; NULL when broken or external
  PRIMARY KEY (tenant, page_id)
);
CREATE INDEX IF NOT EXISTS redirect_target ON view.redirect (tenant, target_ns, target_title);

CREATE TABLE IF NOT EXISTS view.page_link (      -- 0008 §10; both directions
  tenant      text     NOT NULL DEFAULT '',
  from_page   bigint   NOT NULL,
  target_kind smallint NOT NULL,                  -- 1 an entity, by ID; 6 a title, as {ns}:{title}
  target_id   text     NOT NULL,
  from_render boolean  NOT NULL DEFAULT false,    -- one writer per row class: the page projection, or the refresh job
  PRIMARY KEY (tenant, from_page, target_kind, target_id, from_render)
);
CREATE INDEX IF NOT EXISTS page_link_target ON view.page_link (tenant, target_kind, target_id);

CREATE TABLE IF NOT EXISTS view.page_category (  -- categorylinks (0038 §3)
  tenant      text    NOT NULL DEFAULT '',
  page_id     bigint  NOT NULL,
  category    text    NOT NULL,                   -- the normalized name, without the prefix
  sortkey     text    NOT NULL,
  from_render boolean NOT NULL DEFAULT false,
  PRIMARY KEY (tenant, page_id, category, from_render)
);
CREATE INDEX IF NOT EXISTS page_category_members ON view.page_category (tenant, category, sortkey);
