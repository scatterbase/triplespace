-- Roles and grants (ADR 0013 §4; 0056). Privacy is enforced by grants: the role the
-- public API connects with has no privilege on `private`. These are NOLOGIN group roles;
-- a deployment grants them to its login roles. Roles are cluster-wide, so creation
-- tolerates another database having made them already; grants are per database and run
-- every time.
--
--   ts_server      the server and the ingester: log, view, ops
--   ts_accounts    triplespace-accounts: private (all), and what the server has
--   ts_notify      triplespace-notify: private (inbox, contacts), and what the server has
--   ts_federation  triplespace-federation: private (ap_key, ap_follower only), and what the server has
--   ts_verify      verify: log, read-only

DO $$
DECLARE
  r text;
BEGIN
  FOREACH r IN ARRAY ARRAY['ts_server', 'ts_accounts', 'ts_notify', 'ts_federation', 'ts_verify'] LOOP
    BEGIN
      EXECUTE format('CREATE ROLE %I NOLOGIN', r);
    EXCEPTION WHEN duplicate_object THEN
      NULL;
    END;
  END LOOP;
END $$;

GRANT USAGE ON SCHEMA log, view, ops TO ts_server, ts_accounts, ts_notify, ts_federation;
GRANT USAGE ON SCHEMA log TO ts_verify;
GRANT USAGE ON SCHEMA private TO ts_accounts, ts_notify, ts_federation;

-- The server reads and writes the log, the views and ops. Per child table of log.record,
-- so that the accounts partition's child can be excluded (0013 §4): see
-- triplespace_db::grants::grant_partition.
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA log, view, ops TO ts_server, ts_accounts, ts_notify, ts_federation;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA log, view, ops TO ts_server, ts_accounts, ts_notify, ts_federation;
GRANT SELECT ON ALL TABLES IN SCHEMA log TO ts_verify;

-- Private: the accounts service everything; the notifier and federation their tables,
-- granted as those tables are added.
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA private TO ts_accounts;

-- Tables added later inherit the same grants.
ALTER DEFAULT PRIVILEGES IN SCHEMA log, view, ops GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ts_server, ts_accounts, ts_notify, ts_federation;
ALTER DEFAULT PRIVILEGES IN SCHEMA log, view, ops GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO ts_server, ts_accounts, ts_notify, ts_federation;
ALTER DEFAULT PRIVILEGES IN SCHEMA log GRANT SELECT ON TABLES TO ts_verify;
ALTER DEFAULT PRIVILEGES IN SCHEMA private GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ts_accounts;
