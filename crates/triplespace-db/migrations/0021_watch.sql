-- The watch set (0020 §3; 16 §4.3): an account's watched targets, each with an optional
-- expiry, the newest change the holder has seen, and whether its changes are also delivered
-- as notifications (0021 §2). Private state, read and written by the accounts role, and
-- read by the notifier for `watch` notifications; never projected, never exported but in
-- the user data bundle.
--
-- A target is (kind, id) in `view.activity`'s numbering: 1 an entity by ID, 2 a page by page
-- ID (a document page, a board or a thread), 3 an actor by key (their user page and user
-- talk page), 6 a title with no page yet, as `{ns}:{title}`. A talk page is watched as its
-- subject, so the star on either toggles the same row.

CREATE TABLE IF NOT EXISTS private.watch (
  actor_key   text        NOT NULL,
  target_kind smallint    NOT NULL,
  target_id   text        NOT NULL,
  added       timestamptz NOT NULL DEFAULT now(),
  expires     timestamptz,
  seen        timestamptz,            -- MediaWiki's wl_notificationtimestamp, inverted: the newest change seen
  notify      boolean     NOT NULL DEFAULT false,
  PRIMARY KEY (actor_key, target_kind, target_id)
);
CREATE INDEX IF NOT EXISTS watch_expiry ON private.watch (expires) WHERE expires IS NOT NULL;
CREATE INDEX IF NOT EXISTS watch_target ON private.watch (target_kind, target_id) WHERE notify;
COMMENT ON TABLE private.watch IS 'portability: portable';

GRANT SELECT ON private.watch TO ts_notify;
