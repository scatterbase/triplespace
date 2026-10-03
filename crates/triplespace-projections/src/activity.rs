//! `view.activity` (step 5; 0010 §13; 0012 §3; 0013 §5.5): one row per change-set record
//! and log event in the tenant, mirror and page partitions. It is the time-ordered index
//! history, contributions and recent changes read.
//!
//! The row's `kind` is `edit` for a local or page change set, `sync` for a mirror one,
//! `job` for a job event, `log` for any other event, and `erased` when the content part
//! is gone. The performer is the attestation's `actor` and nowhere else, so an erased
//! attestation leaves `actor_key` null. Patrol, visibility and read groups (0023 §5–6;
//! 0056 §14) are left at their defaults until those projections exist.

use scatter_log::cbor::Value;
use scatter_log::header::{CONFIG_PARTITION, Header};
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_model::id::Subject;

use crate::backend::PgCx;
use crate::common::{
    PAYLOAD_CHANGESET, PAYLOAD_LOGEVENT, attested, comment, content, operation, partition_info,
    target_kind, time_of, to_jsonb,
};

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// `view.activity`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ActivityProjection;

/// The target columns of a row.
#[derive(Debug, Default)]
struct Target {
    kind: Option<i16>,
    id: Option<String>,
    member: Option<String>,
}

/// A row's content-derived columns.
#[derive(Debug, Default)]
struct Columns {
    kind: &'static str,
    target: Target,
    summary_op: Option<String>,
    summary_args: Option<String>,
    new: Option<bool>,
    size: Option<i32>,
    delta: Option<i32>,
    changes: Option<serde_json::Value>,
    log_type: Option<String>,
    log_action: Option<String>,
    params: Option<serde_json::Value>,
}

