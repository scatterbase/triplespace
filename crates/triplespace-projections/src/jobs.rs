//! `view.job` and `view.job_reject` (step 2; 0011 §6.3; 0013 §5.5) from the `job` log
//! events of payloads.md §5: `start` opens the row, `finish`, `fail` and `revert` close
//! it. A job's rejects travel on its finish record as `params.rejects`, the first N for
//! the job page (0010 §9).

use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};

use crate::backend::PgCx;
use crate::common::{
    PAYLOAD_LOGEVENT, attested, content, offset_db, partition_info, time_of, to_jsonb,
};

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// `view.job` and `view.job_reject`.
#[derive(Debug, Default, Clone, Copy)]
pub struct JobProjection;

/// The tree size a checkpoint note commits to: its second line.
fn checkpoint_size(note: &str) -> Option<i64> {
    note.lines().nth(1)?.trim().parse().ok()
}

fn text(v: &Value, field: &str) -> Option<String> {
    v.get(field).and_then(Value::as_text).map(str::to_owned)
}

/// What every job event carries.
struct Event<'a> {
    tenant: &'a str,
    job_id: i64,
    time: std::time::SystemTime,
    params: &'a Value,
    offset: i64,
}

async fn start(cx: &PgCx, ev: &Event<'_>, record: &Record) -> Result<u64, String> {
    let Event {
        tenant,
        job_id,
        time,
        params,
        offset,
    } = ev;
    let (tenant, job_id, time, params, offset) = (*tenant, *job_id, *time, *params, *offset);
    let actor = attested(record).actor.unwrap_or_default();
    let args = params
        .get("args")
        .map_or_else(|| serde_json::json!({}), to_jsonb);
    cx.conn()
            .execute(
                "INSERT INTO view.job (tenant, job_id, actor_key, requested_by, source, source_version, adapter_version,
                                       mode, graph, params, status, started, start_offset)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'running', $11, $12)
                 ON CONFLICT (tenant, job_id) DO UPDATE SET
                   actor_key = EXCLUDED.actor_key, requested_by = EXCLUDED.requested_by, source = EXCLUDED.source,
                   source_version = EXCLUDED.source_version, adapter_version = EXCLUDED.adapter_version,
                   mode = EXCLUDED.mode, graph = EXCLUDED.graph, params = EXCLUDED.params,
                   started = EXCLUDED.started, start_offset = EXCLUDED.start_offset",
                &[
                    &tenant,
                    &job_id,
                    &actor,
                    &text(params, "requested_by"),
                    &text(params, "source").unwrap_or_default(),
                    &text(params, "version").or_else(|| text(params, "snapshot")),
                    &text(params, "adapter"),
                    &text(params, "mode"),
                    &text(params, "graph").unwrap_or_default(),
                    &args,
                    &time,
                    &offset,
                ],
            )
            .await
            .map_err(|e| sql(&e))
}

