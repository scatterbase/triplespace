-- Settings and preferences (ADR 0085 §1, §2; 0027 §1, §8).
--
-- `view.registry` keeps who attested each configuration record. A `site` record in a
-- tenant's `config` that carries an instance attestation (a locked template's copy, or a
-- provision written before 0085) is not the tenant's own value, and setting resolution
-- passes over it (0085 §2.1). Rows projected before this migration have neither column
-- filled until the registry projection is rebuilt; they count as the tenant's own.
--
-- `private.preference` is the one preference store (0027 §1): a key–value pair per
-- account, the keys registered in `docs/registry/preferences.toml`. It is the person's
-- own choice about themselves, so it travels in the user data bundle.
-- Idempotent: every statement tolerates having run before.

ALTER TABLE view.registry ADD COLUMN IF NOT EXISTS actor text;               -- the attestation's actor key
ALTER TABLE view.registry ADD COLUMN IF NOT EXISTS instance_act boolean NOT NULL DEFAULT false;  -- the attestation names an authority (08 §8.3)

CREATE TABLE IF NOT EXISTS private.preference (
  actor_key text        NOT NULL,                 -- tenant-qualified, `librarybase:7`
  key       text        NOT NULL,                 -- a registered preference key
  value     jsonb       NOT NULL,
  updated   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (actor_key, key)
);
COMMENT ON TABLE private.preference IS 'portability: portable';
