-- Access controls (0056 A19; 0023 A15).
--
-- The hidden set of 0056 A19 reads a tenant's confidential `read` restrictions on every
-- read request: those whose kind is confidential, and those a deletion keeps beside its
-- own group in `extra.confidential`. The partial index keeps that read to those rows,
-- never the tenant's deletions and protections, of which a large wiki has millions.
CREATE INDEX IF NOT EXISTS acl_confidential ON view.acl (tenant)
  WHERE read_kind = 'confidential' OR extra ? 'confidential';

-- Feed and log rows never hold a group list (0056 A14, A19): what a principal may see is
-- decided at serve time, so this column, which nothing filled, goes.
ALTER TABLE view.activity DROP COLUMN IF EXISTS read_groups;