async fn close(cx: &PgCx, ev: &Event<'_>, action: &str) -> Result<u64, String> {
    let Event {
        tenant,
        job_id,
        time,
        params,
        offset,
    } = ev;
    let (tenant, job_id, time, params, offset) = (*tenant, *job_id, *time, *params, *offset);
    let status = match action {
        "finish" => "finished",
        "fail" => "failed",
        _ => "reverted",
    };
    let counts = match action {
        "finish" => {
            let mut c = params
                .get("counts")
                .map_or_else(|| serde_json::json!({}), to_jsonb);
            if let (Some(m), Some(obj)) = (params.get("hash_mismatches"), c.as_object_mut()) {
                obj.insert("hash_mismatches".into(), to_jsonb(m));
            }
            c
        }
        _ => to_jsonb(params),
    };
    let checkpoint = text(params, "checkpoint").and_then(|n| checkpoint_size(&n));
    let sweep = params.get("sweep");
    let sweep_count = sweep
        .and_then(|s| s.get("count"))
        .and_then(Value::as_u64)
        .and_then(|n| i32::try_from(n).ok());
    let sweep_threshold = sweep
        .and_then(|s| s.get("threshold"))
        .and_then(Value::as_u64)
        .and_then(|n| i32::try_from(n).ok());
    let mut n = cx
        .conn()
        .execute(
            "UPDATE view.job SET status = $3, finished = $4, counts = $5, checkpoint_size = $6,
                                     sweep_count = $7, sweep_threshold = $8, finish_offset = $9
                 WHERE tenant = $1 AND job_id = $2",
            &[
                &tenant,
                &job_id,
                &status,
                &time,
                &counts,
                &checkpoint,
                &sweep_count,
                &sweep_threshold,
                &offset,
            ],
        )
        .await
        .map_err(|e| sql(&e))?;
    if n == 0 {
        return Err(format!(
            "job {job_id} has no start record before its {action}"
        ));
    }
    if let Some(Value::Array(rejects)) = params.get("rejects") {
        for (i, r) in rejects.iter().enumerate() {
            let line = r
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|l| i64::try_from(l).ok())
                .unwrap_or_else(|| i64::try_from(i).unwrap_or(i64::MAX));
            let match_key = r
                .get("match")
                .map(|m| serde_json::to_string(&to_jsonb(m)).unwrap_or_default());
            let reason = text(r, "reason").unwrap_or_default();
            n += cx
                    .conn()
                    .execute(
                        "INSERT INTO view.job_reject (tenant, job_id, line, match_key, reason) VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT (tenant, job_id, line) DO UPDATE SET match_key = EXCLUDED.match_key, reason = EXCLUDED.reason",
                        &[&tenant, &job_id, &line, &match_key, &reason],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
        }
    }
    Ok(n)
}

impl<B: Backend<Cx = PgCx>> Projection<B> for JobProjection {
    fn name(&self) -> &'static str {
        "job"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_LOGEVENT
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let Some(event) = content(record)? else {
                return Ok(Applied::default());
            };
            if !event.get("type").is_some_and(|t| t.text_eq("job")) {
                return Ok(Applied::default());
            }
            let tenant = partition_info(cx.conn(), record.header().partition)
                .await?
                .tenant;
            let action = event
                .get("action")
                .and_then(Value::as_text)
                .ok_or("a log event has `action`")?;
            let job_id = event
                .get("target")
                .and_then(|t| t.get("id"))
                .and_then(Value::as_u64)
                .ok_or("a job event's target is `{kind: job, id}`")?;
            let job_id = i64::try_from(job_id).map_err(|e| e.to_string())?;
            let time = time_of(
                event
                    .get("time")
                    .and_then(Value::as_u64)
                    .unwrap_or(record.header().appended_at),
            );
            let params = event
                .get("params")
                .cloned()
                .unwrap_or_else(|| Value::map(Vec::new()));
            let offset = offset_db(record)?;
            let n = match action {
                "start" => {
                    start(
                        cx,
                        &Event {
                            tenant: &tenant,
                            job_id,
                            time,
                            params: &params,
                            offset,
                        },
                        record,
                    )
                    .await?
                }
                "finish" | "fail" | "revert" => {
                    close(
                        cx,
                        &Event {
                            tenant: &tenant,
                            job_id,
                            time,
                            params: &params,
                            offset,
                        },
                        action,
                    )
                    .await?
                }
                other => return Err(format!("unknown job action `{other}`")),
            };
            Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = partition_info(cx.conn(), partition).await?.tenant;
            cx.conn()
                .execute("DELETE FROM view.job_reject WHERE tenant = $1", &[&tenant])
                .await
                .map_err(|e| sql(&e))?;
            cx.conn()
                .execute("DELETE FROM view.job WHERE tenant = $1", &[&tenant])
                .await
                .map(|_| ())
                .map_err(|e| sql(&e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_sizes() {
        assert_eq!(
            checkpoint_size("librarybase.org/log/local\n350103\nBASE64ROOT\n\n— sig\n"),
            Some(350_103)
        );
        assert_eq!(checkpoint_size("one line"), None);
    }
}
