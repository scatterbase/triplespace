//! Document pages (step 2; 10 §3.2; 03 §4.5, §4.7; 0051 §5; 0083 §4, §6): what a
//! `scatter:v0/page` record in a tenant's `pages` partition projects to.
//!
//! - **`view.page`**: the page's title, model and latest record, its length, time and
//!   revision count, and whether its latest text is a redirect.
//! - **`view.entity_revision`**: the text after every revision, keyed by the page ID in
//!   decimal (03 §4.4), so `prop=revisions` at any `oldid` reads one row (18 §2.4). A
//!   `move` stores the text it kept; a revision whose source withheld the text has no row.
//! - **`view.redirect`**: the target of a redirect's latest text, resolved to a page ID
//!   where one exists, and kept resolved as pages are created and moved.
//! - **`view.page_link`** and **`view.page_category`**: the rows the stored text makes,
//!   with `from_render = false`; the rows expanded text adds are the refresh job's
//!   (0083 §6). Links to titles are recorded by namespace and title (`6`, `{ns}:{title}`),
//!   so a link keeps its row when its target is created, moved or deleted; links into an
//!   entity namespace by entity ID (`1`); interwiki links are not rows.
//!
//! Links and categories are read with `scatter-wikitext`'s extraction from the stored
//! text, over the namespace registry. A tenant's local Project name (its site name,
//! 0008 A37) is not yet one of the names it reads.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_pages::{PAYLOAD_PAGE, PageOp, TEXT_PART};
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_titles::Namespaces;
use scatter_wikitext::{Config, ConfigSource, Namespace, Title};

use crate::backend::PgCx;
use crate::common::{content, partition_info, time_of, to_jsonb};
use scatter_log_postgres::PgClient;

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// The `target_kind` of a link row naming a title as `{ns}:{title}`.
pub const TARGET_TITLE: i16 = crate::common::target_kind::TITLE;

/// The wikitext configuration the registry's namespaces give, with their canonical names.
pub fn wikitext_config() -> &'static Config {
    static C: OnceLock<Config> = OnceLock::new();
    C.get_or_init(|| Config::new(config_source(None)))
}

/// The wikitext configuration source over the namespace registry, with a site name as
/// the Project namespace's local name where it is given.
#[must_use]
pub fn config_source(site_name: Option<&str>) -> ConfigSource {
    let namespaces = Namespaces::embedded()
        .iter()
        .map(|n| {
            let mut ns = Namespace::new(n.id, &n.canonical);
            ns.case_sensitive = n.case.as_deref() == Some("case-sensitive");
            ns.subpages = n.subpages;
            if n.id == 6 {
                ns = ns.alias("Image");
            }
            if n.id == 7 {
                ns = ns.alias("Image talk");
            }
            if n.id == scatter_titles::PROJECT
                && let Some(site) = site_name.filter(|s| *s != n.canonical)
            {
                ns.name = site.to_string();
                ns = ns.canonical(&n.canonical);
            }
            ns
        })
        .collect();
    ConfigSource {
        namespaces,
        ..ConfigSource::default()
    }
}

/// What a page record says, read once.
pub struct PageRecord {
    /// The operation; `None` when the content part is erased.
    pub op: Option<PageOp>,
    /// The text part, where the record carries one.
    pub text: Option<String>,
}

impl PageRecord {
    /// Reads a page record.
    pub fn read(record: &Record) -> Result<Self, String> {
        let op = match content(record)? {
            Some(v) => Some(PageOp::from_json(to_jsonb(&v))?),
            None => None,
        };
        let text = record
            .body()
            .part(TEXT_PART)
            .and_then(|p| p.value().ok().flatten())
            .and_then(|v| match v {
                Value::Text(t) => Some(t),
                _ => None,
            });
        Ok(Self { op, text })
    }
}

