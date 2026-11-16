-- A batch's hash on its job row (ADR 0104 §1, §8; 03 §4.1): the first 20 bytes of
-- SHA-256 over the tenant, the operator and the submitter's value, in z-base-32, which a
-- write naming the value looks up to join the open batch or be refused by a rejected
-- one. `status` gains `rejected` and `closed` (0104 §1, §2). Idempotent.

ALTER TABLE view.job ADD COLUMN IF NOT EXISTS batch text;
CREATE INDEX IF NOT EXISTS job_batch ON view.job (tenant, batch) WHERE batch IS NOT NULL;
