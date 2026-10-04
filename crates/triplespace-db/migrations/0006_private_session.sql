-- Sessions (0024 §4; 0014 §2, A-amended): the session store of an instance without a
-- shared cache. A row is one browser or bot session; the cookie value is random and is the
-- key. Nothing here is the person's: a session is bound to this instance and dies with it.
-- Actor keys here are tenant-qualified, `librarybase:7`, as `view.actor` writes them.

CREATE TABLE IF NOT EXISTS private.session (
  id         text NOT NULL PRIMARY KEY,           -- the cookie value, random
  tenant     text NOT NULL,                       -- the tenant host the session was opened on
  actor_key  text,                                -- NULL for an anonymous session (holds only tokens)
  key_id     text,                                -- the API key a bot-password login used (0024 §4)
  created    timestamptz NOT NULL DEFAULT now(),
  expires    timestamptz NOT NULL,
  last_seen  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS session_key_id ON private.session (key_id) WHERE key_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS session_expires ON private.session (expires);
COMMENT ON TABLE private.session IS 'portability: never-leaves';
