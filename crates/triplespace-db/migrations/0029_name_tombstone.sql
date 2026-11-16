-- Name tombstones (ADR 0010 §10, A6, A52; 0102 §3; 03 §4.16; 19 §2.3).
--
-- Every name a local account has released, by rename or vanish, stays reserved so that
-- no name is ever reused: `[[User:OldName]]` in old text can never come to name a
-- different person. A vanished account's names are erased from the log, so the tombstone
-- is not a projection: `name_hmac` is a keyed hash of the tenant and the lower-cased name
-- under the instance secret, holding no actor key and no plaintext. `issuer` is the OAuth
-- issuer whose username the released name was derived from under strict names (0102 §3),
-- so that the tombstone can yield to that issuer attesting the name again; NULL for a
-- name of any other origin, which holds. Never leaves the instance. Idempotent.

CREATE TABLE IF NOT EXISTS private.name_tombstone (
  name_hmac text        NOT NULL PRIMARY KEY,
  released  timestamptz NOT NULL DEFAULT now(),
  issuer    text
);
COMMENT ON TABLE private.name_tombstone IS 'portability: never-leaves';
