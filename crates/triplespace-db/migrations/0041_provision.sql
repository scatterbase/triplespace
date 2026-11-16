-- The provisioner's role (ADR 0111 §3, A1; 03 §1.2; 08 §1.4). The provisioner creates
-- tenants. A tenant's partitions each have a record table of their own, which only the
-- owner of `log.record` may create, and a tenant has sequences of its own in `log`, which
-- only a role with `CREATE` there may create. `ts_provision` is `ts_server` (a member, so
-- its grants and its row-level policy apply) and may execute two functions the database
-- owner owns: one creates the record table of a partition already registered in
-- `log.partition`, with the options, index and grants
-- `scatter_log_postgres::log::create_partition` and `triplespace_db::grants::grant_partition`
-- give one; the other creates one sequence of a tenant that already has partitions, as
-- `scatter_log_postgres::sequences::create` would. Nothing else.

DO $$
BEGIN
  CREATE ROLE ts_provision NOLOGIN;
EXCEPTION WHEN duplicate_object THEN
  NULL;
END $$;

GRANT ts_server TO ts_provision;

CREATE OR REPLACE FUNCTION log.create_record_table(p bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
  n text;
  t text;
BEGIN
  SELECT name INTO n FROM log.partition WHERE partition = p;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'partition % is not registered', p;
  END IF;
  -- `scatter_log_postgres::ids::record_table`: the partition as 16 hex digits.
  t := 'record_' || lpad(to_hex(p), 16, '0');
  IF to_regclass('log.' || t) IS NOT NULL THEN
    RAISE EXCEPTION 'the record table of partition % exists', p;
  END IF;
  EXECUTE format(
    'CREATE TABLE log.%I PARTITION OF log.record FOR VALUES IN (%s) WITH (autovacuum_vacuum_scale_factor = 0.01, autovacuum_vacuum_insert_scale_factor = 0.01, autovacuum_analyze_scale_factor = 0.005, autovacuum_vacuum_cost_limit = 2000)',
    t, p);
  EXECUTE format('CREATE INDEX ON log.%I (key, "offset" DESC)', t);
  IF n = 'accounts' THEN
    EXECUTE format('REVOKE ALL ON log.%I FROM ts_server, ts_notify, ts_federation, ts_verify', t);
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON log.%I TO ts_accounts', t);
  ELSE
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON log.%I TO ts_server, ts_accounts, ts_notify, ts_federation', t);
    EXECUTE format('GRANT SELECT ON log.%I TO ts_verify', t);
  END IF;
END $$;

CREATE OR REPLACE FUNCTION log.create_sequence(tenant text, kind text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
  -- `scatter_log_postgres::sequences::sequence_name`'s grammar, for a tenant with
  -- partitions: never the instance's sequences, which `instance create` makes.
  IF tenant !~ '^[a-z0-9_-]+$' OR kind !~ '^[a-z0-9_-]+$' OR tenant = 'instance' THEN
    RAISE EXCEPTION '`%` / `%` is not a tenant sequence name', tenant, kind;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM log.partition p WHERE p.tenant = create_sequence.tenant) THEN
    RAISE EXCEPTION 'no tenant `%` has partitions', tenant;
  END IF;
  EXECUTE format('CREATE SEQUENCE IF NOT EXISTS log.%I AS bigint START 1', tenant || '.' || kind);
END $$;

REVOKE ALL ON FUNCTION log.create_record_table(bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION log.create_record_table(bigint) TO ts_provision;
REVOKE ALL ON FUNCTION log.create_sequence(text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION log.create_sequence(text, text) TO ts_provision;
