-- Followers (0092 §3, §10): where each follower of an upstream is, and the optional bot
-- password it logs in with.
--
-- `ops.repo_cursor` is a follower's position, keyed by (tenant, kind, name): '' and `repo`
-- for a page repository's stream or poll, '' and `provider` for a registry provider's, the
-- tenant and `source` for an entity source's. An event stream keeps its event ID; a
-- recent-changes poll its newest time, its continuation and the change IDs within the tail
-- window. The position is a hint written in the batch's transaction; the version cursor
-- (`view.entity_source`) is the guarantee, so a lost row is rebuilt from it.
--
-- `private.upstream_login` holds a bot password per follower, the secret sealed under the
-- instance sealing key as `private.oauth_client`'s is; "never leaves" (0027 §2).

CREATE TABLE IF NOT EXISTS ops.repo_cursor (
  tenant          text        NOT NULL DEFAULT '',
  kind            text        NOT NULL,            -- repo | provider | source
  name            text        NOT NULL,            -- the repository, the provider slug or the source name
  last_event_id   text,                            -- an event stream's
  last_event_time timestamptz,
  rc_time         timestamptz,                     -- a poll's newest time seen
  rc_continue     jsonb,                           -- its continuation, while there is one
  rc_seen         jsonb,                           -- [[rcid, epoch seconds], …] within follow.tail
  rc_retry        jsonb,                           -- [[entity ID, revid], …]: states read stale, to read again
  loaded_to       timestamptz,                     -- where the last load ended (0092 §5)
  handed_over     boolean     NOT NULL DEFAULT false,
  late            bigint      NOT NULL DEFAULT 0,  -- rows the tail pass found the head missed
  stale           bigint      NOT NULL DEFAULT 0,  -- states older than the row that named them
  lag_seconds     integer,
  updated         timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant, kind, name)
);

CREATE TABLE IF NOT EXISTS private.upstream_login (
  tenant    text        NOT NULL DEFAULT '',
  kind      text        NOT NULL,
  name      text        NOT NULL,
  username  text        NOT NULL,                  -- the bot password's login name, `User@label`
  secret    bytea       NOT NULL,                  -- nonce ‖ XChaCha20-Poly1305 ciphertext
  created   timestamptz NOT NULL DEFAULT now(),
  updated   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant, kind, name)
);
COMMENT ON TABLE private.upstream_login IS 'portability: never-leaves';
