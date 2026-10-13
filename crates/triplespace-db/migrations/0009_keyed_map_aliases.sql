-- view.keyed_map holds merge aliases beside each key's owner (0009 §9, A13): an upstream
-- item merged into a mapped item is recorded as an alias of its key. One owner per key
-- stays unique; aliases are not. Idempotent: every statement tolerates having run before.

ALTER TABLE view.keyed_map ADD COLUMN IF NOT EXISTS alias boolean NOT NULL DEFAULT false;

ALTER TABLE view.keyed_map DROP CONSTRAINT IF EXISTS keyed_map_tenant_keyed_type_key_provider_key;

CREATE UNIQUE INDEX IF NOT EXISTS keyed_map_owner
  ON view.keyed_map (tenant, keyed_type, key, provider) WHERE NOT alias;

-- The projection replaces a key's rows (owner and aliases) on every put under the key.
CREATE INDEX IF NOT EXISTS keyed_map_by_key
  ON view.keyed_map (tenant, provider, keyed_type, key);
