-- Autovacuum for the record partitions and the merkle nodes, which run to hundreds of
-- millions of rows (0013 A104). Postgres's defaults vacuum a table once a fifth of its
-- rows are dead or newly inserted, which on a 122M-record mirror partition is 24M rows,
-- weeks of a follower's compaction, while the space compaction frees stays unusable.
-- A partition made after this migration gets the same settings from `create_partition`.
-- Idempotent: setting a storage parameter again changes nothing.

DO $$
DECLARE
    t regclass;
BEGIN
    FOR t IN
        SELECT inhrelid::regclass FROM pg_inherits WHERE inhparent = 'log.record'::regclass
        UNION ALL
        SELECT 'log.merkle_node'::regclass
    LOOP
        EXECUTE format(
            'ALTER TABLE %s SET (autovacuum_vacuum_scale_factor = 0.01, '
            'autovacuum_vacuum_insert_scale_factor = 0.01, '
            'autovacuum_analyze_scale_factor = 0.005, '
            'autovacuum_vacuum_cost_limit = 2000)',
            t
        );
    END LOOP;
END
$$;
