-- Threads, posts and talk pages (0019 §11; 03 §4.8; 0049 §12; 0069 §10; 0090 §9).
--
-- A thread and a talk page each have a row in view.page (namespace 214 with model
-- triplespace-thread; the paired talk namespace with model triplespace-talk), so the title
-- index covers them. Beside it, the tables of 03 §4.8, each with the tenant column every
-- view table carries. Target kinds: 1 page, 2 actor (14 §1.3).

CREATE TABLE IF NOT EXISTS view.talk_page (
  tenant      text     NOT NULL DEFAULT '',
  page_id     bigint   NOT NULL,
  target_kind smallint NOT NULL,
  target_id   text     NOT NULL,
  PRIMARY KEY (tenant, page_id),
  UNIQUE (tenant, target_kind, target_id)
);

CREATE TABLE IF NOT EXISTS view.thread (
  tenant       text     NOT NULL DEFAULT '',
  page_id      bigint   NOT NULL,
  target_kind  smallint NOT NULL,                 -- the home
  target_id    text     NOT NULL,
  talk_page_id bigint,                            -- the home's talk page; NULL until minted
  subject      text     NOT NULL,
  status       text     NOT NULL DEFAULT 'open',
  created      timestamptz NOT NULL,
  last_post    timestamptz NOT NULL,
  post_count   integer  NOT NULL,
  PRIMARY KEY (tenant, page_id)
);

CREATE TABLE IF NOT EXISTS view.thread_attachment (
  tenant         text     NOT NULL DEFAULT '',
  thread_id      bigint   NOT NULL,
  talk_page_id   bigint   NOT NULL,
  home           boolean  NOT NULL,
  attached       timestamptz NOT NULL,
  attached_revid bigint   NOT NULL,               -- the record that attached it here
  pinned         timestamptz,                     -- when it was pinned here, if it is (0069 §4)
  PRIMARY KEY (tenant, thread_id, talk_page_id)
);
CREATE INDEX IF NOT EXISTS thread_attachment_talk ON view.thread_attachment (tenant, talk_page_id, attached);
CREATE UNIQUE INDEX IF NOT EXISTS thread_attachment_home ON view.thread_attachment (tenant, thread_id) WHERE home;

CREATE TABLE IF NOT EXISTS view.post (
  tenant        text     NOT NULL DEFAULT '',
  revid         bigint   NOT NULL,
  thread_id     bigint   NOT NULL,
  parent_revid  bigint,
  actor         text,
  published     timestamptz NOT NULL,
  updated       timestamptz,
  latest_offset bigint   NOT NULL,                -- the record holding the current text
  hidden        smallint NOT NULL DEFAULT 0,
  erased        boolean  NOT NULL DEFAULT false,
  dt_name       text,                             -- an imported post's DiscussionTools name and ID (0090 §9)
  dt_id         text,
  original      timestamptz,                      -- an imported post's original time (0090 §6)
  PRIMARY KEY (tenant, revid)
);
CREATE INDEX IF NOT EXISTS post_thread ON view.post (tenant, thread_id, published);
CREATE INDEX IF NOT EXISTS post_dt_name ON view.post (tenant, dt_name) WHERE dt_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS post_dt_id ON view.post (tenant, dt_id) WHERE dt_id IS NOT NULL;