fn i32_of(n: u64) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A short account of what an `add` or `remove` touched: `claims:P31,P50 labels:en,fr`.
fn summarize(op: &Operation) -> Option<String> {
    let mut parts = Vec::new();
    let list = |name: &str, items: Vec<String>| {
        (!items.is_empty()).then(|| format!("{name}:{}", items.join(",")))
    };
    match op {
        Operation::Add {
            labels,
            descriptions,
            aliases,
            claims,
            sitelinks,
            references,
            qualifiers,
            ..
        } => {
            parts.extend(list(
                "claims",
                claims.keys().map(ToString::to_string).collect(),
            ));
            parts.extend(list("labels", labels.keys().cloned().collect()));
            parts.extend(list("descriptions", descriptions.keys().cloned().collect()));
            parts.extend(list("aliases", aliases.keys().cloned().collect()));
            parts.extend(list("sitelinks", sitelinks.keys().cloned().collect()));
            parts.extend(list(
                "references",
                references.keys().map(ToString::to_string).collect(),
            ));
            parts.extend(list(
                "qualifiers",
                qualifiers.keys().map(ToString::to_string).collect(),
            ));
        }
        Operation::Remove {
            statements,
            labels,
            descriptions,
            aliases,
            sitelinks,
            references,
            qualifiers,
            link,
            ..
        } => {
            parts.extend(list(
                "statements",
                statements.iter().map(ToString::to_string).collect(),
            ));
            parts.extend(list("labels", labels.clone()));
            parts.extend(list("descriptions", descriptions.clone()));
            parts.extend(list("aliases", aliases.keys().cloned().collect()));
            parts.extend(list("sitelinks", sitelinks.clone()));
            parts.extend(list(
                "references",
                references.keys().map(ToString::to_string).collect(),
            ));
            parts.extend(list(
                "qualifiers",
                qualifiers.keys().map(ToString::to_string).collect(),
            ));
            if let Some(l) = link {
                parts.push(format!("link:{}", l.op()));
            }
        }
        Operation::Override {
            statement,
            rank,
            suppress,
            ..
        } => {
            parts.push(statement.to_string());
            if let Some(r) = rank {
                parts.push(format!("rank:{}", serde_json::to_value(r).ok()?.as_str()?));
            }
            if suppress.is_some() {
                parts.push("suppress".to_string());
            }
        }
        Operation::Retain { policy, .. } => {
            parts.push(serde_json::to_value(policy).ok()?.as_str()?.to_string());
        }
        Operation::Redirect { to, .. } => parts.push(format!("to:{to}")),
        Operation::Convert { local, .. } => parts.push(format!("local:{local}")),
        Operation::SameAs { ids, .. }
        | Operation::DifferentFrom { ids, .. }
        | Operation::EquivalentProperty { ids, .. } => {
            parts.push(
                ids.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        _ => {}
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn changeset_columns(record: &Record, mirror: bool) -> Result<Columns, String> {
    let Some(op) = operation(record)? else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let target = match op.subject() {
        Some(Subject::Entity(id)) => Target {
            kind: Some(target_kind::ENTITY),
            id: Some(id.to_string()),
            member: None,
        },
        Some(Subject::Page(p)) => Target {
            kind: Some(target_kind::PAGE),
            id: Some(p.to_string()),
            member: None,
        },
        None => Target::default(),
    };
    let content_len = record
        .body()
        .content()
        .bytes()
        .map(|b| i32_of(b.len() as u64));
    let (size, delta, changes) = match &op {
        Operation::Put {
            size,
            prev_size,
            changes,
            ..
        } => {
            let size = size.map(i32_of).or(content_len);
            let delta = match (size, prev_size) {
                (Some(s), Some(p)) => Some(s.saturating_sub(i32_of(*p))),
                _ => None,
            };
            let changes = changes
                .as_ref()
                .map(|c| serde_json::to_value(c).map_err(|e| e.to_string()))
                .transpose()?;
            (size, delta, changes)
        }
        _ => (content_len, None, None),
    };
    Ok(Columns {
        kind: if mirror { "sync" } else { "edit" },
        target,
        summary_op: Some(op.name().to_string()),
        summary_args: summarize(&op),
        new: Some(matches!(
            op,
            Operation::Create { .. } | Operation::Adopt { .. }
        )),
        size,
        delta,
        changes,
        ..Columns::default()
    })
}

fn logevent_columns(record: &Record) -> Result<Columns, String> {
    let Some(event) = content(record)? else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let log_type = event
        .get("type")
        .and_then(Value::as_text)
        .map(str::to_owned);
    let log_action = event
        .get("action")
        .and_then(Value::as_text)
        .map(str::to_owned);
    let target = match event.get("target") {
        Some(t) => {
            let kind = t.get("kind").and_then(Value::as_text);
            match kind {
                Some("entity") => Target {
                    kind: Some(target_kind::ENTITY),
                    id: t.get("id").and_then(Value::as_text).map(str::to_owned),
                    member: None,
                },
                Some("page") => Target {
                    kind: Some(target_kind::PAGE),
                    id: t.get("id").and_then(Value::as_u64).map(|n| n.to_string()),
                    member: None,
                },
                Some("actor") => Target {
                    kind: Some(target_kind::ACTOR),
                    id: t.get("key").and_then(Value::as_text).map(str::to_owned),
                    member: None,
                },
                Some("job") => Target {
                    kind: Some(target_kind::JOB),
                    id: t.get("id").and_then(Value::as_u64).map(|n| n.to_string()),
                    member: None,
                },
                Some("record") => Target {
                    kind: Some(target_kind::RECORD),
                    id: match (
                        t.get("partition").and_then(Value::as_u64),
                        t.get("offset").and_then(Value::as_u64),
                    ) {
                        (Some(p), Some(o)) => Some(format!("{p}:{o}")),
                        _ => None,
                    },
                    member: t
                        .get("revid")
                        .and_then(Value::as_u64)
                        .map(|r| r.to_string()),
                },
                _ => Target::default(),
            }
        }
        None => Target::default(),
    };
    Ok(Columns {
        kind: if log_type.as_deref() == Some("job") {
            "job"
        } else {
            "log"
        },
        target,
        log_type,
        log_action,
        params: event.get("params").map(to_jsonb),
        ..Columns::default()
    })
}

impl<B: Backend<Cx = PgCx>> Projection<B> for ActivityProjection {
    fn name(&self) -> &'static str {
        "activity"
    }

    fn step(&self) -> Step {
        Step::Activity
    }

    fn accepts(&self, header: &Header) -> bool {
        header.partition != CONFIG_PARTITION
            && (header.payload_type == PAYLOAD_CHANGESET || header.payload_type == PAYLOAD_LOGEVENT)
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx.conn(), h.partition).await?;
            let columns = if h.payload_type == PAYLOAD_CHANGESET {
                changeset_columns(record, info.is_mirror())?
            } else {
                logevent_columns(record)?
            };
            let attested = attested(record);
            let as_i64 = |n: Option<u64>| {
                n.map(i64::try_from)
                    .transpose()
                    .map_err(|e: std::num::TryFromIntError| e.to_string())
            };
            let n = cx
                .conn()
                .execute(
                    "INSERT INTO view.activity
                       (tenant, time, partition, \"offset\", kind, source, revid, logid, target_kind, target_id, target_member,
                        actor_key, job_id, summary_op, summary_args, comment, tags, new, size, delta,
                        log_type, log_action, params, changes)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)
                     ON CONFLICT (tenant, partition, \"offset\") DO UPDATE SET
                       kind = EXCLUDED.kind, target_kind = EXCLUDED.target_kind, target_id = EXCLUDED.target_id,
                       target_member = EXCLUDED.target_member, actor_key = EXCLUDED.actor_key, job_id = EXCLUDED.job_id,
                       summary_op = EXCLUDED.summary_op, summary_args = EXCLUDED.summary_args, comment = EXCLUDED.comment,
                       tags = EXCLUDED.tags, new = EXCLUDED.new, size = EXCLUDED.size, delta = EXCLUDED.delta,
                       log_type = EXCLUDED.log_type, log_action = EXCLUDED.log_action, params = EXCLUDED.params,
                       changes = EXCLUDED.changes",
                    &[
                        &info.tenant,
                        &time_of(h.appended_at),
                        &scatter_log_postgres::ids::partition_to_db(h.partition),
                        &as_i64(Some(h.offset))?,
                        &columns.kind,
                        &info.name,
                        &as_i64(h.revid)?,
                        &as_i64(h.logid)?,
                        &columns.target.kind,
                        &columns.target.id,
                        &columns.target.member,
                        &attested.actor,
                        &as_i64(attested.job)?,
                        &columns.summary_op,
                        &columns.summary_args,
                        &comment(record),
                        &attested.tags,
                        &columns.new,
                        &columns.size,
                        &columns.delta,
                        &columns.log_type,
                        &columns.log_action,
                        &columns.params,
                        &columns.changes,
                    ],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx.conn(), partition).await?;
            cx.conn()
                .execute(
                    "DELETE FROM view.activity WHERE tenant = $1 AND partition = $2",
                    &[
                        &info.tenant,
                        &scatter_log_postgres::ids::partition_to_db(partition),
                    ],
                )
                .await
                .map(|_| ())
                .map_err(|e| sql(&e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summaries() {
        let op: Operation = serde_json::from_value(json!({"op": "add", "id": "Q1",
            "labels": {"en": "x", "fr": "y"},
            "claims": {"P31": [{"mainsnak": {"snaktype": "somevalue", "property": "P31"}, "type": "statement", "rank": "normal"}]}}))
        .unwrap();
        assert_eq!(summarize(&op).as_deref(), Some("claims:P31 labels:en,fr"));
        let op: Operation = serde_json::from_value(json!({"op": "override", "id": "OAW1",
            "statement": "OAW1$2F1C0000-0000-0000-0000-000000000000", "rank": "deprecated"}))
        .unwrap();
        assert_eq!(
            summarize(&op).as_deref(),
            Some("OAW1$2F1C0000-0000-0000-0000-000000000000 rank:deprecated")
        );
        let op: Operation =
            serde_json::from_value(json!({"op": "retain", "id": "WDQ1", "policy": "orphan"}))
                .unwrap();
        assert_eq!(summarize(&op).as_deref(), Some("orphan"));
        let op: Operation = serde_json::from_value(
            json!({"op": "tombstone", "id": "WDQ1", "upstream": {"revid": 1}}),
        )
        .unwrap();
        assert_eq!(summarize(&op), None);
    }
}
