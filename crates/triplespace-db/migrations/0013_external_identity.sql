-- External identity providers (ADR 0088 §3, §5, §7).
--
-- `private.oauth_client` holds the instance's own client credential for an issuer that
-- serves login through OAuth: one row per tenant and issuer, `tenant = ''` being the
-- instance default (§3). The secret is sealed under the instance sealing key, with the
-- table, tenant and issuer as associated data; `callback` is the URL the consumer was
-- registered with, against which a tenant's own callback is matched.
--
-- `private.oauth_flow` holds a login flow's ten-minute handle where there is no shared
-- cache (§5): the hash of its `state`, the session it began in, its purpose, the PKCE
-- verifier, and once the provider has answered, the outcome. A login that found no
-- account keeps the row as the session's pending identity until it expires (§6).
--
-- `private.binding` indexes the bindings to external issuers (§7). The binding records
-- in the tenant's `accounts` partition are the truth; a row points at its record so that
-- removing the sign-in method can erase it. A password binding's index is
-- `private.password`.
-- Idempotent: every statement tolerates having run before.

CREATE TABLE IF NOT EXISTS private.oauth_client (
  tenant    text        NOT NULL,                -- '' for the instance default
  issuer    text        NOT NULL,
  client_id text        NOT NULL,
  secret    bytea       NOT NULL,                -- nonce ‖ XChaCha20-Poly1305 ciphertext
  callback  text        NOT NULL,                -- the URL registered with the provider
  created   timestamptz NOT NULL DEFAULT now(),
  updated   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant, issuer)
);
COMMENT ON TABLE private.oauth_client IS 'portability: never-leaves';

CREATE TABLE IF NOT EXISTS private.oauth_flow (
  state_hash text        NOT NULL PRIMARY KEY,   -- SHA-256 of `state`, hex
  tenant     text        NOT NULL,
  session    text        NOT NULL,               -- the session the flow began in
  issuer     text        NOT NULL,
  purpose    text        NOT NULL CHECK (purpose IN ('login', 'create', 'link')),
  door       text        NOT NULL CHECK (door IN ('site', 'action')),
  verifier   text        NOT NULL,               -- the PKCE code verifier
  name       text,                               -- the name chosen, for `create`
  returnto   text,
  outcome    jsonb,                              -- {"issuer","subject","username"} or {"error"}
  expires    timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS oauth_flow_session ON private.oauth_flow (tenant, session);
CREATE INDEX IF NOT EXISTS oauth_flow_expires ON private.oauth_flow (expires);
COMMENT ON TABLE private.oauth_flow IS 'portability: never-leaves';

CREATE TABLE IF NOT EXISTS private.binding (
  tenant      text        NOT NULL,
  issuer      text        NOT NULL,
  subject     text        NOT NULL,              -- normalized: the bare number
  actor_key   text        NOT NULL,
  "partition" bigint      NOT NULL,              -- the binding record
  "offset"    bigint      NOT NULL,
  added       timestamptz NOT NULL DEFAULT now(),
  last_used   timestamptz,
  PRIMARY KEY (tenant, issuer, subject)
);
CREATE INDEX IF NOT EXISTS binding_actor ON private.binding (actor_key);
COMMENT ON TABLE private.binding IS 'portability: re-established';
