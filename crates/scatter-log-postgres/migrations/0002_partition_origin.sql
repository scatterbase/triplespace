-- The origin line of a partition's checkpoints and segment manifests, fixed when the
-- partition is created from the host it is created under, so that an `alias` of the
-- tenant afterwards does not change it (01 §4.2; 0006 A29). Written by `create_partition`.
-- Idempotent: every statement tolerates having run before.

ALTER TABLE log.partition ADD COLUMN IF NOT EXISTS origin text;

-- A partition created before this column existed had its origin fixed by its first
-- checkpoint or segment manifest (0006 A29): the first line of that note. One with neither
-- stays NULL here; `triplespace-db`'s migration of the same name derives it from the
-- current host, which is the best that can be done.
UPDATE log.partition p
   SET origin = split_part(c.signed_note, E'\n', 1)
  FROM (SELECT DISTINCT ON (partition) partition, signed_note
          FROM log.checkpoint ORDER BY partition, tree_size) c
 WHERE c.partition = p.partition AND p.origin IS NULL;

UPDATE log.partition p
   SET origin = regexp_replace(split_part(m.signed_note, E'\n', 1), '/segment/[0-9]+$', '')
  FROM (SELECT DISTINCT ON (partition) partition, signed_note
          FROM log.segment_manifest ORDER BY partition, segment, sealed_at) m
 WHERE m.partition = p.partition AND p.origin IS NULL;
