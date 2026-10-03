-- The `private` schema (ADR 0013 §4, §5.6; 0027 §2): state that belongs to one account
-- and has no public history. Never exported; never joined with `view`. Every table
-- carries its portability class (0027 §2) as a comment, which `triplespace-db` checks
-- against its own PRIVATE_TABLES list so that a table cannot exist without a class:
--   portable        — may leave with the account in a user data bundle
--   re-established  — secrets and proofs an instance mints for itself; reissued elsewhere
--   never-leaves    — not the person's, or bound to this instance
-- The milestone-1 tables; the rest (watch, inbox, preference, email, …) follow with the
-- crates that read them.

CREATE SCHEMA IF NOT EXISTS private;

CREATE TABLE IF NOT EXISTS private.password (     -- the built-in `password` issuer (0007 A3)
  actor_key text NOT NULL PRIMARY KEY,            -- local:{id}
  hash      text NOT NULL,                        -- a PHC string
  updated   timestamptz NOT NULL DEFAULT now()
);
COMMENT ON TABLE private.password IS 'portability: re-established';

CREATE TABLE IF NOT EXISTS private.api_key (      -- subsidiary accounts' keys (0024 §4, §10)
  actor_key  text NOT NULL,                       -- the subsidiary's actor key
  key_id     text NOT NULL UNIQUE,                -- carried by sessions so revocation can find them
  label      text NOT NULL,                       -- chosen by the operator ("toolforge", "laptop")
  hash       text NOT NULL,                       -- of the secret, which is shown once
  grants     text[] NOT NULL DEFAULT '{}',        -- 0024 §4
  ip_ranges  cidr[],                              -- NULL is unrestricted
  created    timestamptz NOT NULL DEFAULT now(),
  expires    timestamptz,
  last_used  timestamptz,
  revoked_at timestamptz,
  PRIMARY KEY (actor_key, key_id),
  UNIQUE (actor_key, label)
);
-- Metadata (labels, grants, ranges, expiries) is portable; the hash is re-established.
-- The table takes the stricter class and the export reads only the metadata columns.
COMMENT ON TABLE private.api_key IS 'portability: re-established';
