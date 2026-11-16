-- Autovacuum for the view tables that grow with the mirror (0013 A104): on the Wikidata
-- mirror, `identifier` 272M rows, `term` 233M, `entity` and `entity_source` 115M each.
-- Postgres's default waits for a fifth of a table's rows to be dead, 54M on `identifier`.
-- Storage settings only; the tables and their projection are unchanged.
-- Idempotent: setting a storage parameter again changes nothing.

DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['view.identifier', 'view.term', 'view.entity',
                             'view.entity_source', 'view.activity']
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
