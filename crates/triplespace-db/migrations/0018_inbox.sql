-- Notifications (0021 §3; 16 §5.3): an account's inbox and its seen time, filled by the
-- addressing projection (triplespace-notify) from activity rows. Private state, read and
-- written by the notifier role only; rebuildable from the activity rows within the
-- retention window, except read state.

CREATE TABLE IF NOT EXISTS private.inbox (
  actor_key   text        NOT NULL,                -- the recipient, tenant-qualified
  id          bigint      GENERATED ALWAYS AS IDENTITY,
  partition   bigint      NOT NULL,                -- the activity row
  "offset"    bigint      NOT NULL,
  reason      text        NOT NULL,                -- 16 §5.2: rights, group-join, …
  target_kind smallint,
  target_id   text,
  published   timestamptz NOT NULL,
  read_at     timestamptz,
  PRIMARY KEY (actor_key, id),
  UNIQUE (actor_key, partition, "offset", reason)
);
CREATE INDEX IF NOT EXISTS inbox_recent ON private.inbox (actor_key, published DESC, id DESC);
CREATE INDEX IF NOT EXISTS inbox_activity ON private.inbox (partition, "offset");
COMMENT ON TABLE private.inbox IS 'portability: portable';

CREATE TABLE IF NOT EXISTS private.inbox_state (
  actor_key text PRIMARY KEY,
  seen_at   timestamptz
);
COMMENT ON TABLE private.inbox_state IS 'portability: portable';

GRANT SELECT, INSERT, UPDATE, DELETE ON private.inbox, private.inbox_state TO ts_notify, ts_accounts;
