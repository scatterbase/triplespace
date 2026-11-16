-- Instance logins (ADR 0089 §2, §3, §4, §5; 03 §4.16; 07 §2.8).
--
-- `private.login` indexes the `logins` partition's login records: one row per login, with
-- its private name and the record that holds it. `id` is the login's instance-wide ID, the
-- subject of the built-in `login` issuer, minted by `private.login_id`.
--
-- Passwords belong to the login (0089 §3): `private.password` gains `login`, and `actor_key`
-- stays nullable for `triplespace logins migrate` to read and clear; a row is one or the
-- other. `private.binding`'s holder is no longer always an account: a login's external
-- identities are rows with the empty tenant whose holder is `login:{n}`, and a tenant
-- account's binding to its login a row with the tenant, issuer `login` and the login ID as
-- subject, whose holder is the actor key; the column is renamed to say so. `private.session`
-- gains the login a session holds and, for a tenant session opened from a central one, that
-- central session (0089 §5). Idempotent: every statement tolerates having run before.

CREATE SEQUENCE IF NOT EXISTS private.login_id;

CREATE TABLE IF NOT EXISTS private.login (
  id          bigint      NOT NULL PRIMARY KEY DEFAULT nextval('private.login_id'),
  name        text        NOT NULL,              -- the private login name, normalized
  name_key    text        NOT NULL,              -- lower-cased, for uniqueness
  created     timestamptz NOT NULL DEFAULT now(),
  last_used   timestamptz,
  "partition" bigint      NOT NULL,              -- the login record in `logins`
  "offset"    bigint      NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS login_name_key ON private.login (name_key);
COMMENT ON TABLE private.login IS 'portability: never-leaves';

ALTER TABLE private.password DROP CONSTRAINT IF EXISTS password_pkey;
ALTER TABLE private.password ALTER COLUMN actor_key DROP NOT NULL;
ALTER TABLE private.password ADD COLUMN IF NOT EXISTS login bigint;
CREATE UNIQUE INDEX IF NOT EXISTS password_actor_key ON private.password (actor_key) WHERE actor_key IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS password_login ON private.password (login) WHERE login IS NOT NULL;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
             WHERE table_schema = 'private' AND table_name = 'binding' AND column_name = 'actor_key') THEN
    ALTER TABLE private.binding RENAME COLUMN actor_key TO holder;
  END IF;
END $$;
CREATE INDEX IF NOT EXISTS binding_holder ON private.binding (holder);

ALTER TABLE private.session ADD COLUMN IF NOT EXISTS login bigint;
ALTER TABLE private.session ADD COLUMN IF NOT EXISTS central text;
CREATE INDEX IF NOT EXISTS session_login ON private.session (login) WHERE login IS NOT NULL;