/// The time a page record shows: an adopted revision's source time, else the append time.
#[must_use]
pub fn shown_time(header: &Header, op: &PageOp) -> u64 {
    op.source()
        .filter(|s| header.revid == Some(s.source_revid))
        .and_then(|s| crate::feed::parse_timestamp(&s.source_time))
        .unwrap_or(header.appended_at)
}

/// The `view` rows of document pages.
#[derive(Debug, Default, Clone, Copy)]
pub struct PageProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for PageProjection {
    fn name(&self) -> &'static str {
        "page"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_PAGE
    }

    #[allow(clippy::too_many_lines)]
    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx, h.partition).await?;
            let page = PageRecord::read(record)?;
            let Some(op) = page.op else {
                return Ok(Applied::rows(0));
            };
            let page_id = h
                .page_id
                .and_then(|p| i64::try_from(p).ok())
                .ok_or("a page record carries its page ID")?;
            let offset = i64::try_from(h.offset).map_err(|e| e.to_string())?;
            let revid = h.revid.and_then(|r| i64::try_from(r).ok());
            let at = time_of(shown_time(h, &op));
            let tenant = info.tenant.as_str();
            let len = page
                .text
                .as_ref()
                .map(|t| i32::try_from(t.len()).unwrap_or(i32::MAX));
            let mut rows = 0u64;

            // The title the latest text is read as, after this record.
            let (ns, title) = match &op {
                PageOp::Create {
                    ns, title, model, ..
                } => {
                    rows += cx
                        .execute(
                            "INSERT INTO view.page (tenant, page_id, ns, title, content_model, latest_offset,
                               latest_revid, len, latest_at, revisions, reserved)
                             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1, false)
                             ON CONFLICT (tenant, page_id) DO UPDATE SET
                               ns = EXCLUDED.ns, title = EXCLUDED.title, content_model = EXCLUDED.content_model,
                               latest_offset = EXCLUDED.latest_offset, latest_revid = EXCLUDED.latest_revid,
                               len = EXCLUDED.len, latest_at = EXCLUDED.latest_at, revisions = 1, reserved = false",
                            &[&tenant, &page_id, ns, title, model, &offset, &revid, &len, &at],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    (*ns, title.clone())
                }
                PageOp::Edit { model, .. } => {
                    let n = cx
                        .execute(
                            "UPDATE view.page SET latest_offset = $3, latest_revid = $4,
                               len = COALESCE($5, len), latest_at = $6,
                               content_model = COALESCE($7, content_model), revisions = revisions + 1
                             WHERE tenant = $1 AND page_id = $2 AND (latest_offset IS NULL OR latest_offset < $3)",
                            &[&tenant, &page_id, &offset, &revid, &len, &at, model],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    if n == 0 {
                        return Ok(Applied::rows(0));
                    }
                    rows += n;
                    current_title(cx, tenant, page_id).await?
                }
                PageOp::Move { ns, title } => {
                    let n = cx
                        .execute(
                            "UPDATE view.page SET ns = $3, title = $4, latest_offset = $5, latest_revid = $6,
                               latest_at = $7, revisions = revisions + 1
                             WHERE tenant = $1 AND page_id = $2 AND (latest_offset IS NULL OR latest_offset < $5)",
                            &[&tenant, &page_id, ns, title, &offset, &revid, &at],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    if n == 0 {
                        return Ok(Applied::rows(0));
                    }
                    rows += n;
                    // Redirects that named the page's old title no longer reach it.
                    cx.execute(
                        "UPDATE view.redirect SET target_page_id = NULL
                         WHERE tenant = $1 AND target_page_id = $2 AND NOT (target_ns = $3 AND target_title = $4)",
                        &[&tenant, &page_id, ns, title],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
                    (*ns, title.clone())
                }
            };

            // The text after the revision: its own, or for a move the text it kept.
            let text = match (&op, page.text) {
                (PageOp::Move { .. }, _) => latest_text(cx, tenant, page_id).await?,
                (_, t) => t,
            };
            if let (Some(text), Some(revid)) = (&text, revid) {
                rows += cx
                    .execute(
                        "INSERT INTO view.entity_revision (tenant, revid, entity_id, partition, \"offset\", state)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (tenant, revid) DO NOTHING",
                        &[
                            &tenant,
                            &revid,
                            &page_id.to_string(),
                            &scatter_log_postgres::ids::partition_to_db(h.partition),
                            &offset,
                            &text.as_bytes(),
                        ],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }

            // A page that now holds a title resolves the redirects that name it.
            if matches!(op, PageOp::Create { .. } | PageOp::Move { .. }) {
                cx.execute(
                    "UPDATE view.redirect SET target_page_id = $2
                     WHERE tenant = $1 AND target_ns = $3 AND target_title = $4 AND interwiki IS NULL",
                    &[&tenant, &page_id, &ns, &title],
                )
                .await
                .map_err(|e| sql(&e))?;
            }

            if let Some(text) = &text {
                rows += index_text(cx, tenant, page_id, ns, &title, text).await?;
            }
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(usize::MAX)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            let p = scatter_log_postgres::ids::partition_to_db(partition);
            for q in [
                "DELETE FROM view.page_link WHERE tenant = $1 AND NOT from_render",
                "DELETE FROM view.page_category WHERE tenant = $1 AND NOT from_render",
                "DELETE FROM view.redirect WHERE tenant = $1",
                "DELETE FROM view.page WHERE tenant = $1 AND NOT reserved",
            ] {
                cx.execute(q, &[&info.tenant]).await.map_err(|e| sql(&e))?;
            }
            cx.execute(
                "DELETE FROM view.entity_revision e WHERE e.tenant = $1 AND e.partition = $2",
                &[&info.tenant, &p],
            )
            .await
            .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}

async fn current_title(cx: &PgCx, tenant: &str, page_id: i64) -> Result<(i32, String), String> {
    let row = cx
        .query_one(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &page_id],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok((row.get(0), row.get(1)))
}

/// The page's newest stored text.
pub async fn latest_text<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: i64,
) -> Result<Option<String>, String> {
    let row = client
        .query_opt(
            "SELECT state FROM view.entity_revision WHERE tenant = $1 AND entity_id = $2
             ORDER BY \"offset\" DESC LIMIT 1",
            &[&tenant, &page_id.to_string()],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.map(|r| String::from_utf8_lossy(&r.get::<_, Vec<u8>>(0)).into_owned()))
}

/// The text of a page after a revision.
pub async fn text_at<C: PgClient>(
    client: &C,
    tenant: &str,
    revid: u64,
) -> Result<Option<(String, String)>, String> {
    let row = client
        .query_opt(
            "SELECT entity_id, state FROM view.entity_revision WHERE tenant = $1 AND revid = $2",
            &[&tenant, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.map(|r| {
        (
            r.get::<_, String>(0),
            String::from_utf8_lossy(&r.get::<_, Vec<u8>>(1)).into_owned(),
        )
    }))
}

/// Rewrites the rows the page's latest text makes: its redirect, links and categories.
async fn index_text(
    cx: &PgCx,
    tenant: &str,
    page_id: i64,
    ns: i32,
    title: &str,
    text: &str,
) -> Result<u64, String> {
    let config = wikitext_config();
    let page = Title::new(ns, title);
    let ex = scatter_wikitext::extract(text, config, &page);
    let namespaces = Namespaces::embedded();
    let mut rows = 0u64;

    cx.execute(
        "UPDATE view.page SET is_redirect = $3 WHERE tenant = $1 AND page_id = $2",
        &[&tenant, &page_id, &ex.redirect.is_some()],
    )
    .await
    .map_err(|e| sql(&e))?;
    match &ex.redirect {
        Some(t) => {
            let target_ns = t.interwiki.is_none().then_some(t.namespace);
            rows += cx
                .execute(
                    "INSERT INTO view.redirect (tenant, page_id, target_ns, target_title, fragment, interwiki, target_page_id)
                     VALUES ($1, $2, $3, $4, $5, $6,
                       (SELECT page_id FROM view.page p WHERE p.tenant = $1 AND p.ns = $3 AND p.title = $4 AND NOT p.deleted))
                     ON CONFLICT (tenant, page_id) DO UPDATE SET target_ns = EXCLUDED.target_ns,
                       target_title = EXCLUDED.target_title, fragment = EXCLUDED.fragment,
                       interwiki = EXCLUDED.interwiki, target_page_id = EXCLUDED.target_page_id",
                    &[&tenant, &page_id, &target_ns, &t.text, &t.fragment, &t.interwiki],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        None => {
            cx.execute(
                "DELETE FROM view.redirect WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &page_id],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
    }

    let mut links: BTreeSet<(i16, String)> = BTreeSet::new();
    let targets = ex
        .links
        .iter()
        .map(|l| &l.target)
        .chain(ex.redirect.as_ref());
    for t in targets {
        if t.interwiki.is_some() || t.text.is_empty() {
            continue;
        }
        let entity = namespaces
            .get(t.namespace)
            .is_some_and(scatter_titles::Namespace::is_entity_namespace);
        if entity {
            links.insert((crate::common::target_kind::ENTITY, t.text.clone()));
        } else {
            links.insert((TARGET_TITLE, format!("{}:{}", t.namespace, t.text)));
        }
    }
    cx.execute(
        "DELETE FROM view.page_link WHERE tenant = $1 AND from_page = $2 AND NOT from_render",
        &[&tenant, &page_id],
    )
    .await
    .map_err(|e| sql(&e))?;
    for (kind, id) in &links {
        rows += cx
            .execute(
                "INSERT INTO view.page_link (tenant, from_page, target_kind, target_id, from_render)
                 VALUES ($1, $2, $3, $4, false) ON CONFLICT DO NOTHING",
                &[&tenant, &page_id, kind, id],
            )
            .await
            .map_err(|e| sql(&e))?;
    }

    cx.execute(
        "DELETE FROM view.page_category WHERE tenant = $1 AND page_id = $2 AND NOT from_render",
        &[&tenant, &page_id],
    )
    .await
    .map_err(|e| sql(&e))?;
    let mut seen = BTreeSet::new();
    for c in &ex.categories {
        if !seen.insert(c.name.clone()) {
            continue;
        }
        let sortkey = c.sort_key.clone().unwrap_or_else(|| title.to_string());
        rows += cx
            .execute(
                "INSERT INTO view.page_category (tenant, page_id, category, sortkey, from_render)
                 VALUES ($1, $2, $3, $4, false) ON CONFLICT DO NOTHING",
                &[&tenant, &page_id, &c.name, &sortkey],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_configures_wikitext() {
        let c = wikitext_config();
        assert_eq!(c.namespace(4).map(|n| n.name.as_str()), Some("Project"));
        assert_eq!(c.namespace_by_name("image").map(|n| n.id), Some(6));
        let page = Title::new(4, "Home");
        let ex = scatter_wikitext::extract(
            "[[Item:Q5]] [[project:scope]] [[Category:X|k]]\n#REDIRECT [[Not first]]",
            c,
            &page,
        );
        // A `#REDIRECT` after the first line is a list item, and its link an ordinary one.
        assert!(ex.redirect.is_none());
        assert_eq!(ex.links.len(), 3);
        assert_eq!(ex.links[1].target, Title::new(4, "Scope"));
        assert_eq!(ex.links[2].target, Title::new(0, "Not first"));
        assert_eq!(ex.categories[0].name, "X");
        let with_site = Config::new(config_source(Some("Librarybase")));
        assert_eq!(
            with_site.namespace_by_name("Librarybase").map(|n| n.id),
            Some(4)
        );
        assert_eq!(
            with_site.namespace_by_name("Project").map(|n| n.id),
            Some(4)
        );
    }
}
