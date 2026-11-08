-- The job queue (ADR 0104 §4, §8; 03 §4.12; 16 §2.2): a submitted job waiting for the
-- runner, keyed by the job ID minted at submission, so that the API answers with the ID
-- and `Special:Jobs/{id}` shows the job `queued` before its start record exists.
--
-- A runner task claims a row under `SELECT … FOR UPDATE SKIP LOCKED`, one running job per
-- partition at a time, and keeps `heartbeat` fresh while it runs; a claimed row whose
-- heartbeat is stale was a runner that died, and the next claim marks its job
-- `job/interrupt` and runs it again under a new job ID (`attempts` counts the claims).
-- The row is deleted when the job's finish or fail record is written. `payload` holds an
-- uploaded file for a job that takes one (an import's XML). Idempotent: every statement
-- tolerates having run before.

CREATE TABLE IF NOT EXISTS ops.job_queue (
  tenant      text        NOT NULL DEFAULT '',   -- '' for an instance job
  job_id      bigint      NOT NULL,              -- minted at submission (0013 §6)
  partition   bigint      NOT NULL,              -- the log partition the job's records go to
  kind        text        NOT NULL,              -- import | export | dump | revert | sync | adopt | follow
  params      jsonb       NOT NULL,              -- what the job needs, by kind
  payload     bytea,                             -- an uploaded file, where the kind takes one
  actor_key   text        NOT NULL,              -- who submitted it: the job's actor
  submitted   timestamptz NOT NULL DEFAULT now(),
  claimed_by  text,                              -- the runner task's name
  claimed_at  timestamptz,
  heartbeat   timestamptz,
  attempts    integer     NOT NULL DEFAULT 0,
  progress    jsonb,                             -- the running job's progress, for its page
  cancel      boolean     NOT NULL DEFAULT false,
  PRIMARY KEY (tenant, job_id)
);
CREATE INDEX IF NOT EXISTS job_queue_order ON ops.job_queue (submitted, job_id);
CREATE INDEX IF NOT EXISTS job_queue_partition ON ops.job_queue (partition) WHERE claimed_at IS NOT NULL;
