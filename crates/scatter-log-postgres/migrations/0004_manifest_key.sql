-- One manifest per sealed segment (ADR 0006 A24; 0013 A57): `log.segment_manifest` is keyed
-- `(partition, segment)` and has no `replaces`. A segment's manifest is signed once and never
-- re-signed; where an earlier store kept several for one segment, the newest stays, which is
-- the one it served. Idempotent.

DELETE FROM log.segment_manifest m
 USING log.segment_manifest n
 WHERE m.partition = n.partition AND m.segment = n.segment AND m.sealed_at < n.sealed_at;

ALTER TABLE log.segment_manifest DROP COLUMN IF EXISTS replaces;

DO $$
BEGIN
  IF (SELECT array_agg(a.attname::text ORDER BY a.attname)
        FROM pg_index i
        JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY (i.indkey)
       WHERE i.indrelid = 'log.segment_manifest'::regclass AND i.indisprimary)
     IS DISTINCT FROM ARRAY['partition', 'segment'] THEN
    ALTER TABLE log.segment_manifest DROP CONSTRAINT IF EXISTS segment_manifest_pkey;
    ALTER TABLE log.segment_manifest ADD PRIMARY KEY (partition, segment);
  END IF;
END $$;
