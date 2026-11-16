-- Forwarder keys (0057 §10): instance credentials that let a proxy vouch for the client
-- address it appends to X-Forwarded-For. A key is `{key ID}.{secret}`; only the secret's
-- SHA-256 is kept. Issuing and revoking a key is also a `forwarder` record in the
-- instance `config` partition (0015 §3), which holds the ID, label and dates and never
-- the secret or its hash.

CREATE TABLE IF NOT EXISTS private.forwarder_key (
  key_id     text NOT NULL PRIMARY KEY,           -- random; the record key `forwarder:{key_id}`
  label      text NOT NULL,                       -- chosen by the operator ("edge", "web")
  hash       text NOT NULL,                       -- of the secret, which is shown once
  created    timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz
);
CREATE UNIQUE INDEX IF NOT EXISTS forwarder_key_live_label
  ON private.forwarder_key (label) WHERE revoked_at IS NULL;
COMMENT ON TABLE private.forwarder_key IS 'portability: never-leaves';
