-- The accounts partition is hidden from the public role however the log is read (ADR 0013
-- §4, A98). Postgres checks privileges only on the table a query names, so revoking the
-- accounts partition's child table (triplespace_db::grants::grant_partition) does not stop
-- a role that may read the parent `log.record` from reading its rows through it. Row-level
-- security on the parent does: `ts_accounts` reads every row, and the other roles every
-- row outside a partition named `accounts`. The subquery is uncorrelated, so Postgres
-- hashes it once per query; partition pruning is unchanged. The owner, which every service
-- connects as today, bypasses row-level security.
--
-- triplespace_db::grants::POLICIES repeats these, for a database restored onto a server
-- that lacked the roles the policies name.

ALTER TABLE log.record ENABLE ROW LEVEL SECURITY;
CREATE POLICY ts_accounts_reads_all ON log.record TO ts_accounts USING (true) WITH CHECK (true);
CREATE POLICY ts_public_skips_accounts ON log.record TO ts_server, ts_notify, ts_federation, ts_verify USING (partition NOT IN (SELECT p.partition FROM log.partition p WHERE p.name = 'accounts')) WITH CHECK (partition NOT IN (SELECT p.partition FROM log.partition p WHERE p.name = 'accounts'));
