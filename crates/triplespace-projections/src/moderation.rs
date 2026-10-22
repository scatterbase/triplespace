//! What an ACL does to the views (0023 §4–§5; 09 §6.2–§6.3), after [`crate::actors::AclProjection`]
//! has written `view.acl`:
//!
//! - **A page or thread** with a moderation `read` restriction is deleted:
//!   `view.page.deleted` is set, so it leaves the title index, listings and search, and its
//!   title can be taken. With `talk: true` its talk page goes with it. Retiring the ACL
//!   clears both, the talk page only where no ACL of its own still deletes it.
//! - **A local entity** likewise sets `view.entity.deleted`.
//! - **A record** with a `record` ACL gets the visibility bits of 0023 §5 on its
//!   `view.activity` row: content (or a thread's or page's `text`), comment, attestation,
//!   and suppressed when the restriction names the suppression group. Retiring the ACL
//!   clears them.
//!
//! The log event each ACL record projects as is [`crate::activity`]'s. The cache purges and
//! search re-sends of 09 §6.3 wait for those subsystems.

use scatter_actors::acl::{Acl, ReadKind, Target};
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};

use crate::actors::payload;
use crate::backend::PgCx;
use crate::common::{content, tenant_of, to_jsonb};

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// The suppression group the bits name as suppressed (`moderation.suppress_group`'s
/// default; a tenant that renames it is not read yet).
const SUPPRESS: &str = "suppress";

/// The visibility bits, as MediaWiki numbers them.
mod bits {
    pub const CONTENT: i16 = 1;
    pub const COMMENT: i16 = 2;
    pub const USER: i16 = 4;
    pub const SUPPRESSED: i16 = 8;
}

/// The bits a `record` ACL sets.
#[must_use]
pub fn visibility_bits(acl: &Acl) -> i16 {
    let Some(read) = acl.restrictions.get(scatter_actors::acl::READ) else {
        return 0;
    };
    let mut v = 0;
    if acl.parts.is_empty() {
        v = bits::CONTENT | bits::COMMENT | bits::USER;
    }
    for p in &acl.parts {
        v |= match p.as_str() {
            "content" | "text" => bits::CONTENT,
            "comment" => bits::COMMENT,
            "attestation" => bits::USER,
            _ => 0,
        };
    }
    if read.group.as_deref() == Some(SUPPRESS) || read.group.is_none() {
        v |= bits::SUPPRESSED;
    }
    v
}

fn deletes(acl: &Acl) -> bool {
    acl.restrictions.contains_key(scatter_actors::acl::READ)
        && acl.read_kind == Some(ReadKind::Moderation)
}

/// The views an ACL changes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ModerationProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for ModerationProjection {
    fn name(&self) -> &'static str {
        "moderation"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::ACL
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx, h.partition).await?;
            let key = h
                .key
                .as_deref()
                .ok_or("an acl record is keyed by its target")?;
            let target = Target::parse(key).map_err(|e| e.to_string())?;
            let acl: Option<Acl> = match content(record)? {
                Some(v) => Some(serde_json::from_value(to_jsonb(&v)).map_err(|e| e.to_string())?),
                None => None,
            };
            let rows = match &target {
                Target::Page(id) => page(cx, &tenant, *id, acl.as_ref()).await?,
                Target::Entity(id) => {
                    let deleted = acl.as_ref().is_some_and(deletes);
                    cx.execute(
                        "UPDATE view.entity SET deleted = $3 WHERE tenant = $1 AND id = $2",
                        &[&tenant, id, &deleted],
                    )
                    .await
                    .map_err(|e| sql(&e))?
                }
                Target::Record { partition, offset } => {
                    let v = acl.as_ref().map_or(0, visibility_bits);
                    let p = scatter_log_postgres::ids::partition_to_db(*partition);
                    let o = i64::try_from(*offset).map_err(|e| e.to_string())?;
                    cx.execute(
                        "UPDATE view.activity SET visibility = $4
                         WHERE tenant = $1 AND partition = $2 AND \"offset\" = $3",
                        &[&tenant, &p, &o, &v],
                    )
                    .await
                    .map_err(|e| sql(&e))?
                }
                _ => 0,
            };
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, _: &'a mut PgCx, _: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

async fn page(cx: &PgCx, tenant: &str, id: u64, acl: Option<&Acl>) -> Result<u64, String> {
    let page = i64::try_from(id).map_err(|e| e.to_string())?;
    let deleted = acl.is_some_and(deletes);
    let mut rows = cx
        .execute(
            "UPDATE view.page SET deleted = $3 WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &page, &deleted],
        )
        .await
        .map_err(|e| sql(&e))?;
    // The talk page goes with it, or comes back unless its own ACL deletes it.
    let talk = cx
        .query_opt(
            "SELECT page_id FROM view.talk_page WHERE tenant = $1 AND target_kind = 1 AND target_id = $2",
            &[&tenant, &id.to_string()],
        )
        .await
        .map_err(|e| sql(&e))?
        .map(|r| r.get::<_, i64>(0))
        .filter(|t| *t != page);
    if let Some(t) = talk {
        let talk_deleted = if deleted {
            acl.is_some_and(|a| a.talk)
        } else {
            let own = crate::acl_read::current(
                cx,
                tenant,
                &[Target::Page(u64::try_from(t).map_err(|e| e.to_string())?)],
            )
            .await?;
            own.iter().any(deletes)
        };
        if deleted && !talk_deleted {
            return Ok(rows);
        }
        rows += cx
            .execute(
                "UPDATE view.page SET deleted = $3 WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &t, &talk_deleted],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(rows)
}
