-- Freezing and erasing a tenant (ADR 0111 §10, A3; 0018 §10; 0028 §5; 08 §6.3, §6.6).
-- A frozen tenant's record tables refuse inserts: freezing attaches a row-level trigger
-- to each of the tenant's record tables, and unfreezing drops it, so a tenant that is not
-- frozen pays nothing on its writes. Row-level, because an insert names `log.record` and
-- reaches the partition through it, and Postgres fires a partition's statement-level
-- triggers only for a statement that names the partition itself. Erasing a deleted tenant after its grace period
-- drops its record tables, its log bookkeeping rows and its sequences. Both are DDL, which
-- only the database owner may do on `log.record`'s partitions, so they are functions the
-- owner owns, which `ts_provision` may execute (0111 §3).

CREATE OR REPLACE FUNCTION log.refuse_frozen() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'ts-tenant-frozen: the tenant is frozen; it takes no writes'
    USING ERRCODE = 'TSFRZ';
END $$;

CREATE OR REPLACE FUNCTION log.set_frozen(p bigint, frozen boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
  t text;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM log.partition WHERE partition = p AND tenant IS NOT NULL) THEN
    RAISE EXCEPTION 'partition % is no tenant''s', p;
  END IF;
  t := 'record_' || lpad(to_hex(p), 16, '0');
  EXECUTE format('DROP TRIGGER IF EXISTS ts_frozen ON log.%I', t);
  IF frozen THEN
    EXECUTE format(
      'CREATE TRIGGER ts_frozen BEFORE INSERT ON log.%I FOR EACH ROW EXECUTE FUNCTION log.refuse_frozen()',
      t);
  END IF;
END $$;

CREATE OR REPLACE FUNCTION log.drop_tenant_storage(tenant text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
  p bigint;
  s text;
BEGIN
  IF tenant IS NULL OR tenant = '' OR tenant = 'instance' OR tenant !~ '^[a-z0-9_-]+$' THEN
    RAISE EXCEPTION '`%` is not a tenant', tenant;
  END IF;
  FOR p IN SELECT lp.partition FROM log.partition lp WHERE lp.tenant = drop_tenant_storage.tenant LOOP
    EXECUTE format('DROP TABLE IF EXISTS log.%I', 'record_' || lpad(to_hex(p), 16, '0'));
    DELETE FROM log.merkle_node WHERE partition = p;
    DELETE FROM log.checkpoint WHERE partition = p;
    DELETE FROM log.segment_manifest WHERE partition = p;
    DELETE FROM log.partition WHERE partition = p;
  END LOOP;
  FOR s IN
    SELECT c.relname FROM pg_class c
    WHERE c.relkind = 'S' AND c.relnamespace = 'log'::regnamespace
      AND starts_with(c.relname, drop_tenant_storage.tenant || '.')
  LOOP
    EXECUTE format('DROP SEQUENCE IF EXISTS log.%I', s);
  END LOOP;
END $$;

REVOKE ALL ON FUNCTION log.set_frozen(bigint, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION log.set_frozen(bigint, boolean) TO ts_provision;
REVOKE ALL ON FUNCTION log.drop_tenant_storage(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION log.drop_tenant_storage(text) TO ts_provision;
