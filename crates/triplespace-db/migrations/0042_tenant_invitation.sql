-- A tenant owner's invitation (ADR 0111 §5, A2; 03 §4.16; 08 §1.4): for an owner account
-- whose request named no login, a single-use code, kept as its SHA-256 only, valid for
-- seven days, which binds the account to the login its holder creates. One per account:
-- asking again replaces it. Private, like every row a person's sign-in depends on; the
-- schema's default privileges give it to `ts_accounts`.

CREATE TABLE IF NOT EXISTS private.tenant_invitation (
  tenant     text        NOT NULL,
  actor_key  text        NOT NULL,
  code_hash  bytea       NOT NULL UNIQUE,
  created    timestamptz NOT NULL DEFAULT now(),
  expires    timestamptz NOT NULL,
  created_by text        NOT NULL,
  used       timestamptz,
  PRIMARY KEY (tenant, actor_key)
);

COMMENT ON TABLE private.tenant_invitation IS 'portability: never-leaves';
