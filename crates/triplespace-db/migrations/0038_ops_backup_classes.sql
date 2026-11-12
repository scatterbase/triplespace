-- What a backup does with each `ops` table (ADR 0103 A2, A4): `carried` in the bundle as
-- it is; `sealed`, carried but encrypted with `private`, for operational state that holds
-- personal data; `left` out, for what a restore's migrations or rebuild fill, or what a
-- restored instance starts empty. The class is the table's comment, which
-- `triplespace_db::ops::check` compares with `OPS_TABLES`, so that no `ops` table exists
-- without saying what a backup does with it.

COMMENT ON TABLE ops.delivery IS 'backup: sealed';
COMMENT ON TABLE ops.job_queue IS 'backup: carried';
COMMENT ON TABLE ops.forwarder_key IS 'backup: carried';
COMMENT ON TABLE ops.repo_cursor IS 'backup: carried';
COMMENT ON TABLE ops.visibility_epoch IS 'backup: carried';
COMMENT ON TABLE ops.migration IS 'backup: left';
COMMENT ON TABLE ops.projection_state IS 'backup: left';
COMMENT ON TABLE ops.projection_work IS 'backup: left';
COMMENT ON TABLE ops.purge IS 'backup: left';
