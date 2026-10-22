//! The current ACLs along a target's enclosure chain, read from `view.acl` (0016 §4; 0023
//! §2; 0056 §2–§5), for the routes that decide a read with
//! [`scatter_actors::read_decision`] and the writes that refuse a deleted target.
//!
//! - [`page_chain`]: a page, its namespace, the `pages` graph and the tenant; for a talk
//!   page, also its subject when the subject's deletion took the talk page with it.
//! - [`thread_chain`]: a thread, and its home talk page or board, which encloses it
//!   (09 §4.4), with that talk page's own chain.
//! - [`record_acls`]: the `record` ACLs of a set of records, which hide parts of them.
//! - [`removed`]: whether a moderation restriction is in force anywhere on a chain, which
//!   is what makes a thread or page deleted for everyone, its deletion group included,
//!   when it comes to writing.

use std::collections::HashMap;

use scatter_actors::{Acl, ReadKind, Target as AclTarget, Timestamp};
use scatter_log_postgres::PgClient;
use serde_json::Value;

use crate::threads::TARGET_PAGE;

const NS_THREAD_I64: i64 = scatter_threads::NS_THREAD as i64;

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// The current ACLs of these targets, in the order given; targets with none are skipped.
pub async fn current<C: PgClient>(
    client: &C,
    tenant: &str,
    targets: &[AclTarget],
) -> Result<Vec<Acl>, String> {
    let keys: Vec<String> = targets.iter().map(AclTarget::key).collect();
    let rows = client
        .query(
            "SELECT target, restrictions, read_kind, extra FROM view.acl
             WHERE tenant = $1 AND target = ANY($2)",
            &[&tenant, &keys],
        )
        .await
        .map_err(|e| sql(&e))?;
    let mut by_key = HashMap::new();
    for r in rows {
        let key: String = r.get(0);
        let mut v = serde_json::Map::new();
        v.insert("target".into(), Value::String(key.clone()));
        v.insert("restrictions".into(), r.get::<_, Value>(1));
        if let Some(k) = r.get::<_, Option<String>>(2) {
            v.insert("read_kind".into(), Value::String(k));
        }
        if let Some(Value::Object(extra)) = r.get::<_, Option<Value>>(3) {
            v.extend(extra);
        }
        let acl: Acl = serde_json::from_value(Value::Object(v))
            .map_err(|e| format!("view.acl `{key}`: {e}"))?;
        by_key.insert(key, acl);
    }
    Ok(keys.iter().filter_map(|k| by_key.remove(k)).collect())
}

/// Whether a moderation `read` restriction is in force on the chain: the target, or what
/// encloses it, is deleted, hidden or taken down.
#[must_use]
pub fn removed(chain: &[Acl], at: Timestamp) -> bool {
    chain.iter().any(|a| {
        a.restriction_at(scatter_actors::acl::READ, at).is_some()
            && a.read_kind != Some(ReadKind::Confidential)
    })
}

/// The chain of a page: the page, its namespace, the `pages` graph and the tenant; for a
/// talk page, its subject's ACL too when that ACL took the talk page with it (`talk`).
pub async fn page_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: u64,
) -> Result<Vec<Acl>, String> {
    let id = i64::try_from(page_id).map_err(|e| e.to_string())?;
    let ns: Option<i32> = client
        .query_opt(
            "SELECT ns FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
        .map(|r| r.get(0));
    let mut targets = vec![AclTarget::Page(page_id)];
    if let Some(ns) = ns {
        targets.push(AclTarget::Namespace(i64::from(ns)));
    }
    targets.push(AclTarget::Graph("pages".into()));
    targets.push(AclTarget::Tenant(tenant.to_string()));
    let mut chain = current(client, tenant, &targets).await?;
    // A talk page goes with its subject when the subject's deletion said so.
    let subject = client
        .query_opt(
            "SELECT target_id FROM view.talk_page
             WHERE tenant = $1 AND page_id = $2 AND target_kind = $3",
            &[&tenant, &id, &TARGET_PAGE],
        )
        .await
        .map_err(|e| sql(&e))?
        .and_then(|r| r.get::<_, String>(0).parse::<u64>().ok())
        .filter(|s| *s != page_id);
    if let Some(s) = subject {
        chain.extend(
            current(client, tenant, &[AclTarget::Page(s)])
                .await?
                .into_iter()
                .filter(|a| a.talk),
        );
    }
    Ok(chain)
}

/// The chain of a thread: the thread, the `Thread` namespace, then its home talk page or
/// board and that page's chain. `None` when no thread has the page ID.
pub async fn thread_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    thread: u64,
) -> Result<Option<Vec<Acl>>, String> {
    let id = i64::try_from(thread).map_err(|e| e.to_string())?;
    let Some(row) = client
        .query_opt(
            "SELECT talk_page_id FROM view.thread WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
    else {
        return Ok(None);
    };
    let home: Option<i64> = row.get(0);
    let mut chain = current(
        client,
        tenant,
        &[AclTarget::Page(thread), AclTarget::Namespace(NS_THREAD_I64)],
    )
    .await?;
    match home
        .and_then(|h| u64::try_from(h).ok())
        .filter(|h| *h != thread)
    {
        Some(h) => chain.extend(page_chain(client, tenant, h).await?),
        None => chain.extend(
            current(
                client,
                tenant,
                &[
                    AclTarget::Graph("pages".into()),
                    AclTarget::Tenant(tenant.to_string()),
                ],
            )
            .await?,
        ),
    }
    Ok(Some(chain))
}

/// The `record` ACLs of these records of one partition, by offset.
pub async fn record_acls<C: PgClient>(
    client: &C,
    tenant: &str,
    partition: u64,
    offsets: &[u64],
) -> Result<HashMap<u64, Acl>, String> {
    let targets: Vec<AclTarget> = offsets
        .iter()
        .map(|o| AclTarget::Record {
            partition,
            offset: *o,
        })
        .collect();
    Ok(current(client, tenant, &targets)
        .await?
        .into_iter()
        .filter_map(|a| match a.target {
            AclTarget::Record { offset, .. } => Some((offset, a)),
            _ => None,
        })
        .collect())
}
