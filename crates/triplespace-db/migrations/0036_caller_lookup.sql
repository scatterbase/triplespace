-- A request's caller, found without reading `private` (ADR 0013 §4, A101). Every request
-- turns its session cookie or bearer key into an account. These functions do that one
-- lookup as their owner, the role that migrated the database, and return the account and
-- nothing else: the public API's role may call them, and still may not read
-- `private.session` or `private.api_key`. They live in `ops`, which that role may use;
-- `private` it may not even look into.

-- A live session of the tenant, marked as used, with the grants of the bot password it
-- was opened with (0024 §4). A session whose key has been revoked or has expired since
-- is ended, and nothing is returned.
CREATE OR REPLACE FUNCTION ops.caller_session(p_id text, p_tenant text)
RETURNS TABLE (actor_key text, key_id text, expires timestamptz, login bigint, central text,
               key_grants text[])
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
#variable_conflict use_column
DECLARE
  s record;
  k_grants text[];
BEGIN
  UPDATE private.session AS x SET last_seen = now()
   WHERE x.id = p_id AND x.tenant = p_tenant AND x.expires > now()
  RETURNING x.actor_key, x.key_id, x.expires, x.login, x.central INTO s;
  IF NOT FOUND THEN
    RETURN;
  END IF;
  IF s.key_id IS NOT NULL THEN
    SELECT a.grants INTO k_grants FROM private.api_key a
     WHERE a.key_id = s.key_id AND a.revoked_at IS NULL
       AND (a.expires IS NULL OR a.expires > now());
    IF NOT FOUND THEN
      DELETE FROM private.session AS x WHERE x.id = p_id;
      RETURN;
    END IF;
  END IF;
  RETURN QUERY SELECT s.actor_key, s.key_id, s.expires, s.login, s.central, k_grants;
END $$;

-- A live API key by its ID and the SHA-256 of the secret presented, its use noted at most
-- once a minute: the account and the key's grants. The stored hash never leaves.
CREATE OR REPLACE FUNCTION ops.caller_key(p_key_id text, p_hash text)
RETURNS TABLE (actor_key text, grants text[])
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
#variable_conflict use_column
DECLARE
  a_actor text;
  a_grants text[];
BEGIN
  SELECT a.actor_key, a.grants INTO a_actor, a_grants FROM private.api_key a
   WHERE a.key_id = p_key_id AND a.hash = p_hash AND a.revoked_at IS NULL
     AND (a.expires IS NULL OR a.expires > now());
  IF NOT FOUND THEN
    RETURN;
  END IF;
  UPDATE private.api_key AS a SET last_used = now()
   WHERE a.key_id = p_key_id AND (a.last_used IS NULL OR a.last_used < now() - interval '1 minute');
  RETURN QUERY SELECT a_actor, a_grants;
END $$;

REVOKE ALL ON FUNCTION ops.caller_session(text, text), ops.caller_key(text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION ops.caller_session(text, text), ops.caller_key(text, text) TO ts_server, ts_accounts;
