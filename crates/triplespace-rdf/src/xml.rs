//! Assembling `Special:Export`'s MediaWiki XML from the live store (0047 §8.1; 21 §4.2;
//! 0005 A119): the selected document pages from the tenant's `pages` partition, one
//! `<revision>` per record, and the selected local entities as `wikibase-item` and
//! `wikibase-property` pages, one `<revision>` per `view.entity_revision` row with the
//! canonical JSON of the local-graph state after it. History is the newest records up
//! to the caps (`export.max_history` per page, `export.max_entity_history` per entity),
//! or the current revision alone. The parts the reader may not read are decided by
//! [`crate::bundle::Reader`] exactly as the `records` format decides them (21 §4.4): a
//! withheld or erased text, comment or attestation is written as MediaWiki writes
//! revision-deleted content, and a record withheld whole is left out, a target with
//! none left being listed as missing. The XML is written through `scatter-mediawiki-xml`.
//!
//! Two phases, so that a refusal (`413` for a selection over `export.sync_revisions`)
//! comes before any byte of the body: [`plan`] reads the metadata of the revisions the
//! caps keep, the database cutting each target's history and stopping one row past the
//! limit, and decides what is withheld; [`write()`] then reads each revision's content and
//! streams the document into any `Write`, and [`write_chunks`] hands it on chunk by chunk,
//! waiting for each to be taken. [`build`] does both phases into memory.
//!
//! **Not yet:** mirrored pages from `pages/{repo}` (13 §2.6), since no such partition is
//! projected yet; `templates`, which waits for `view.transclusion` (0047 A33) and is
//! answered with a notice; files as `<upload>` elements (out of scope).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use scatter_integrity::bundle::Withhold;
use scatter_log::body::{ATTESTATION, COMMENT, CONTENT};
use scatter_log_postgres::{PgClient, ids, log};
use scatter_mediawiki_xml::{DumpPage, Revision, SiteInfo, SiteNamespace, Writer};
use scatter_wikibase_model::EntityId;
use tokio_postgres::GenericClient;
use triplespace_projections::common::target_kind;
use triplespace_projections::pages::TenantNames;
use triplespace_projections::read::iso8601;
use triplespace_projections::revision_state;

use crate::bundle::{Offsets, PartitionRef, Reader, Shown, published, redact};
pub use crate::records::BuildError;

/// MediaWiki's `$wgExportPagelistLimit`: how many pages `addns` or `addcat` may add.
pub const PAGELIST_LIMIT: i64 = 5000;

/// What to export.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Page IDs, each with its title as asked.
    pub pages: Vec<(u64, String)>,
    /// Local entity IDs, each as asked.
    pub entities: Vec<(String, String)>,
    /// Every revision, up to the caps, rather than the current one.
    pub history: bool,
    /// Revisions per page, at most (`export.max_history`).
    pub max_page_history: usize,
    /// Revisions per entity, at most (`export.max_entity_history`).
    pub max_entity_history: usize,
    /// `templates`: accepted, and answered with a notice until `view.transclusion`
    /// exists (0047 A33).
    pub templates: bool,
}

/// The `<siteinfo>` of a tenant's export: its names for the namespaces, and what the
/// caller knows of the site.
#[must_use]
pub fn site_info(
    names: &TenantNames,
    lang: &str,
    sitename: &str,
    dbname: &str,
    base: &str,
    generator: &str,
) -> SiteInfo {
    let mut namespaces: Vec<SiteNamespace> = names
        .source()
        .namespaces
        .iter()
        .map(|n| SiteNamespace {
            key: i64::from(n.id),
            case: if n.case_sensitive {
                "case-sensitive".into()
            } else {
                "first-letter".into()
            },
            name: n.name.clone(),
        })
        .collect();
    namespaces.sort_by_key(|n| n.key);
    namespaces.dedup_by_key(|n| n.key);
    SiteInfo {
        lang: lang.to_string(),
        sitename: sitename.to_string(),
        dbname: dbname.to_string(),
        base: base.to_string(),
        generator: generator.to_string(),
        case: "first-letter".into(),
        namespaces,
    }
}

/// A title with its namespace's local name, `Project:Scope`.
fn prefixed(site: &SiteInfo, ns: i64, title: &str) -> String {
    match site.namespaces.iter().find(|n| n.key == ns) {
        Some(n) if !n.name.is_empty() => format!("{}:{title}", n.name),
        _ if ns == 0 => title.to_string(),
        _ => format!("{ns}:{title}"),
    }
}

/// Who made a revision, as the dump names them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Contributor {
    /// A registered account, by user ID and name; an `imported>Name` has ID 0, as the
    /// source wrote it.
    User(u64, String),
    /// An anonymous edit.
    Ip(String),
    /// No attestation to show.
    Hidden,
}

/// One revision to write.
#[derive(Debug, Clone)]
struct RevisionPlan {
    partition: u64,
    offset: u64,
    revid: u64,
    parent: u64,
    time: u64,
    contributor: Contributor,
    comment: Option<String>,
    minor: bool,
    withhold: Option<Withhold>,
}

/// What a page is: a document page, or a local entity.
#[derive(Debug, Clone)]
enum Kind {
    Page,
    Entity(EntityId),
}

/// One page to write, with its revisions oldest first.
#[derive(Debug, Clone)]
struct PagePlan {
    head: DumpPage,
    kind: Kind,
    model: String,
    revisions: Vec<RevisionPlan>,
}

/// An export planned: what [`write()`] streams.
#[derive(Debug, Clone)]
pub struct Plan {
    site: SiteInfo,
    pages: Vec<PagePlan>,
    /// The titles asked for that the export does not hold: absent, deleted, or nothing
    /// of them readable.
    pub missing: Vec<String>,
    /// What the export says about itself: `templates` added nothing, and the like.
    pub notices: Vec<String>,
}

impl Plan {
    /// The revisions the export holds.
    #[must_use]
    pub fn revisions(&self) -> usize {
        self.pages.iter().map(|p| p.revisions.len()).sum()
    }

    /// The pages the export holds.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.pages.len()
    }
}

fn db(e: impl std::fmt::Display) -> BuildError {
    BuildError::Database(e.to_string())
}

/// The content model of an entity's page, by its type (namespaces.toml; 0041 §3), and
/// the namespace that holds it.
fn entity_page(id: &EntityId, entity_type: &str) -> (i64, &'static str) {
    if let Some((t, _)) = id.keyed_parts() {
        return match t {
            "domain" => (210, "triplespace-domain"),
            "keyword" => (212, "triplespace-keyword"),
            _ => (216, "triplespace-notation"),
        };
    }
    match entity_type {
        "property" => (122, "wikibase-property"),
        "lexeme" => (146, "wikibase-lexeme"),
        _ => (120, "wikibase-item"),
    }
}

/// The `format` of a content model, from the registry.
fn format_of(model: &str) -> String {
    scatter_pages::ContentModels::embedded()
        .get(model)
        .and_then(|m| m.format.clone())
        .unwrap_or_else(|| "application/json".into())
}

/// The contributor of an activity row, from its actor.
fn contributor(
    key: Option<&str>,
    name: Option<&str>,
    kind: Option<&str>,
    raw: Option<&str>,
    issuer: Option<&str>,
    subject: Option<&str>,
) -> Contributor {
    let Some(key) = key else {
        return Contributor::Hidden;
    };
    match kind {
        Some("anonymous") => Contributor::Ip(raw.or(name).unwrap_or(key).to_string()),
        Some("imported") => Contributor::User(0, raw.or(name).unwrap_or(key).to_string()),
        _ => {
            let id = subject
                .filter(|_| issuer != Some(scatter_actors::issuer::ACTOR))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            Contributor::User(id, name.unwrap_or(key).to_string())
        }
    }
}

/// MediaWiki's comment field: the automatic summary, then the user's comment (0001 §6).
fn comment_text(
    op: Option<&str>,
    args: Option<&str>,
    log_type: Option<&str>,
    log_action: Option<&str>,
    comment: Option<&str>,
) -> Option<String> {
    let mut s = String::new();
    if let Some(op) = op {
        s.push_str("/* ");
        s.push_str(op);
        if let Some(args) = args {
            s.push(':');
            s.push_str(args);
        }
        s.push_str(" */");
    } else if let (Some(t), Some(a)) = (log_type, log_action) {
        s.push_str("/* ");
        s.push_str(t);
        s.push('/');
        s.push_str(a);
        s.push_str(" */");
    }
    if let Some(c) = comment {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(c);
    }
    (!s.is_empty()).then_some(s)
}

/// How many rows a query may return for `limit`: one more, which says the limit is passed.
pub(crate) fn one_past(limit: usize) -> i64 {
    i64::try_from(limit).map_or(i64::MAX, |l| l.saturating_add(1))
}

/// A history cap as the database takes it: at least one.
pub(crate) fn cap_of(cap: usize) -> i64 {
    i64::try_from(cap.max(1)).unwrap_or(i64::MAX)
}

/// The revision rows of the selected targets in the published partitions, oldest first
/// per target, with their contributors: of each target the newest `page_cap` or
/// `entity_cap` only, each with its parent (the revision before it, kept or not), and
/// of all of them no more than one past `limit`. The cut is the database's, so a long
/// history is never read whole; the count of what the caps keep comes with the rows.
#[allow(clippy::too_many_arguments)]
async fn revision_rows<C: PgClient + GenericClient>(
    client: &C,
    tenant: &str,
    partitions: &[i64],
    page_ids: &[String],
    entity_ids: &[String],
    page_cap: usize,
    entity_cap: usize,
    limit: usize,
) -> Result<BTreeMap<(i16, String), Vec<RevisionPlan>>, BuildError> {
    let rows = PgClient::query(
        client,
        "WITH ranked AS (
           SELECT a.partition, a.\"offset\", a.target_kind, a.target_id,
                  row_number() OVER (PARTITION BY a.target_kind, a.target_id
                                     ORDER BY a.partition DESC, a.\"offset\" DESC) AS newest,
                  lag(a.revid) OVER (PARTITION BY a.target_kind, a.target_id
                                     ORDER BY a.partition, a.\"offset\") AS parent
           FROM view.activity a
           WHERE a.tenant = $1 AND a.partition = ANY($2)
             AND a.kind IN ('edit', 'erased') AND a.revid IS NOT NULL
             AND ((a.target_kind = $3 AND a.target_id = ANY($4))
               OR (a.target_kind = $5 AND a.target_id = ANY($6)))
         ), kept AS (
           SELECT r.partition, r.\"offset\", r.parent, count(*) OVER () AS total
           FROM ranked r
           WHERE r.newest <= CASE WHEN r.target_kind = $3 THEN $7::bigint ELSE $8::bigint END
           ORDER BY r.partition, r.\"offset\" LIMIT $9
         )
         SELECT a.partition, a.\"offset\", a.target_kind, a.target_id, a.revid,
                (EXTRACT(EPOCH FROM a.time) * 1000000)::bigint,
                a.actor_key, ac.name, ac.kind, ac.raw, ac.issuer, ac.subject,
                a.summary_op, a.summary_args, a.log_type, a.log_action, a.comment,
                coalesce(a.minor, false), coalesce(k.parent, 0), k.total
         FROM kept k
         JOIN view.activity a
           ON a.tenant = $1 AND a.partition = k.partition AND a.\"offset\" = k.\"offset\"
         LEFT JOIN LATERAL (
           SELECT x.name, x.kind, x.raw, x.issuer, x.subject FROM view.actor x
           WHERE x.actor_key = a.actor_key AND x.tenant IN (a.tenant, '')
           ORDER BY (x.tenant = a.tenant) DESC LIMIT 1) ac ON true
         ORDER BY a.partition, a.\"offset\"",
        &[
            &tenant,
            &partitions,
            &target_kind::PAGE,
            &page_ids,
            &target_kind::ENTITY,
            &entity_ids,
            &cap_of(page_cap),
            &cap_of(entity_cap),
            &one_past(limit),
        ],
    )
    .await
    .map_err(db)?;
    let total = rows.first().map_or(0, |r| {
        usize::try_from(r.get::<_, i64>(19)).unwrap_or(usize::MAX)
    });
    if total > limit {
        return Err(BuildError::TooLarge {
            records: total,
            limit,
        });
    }
    let mut by_target: BTreeMap<(i16, String), Vec<RevisionPlan>> = BTreeMap::new();
    for r in &rows {
        let get = |i: usize| r.get::<_, Option<String>>(i);
        let plan = RevisionPlan {
            partition: ids::partition_from_db(r.get(0)),
            offset: u64::try_from(r.get::<_, i64>(1)).unwrap_or_default(),
            revid: u64::try_from(r.get::<_, i64>(4)).unwrap_or_default(),
            parent: u64::try_from(r.get::<_, i64>(18)).unwrap_or_default(),
            time: u64::try_from(r.get::<_, i64>(5)).unwrap_or_default(),
            contributor: contributor(
                get(6).as_deref(),
                get(7).as_deref(),
                get(8).as_deref(),
                get(9).as_deref(),
                get(10).as_deref(),
                get(11).as_deref(),
            ),
            comment: comment_text(
                get(12).as_deref(),
                get(13).as_deref(),
                get(14).as_deref(),
                get(15).as_deref(),
                get(16).as_deref(),
            ),
            minor: r.get(17),
            withhold: None,
        };
        by_target
            .entry((r.get(2), r.get(3)))
            .or_default()
            .push(plan);
    }
    Ok(by_target)
}

/// Plans the export of `selection` on `tenant` for `reader`, refusing more than `limit`
/// revisions.
///
/// # Errors
///
/// A selection over `limit`, or a database failure.
#[allow(clippy::too_many_lines)]
pub async fn plan<C: PgClient + GenericClient>(
    client: &C,
    tenant: &str,
    reader: &Reader,
    selection: &Selection,
    site: SiteInfo,
    limit: usize,
) -> Result<Plan, BuildError> {
    let mut rows = Vec::new();
    for name in ["pages", "local"] {
        if let Some(r) = published(client, tenant, reader, PartitionRef::Name(name))
            .await
            .map_err(db)?
        {
            rows.push(r);
        }
    }
    let partition_ids: Vec<i64> = rows
        .iter()
        .map(|r| ids::partition_to_db(r.partition))
        .collect();
    let page_ids: Vec<String> = selection
        .pages
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    let entity_ids: Vec<String> = selection
        .entities
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    // Each target's revisions, oldest first, cut to the newest its cap allows, and the
    // selection refused over `limit`: both by the database, before the rows are read.
    let (page_cap, entity_cap) = if selection.history {
        (selection.max_page_history, selection.max_entity_history)
    } else {
        (1, 1)
    };
    let mut by_target = revision_rows(
        client,
        tenant,
        &partition_ids,
        &page_ids,
        &entity_ids,
        page_cap,
        entity_cap,
        limit,
    )
    .await?;
    let offsets: Vec<u64> = by_target
        .values()
        .flatten()
        .map(|r| r.offset)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let withhold = reader
        .decide(client, tenant, &partition_ids, Offsets::At(&offsets))
        .await
        .map_err(db)?;
    for revisions in by_target.values_mut() {
        for r in revisions.iter_mut() {
            r.withhold = withhold.get(&(r.partition, r.offset)).cloned();
        }
        // A record withheld whole is not written at all.
        revisions.retain(|r| !matches!(r.withhold, Some(Withhold::Record)));
    }
    by_target.retain(|_, revisions| !revisions.is_empty());

    // The pages' rows: title, namespace, model, redirect.
    let page_ids_db: Vec<i64> = selection
        .pages
        .iter()
        .filter_map(|(id, _)| i64::try_from(*id).ok())
        .collect();
    let page_rows = PgClient::query(
        client,
        "SELECT p.page_id, p.ns, p.title, p.content_model, r.target_ns, r.target_title
         FROM view.page p
         LEFT JOIN view.redirect r ON r.tenant = p.tenant AND r.page_id = p.page_id AND p.is_redirect
         WHERE p.tenant = $1 AND p.page_id = ANY($2) AND NOT p.deleted",
        &[&tenant, &page_ids_db],
    )
    .await
    .map_err(db)?;
    let mut heads: BTreeMap<(i16, String), PagePlan> = BTreeMap::new();
    for r in &page_rows {
        let page_id = u64::try_from(r.get::<_, i64>(0)).unwrap_or_default();
        let ns = i64::from(r.get::<_, i32>(1));
        let model: String = r.get(3);
        let redirect = match (r.get::<_, Option<i32>>(4), r.get::<_, Option<String>>(5)) {
            (Some(tns), Some(title)) => Some(prefixed(&site, i64::from(tns), &title)),
            _ => None,
        };
        heads.insert(
            (target_kind::PAGE, page_id.to_string()),
            PagePlan {
                head: DumpPage {
                    title: prefixed(&site, ns, r.get(2)),
                    ns,
                    id: page_id,
                    redirect,
                    revisions: Vec::new(),
                },
                kind: Kind::Page,
                model,
                revisions: Vec::new(),
            },
        );
    }
    let entity_rows = PgClient::query(
        client,
        "SELECT id, type, page_id FROM view.entity
         WHERE tenant = $1 AND id = ANY($2) AND NOT deleted",
        &[&tenant, &entity_ids],
    )
    .await
    .map_err(db)?;
    for r in &entity_rows {
        let text: String = r.get(0);
        let Ok(id) = EntityId::parse(&text) else {
            continue;
        };
        let (ns, model) = entity_page(&id, r.get(1));
        let title = match id.keyed_parts() {
            Some((_, key)) => key.to_string(),
            None => id.as_str().to_string(),
        };
        heads.insert(
            (target_kind::ENTITY, text.clone()),
            PagePlan {
                head: DumpPage {
                    title: prefixed(&site, ns, &title),
                    ns,
                    id: u64::try_from(r.get::<_, i64>(2)).unwrap_or_default(),
                    redirect: None,
                    revisions: Vec::new(),
                },
                kind: Kind::Entity(id),
                model: model.to_string(),
                revisions: Vec::new(),
            },
        );
    }

    let mut missing = Vec::new();
    let mut pages = Vec::new();
    let asked = selection
        .pages
        .iter()
        .map(|(id, t)| ((target_kind::PAGE, id.to_string()), t.clone()))
        .chain(
            selection
                .entities
                .iter()
                .map(|(id, t)| ((target_kind::ENTITY, id.clone()), t.clone())),
        );
    let mut seen = BTreeSet::new();
    for (key, title) in asked {
        if !seen.insert(key.clone()) {
            continue;
        }
        match (heads.remove(&key), by_target.remove(&key)) {
            (Some(mut page), Some(revisions)) => {
                page.revisions = revisions;
                pages.push(page);
            }
            _ => missing.push(title),
        }
    }
    let mut notices = Vec::new();
    if selection.templates {
        notices.push(
            "templates: no templates were added; the transclusion index (view.transclusion) is not built yet"
                .to_string(),
        );
    }
    Ok(Plan {
        site,
        pages,
        missing,
        notices,
    })
}

/// The content of a revision after redaction: the text, or `None` where it is hidden.
async fn content<C: PgClient>(
    client: &C,
    page: &PagePlan,
    r: &RevisionPlan,
) -> Result<Option<String>, BuildError> {
    let row = client
        .query_opt(
            "SELECT state FROM view.entity_revision WHERE partition = $1 AND \"offset\" = $2",
            &[
                &ids::partition_to_db(r.partition),
                &i64::try_from(r.offset).unwrap_or(i64::MAX),
            ],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let bytes: Vec<u8> = row.get(0);
    Ok(Some(match &page.kind {
        Kind::Page => String::from_utf8_lossy(&bytes).into_owned(),
        Kind::Entity(id) => {
            let state = revision_state::decode(&bytes).map_err(db)?;
            match (&state.entity, &state.redirect_to) {
                (Some(entity), _) => entity.to_canonical_json(),
                (None, Some(to)) => format!(
                    "{{\"entity\":\"{}\",\"redirect\":\"{}\"}}",
                    id.as_str(),
                    to.as_str()
                ),
                (None, None) => String::new(),
            }
        }
    }))
}

/// Writes a planned export into `out`, reading each revision's record and content as
/// it goes, and hands the sink back.
///
/// # Errors
///
/// A database or write failure.
pub async fn write<C: PgClient, W: Write>(
    client: &C,
    plan: &Plan,
    mut out: W,
) -> Result<W, BuildError> {
    write_chunks(client, plan, CHUNK, |bytes| {
        std::future::ready(
            out.write_all(&bytes)
                .map_err(|e| BuildError::Io(e.to_string())),
        )
    })
    .await?;
    out.flush().map_err(|e| BuildError::Io(e.to_string()))?;
    Ok(out)
}

/// The size [`write()`] gathers before it writes on: a chunk of a streamed export.
pub const CHUNK: usize = 64 * 1024;

/// Writes a planned export chunk by chunk: `emit` is given the document's next bytes
/// once `chunk` of them are gathered (a revision is never split, so a chunk is as much
/// larger as its last revision), and is awaited before anything more is read. A slow
/// reader of the export therefore holds the writing back, and no more than one chunk
/// and one revision is in memory at a time.
///
/// # Errors
///
/// A database failure, or `emit`'s: the document then ends without its closing tag.
pub async fn write_chunks<C, F, Fut>(
    client: &C,
    plan: &Plan,
    chunk: usize,
    mut emit: F,
) -> Result<(), BuildError>
where
    C: PgClient,
    F: FnMut(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<(), BuildError>>,
{
    let io = |e: scatter_mediawiki_xml::XmlError| BuildError::Io(e.to_string());
    let mut w = Writer::new(Vec::new());
    w.start(&plan.site).map_err(io)?;
    for note in &plan.notices {
        w.comment(note).map_err(io)?;
    }
    for page in &plan.pages {
        w.open_page(&page.head).map_err(io)?;
        // A page's revisions carry the model each had (0047 A37): the `create`'s, then
        // each `edit`'s change, walked in order; the page's current model where the
        // records that set it are withheld. An entity's is its type's.
        let mut model = page.model.clone();
        for r in &page.revisions {
            let slot = log::read(client, r.partition, r.offset).await.map_err(db)?;
            let (text_hidden, comment_hidden, user_hidden) =
                match redact(slot, r.withhold.as_ref()).map_err(db)? {
                    Shown::Leaf(_) => continue,
                    Shown::Record { record, .. } => {
                        if matches!(page.kind, Kind::Page)
                            && let Ok(Some(v)) = record.body().content().value()
                            && let Ok(j) = v.to_json()
                            && let Ok(op) = scatter_pages::PageOp::from_json(j)
                        {
                            match op {
                                scatter_pages::PageOp::Create { model: m, .. }
                                | scatter_pages::PageOp::Edit { model: Some(m), .. } => model = m,
                                _ => {}
                            }
                        }
                        let parts = record.body().parts();
                        let erased =
                            |i: usize| parts.get(i).is_none_or(scatter_log::body::Part::is_erased);
                        (erased(CONTENT), erased(COMMENT), erased(ATTESTATION))
                    }
                };
            let text = if text_hidden {
                None
            } else {
                content(client, page, r).await?
            };
            let (user, ip) = match &r.contributor {
                Contributor::User(id, name) => (Some((*id, name.clone())), None),
                Contributor::Ip(ip) => (None, Some(ip.clone())),
                Contributor::Hidden => (None, None),
            };
            let rev = Revision {
                id: r.revid,
                parent_id: (r.parent != 0).then_some(r.parent),
                timestamp: iso8601(r.time),
                user,
                ip,
                user_hidden: user_hidden || r.contributor == Contributor::Hidden,
                comment_hidden,
                text_hidden: text.is_none(),
                minor: r.minor,
                comment: if comment_hidden {
                    None
                } else {
                    r.comment.clone()
                },
                origin: Some(r.revid),
                model: Some(model.clone()),
                format: Some(format_of(&model)),
                text: text.unwrap_or_default(),
                sha1: None,
                slots: Vec::new(),
            };
            w.revision(&rev).map_err(io)?;
            if w.get_mut().len() >= chunk {
                emit(std::mem::take(w.get_mut())).await?;
            }
        }
        w.close_page().map_err(io)?;
    }
    let rest = w.finish().map_err(io)?;
    if rest.is_empty() {
        Ok(())
    } else {
        emit(rest).await
    }
}

/// A whole export in memory: [`plan`], then [`write()`] into a `Vec`.
#[derive(Debug, Clone)]
pub struct Export {
    /// The document.
    pub xml: Vec<u8>,
    /// What was asked for and not exported.
    pub missing: Vec<String>,
    /// What the export says about itself.
    pub notices: Vec<String>,
    /// Revisions written.
    pub revisions: usize,
}

/// Plans and writes the export of `selection` in memory.
///
/// # Errors
///
/// A selection over `limit`, or a database failure.
pub async fn build<C: PgClient + GenericClient>(
    client: &C,
    tenant: &str,
    reader: &Reader,
    selection: &Selection,
    site: SiteInfo,
    limit: usize,
) -> Result<Export, BuildError> {
    let planned = plan(client, tenant, reader, selection, site, limit).await?;
    let xml = write(client, &planned, Vec::new()).await?;
    let revisions = planned.revisions();
    Ok(Export {
        xml,
        missing: planned.missing,
        notices: planned.notices,
        revisions,
    })
}

/// The pages of a namespace, up to [`PAGELIST_LIMIT`]: what `addns` adds (21 §4.2).
///
/// # Errors
///
/// A database failure.
pub async fn pages_in_namespace<C: GenericClient>(
    client: &C,
    tenant: &str,
    ns: i32,
) -> Result<Vec<(u64, i32, String)>, BuildError> {
    let rows = client
        .query(
            "SELECT page_id, ns, title FROM view.page
             WHERE tenant = $1 AND ns = $2 AND NOT deleted AND latest_offset IS NOT NULL
             ORDER BY title LIMIT $3",
            &[&tenant, &ns, &PAGELIST_LIMIT],
        )
        .await
        .map_err(db)?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                u64::try_from(r.get::<_, i64>(0)).unwrap_or_default(),
                r.get(1),
                r.get(2),
            )
        })
        .collect())
}

/// The pages in a category, up to [`PAGELIST_LIMIT`]: what `addcat` adds. The name is
/// taken without its `Category:` prefix, with spaces for underscores and its first
/// letter upper-cased, as the category rows are kept.
///
/// # Errors
///
/// A database failure.
pub async fn pages_in_category<C: GenericClient>(
    client: &C,
    tenant: &str,
    category: &str,
) -> Result<Vec<(u64, i32, String)>, BuildError> {
    let name = category_name(category);
    let rows = client
        .query(
            "SELECT p.page_id, p.ns, p.title FROM view.page_category c
             JOIN view.page p ON p.tenant = c.tenant AND p.page_id = c.page_id
             WHERE c.tenant = $1 AND c.category = $2 AND NOT p.deleted AND p.latest_offset IS NOT NULL
             ORDER BY c.sortkey, p.page_id LIMIT $3",
            &[&tenant, &name, &PAGELIST_LIMIT],
        )
        .await
        .map_err(db)?;
    let mut seen = BTreeSet::new();
    Ok(rows
        .iter()
        .map(|r| {
            (
                u64::try_from(r.get::<_, i64>(0)).unwrap_or_default(),
                r.get::<_, i32>(1),
                r.get::<_, String>(2),
            )
        })
        .filter(|(id, _, _)| seen.insert(*id))
        .collect())
}

/// A category name as the rows keep it.
#[must_use]
pub fn category_name(category: &str) -> String {
    let name = category.trim().replace('_', " ");
    let name = name.trim();
    let name = name
        .split_once(':')
        .filter(|(prefix, _)| prefix.trim().eq_ignore_ascii_case("category"))
        .map_or(name, |(_, rest)| rest.trim());
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_named_as_the_rows_keep_them() {
        assert_eq!(category_name("Category:foo_bar"), "Foo bar");
        assert_eq!(category_name(" foo "), "Foo");
        assert_eq!(category_name("category: Wiki "), "Wiki");
        assert_eq!(category_name(""), "");
    }

    #[test]
    fn contributors_as_the_dump_names_them() {
        assert_eq!(
            contributor(
                Some("local:7"),
                Some("Alice"),
                Some("registered"),
                None,
                Some("local"),
                Some("7")
            ),
            Contributor::User(7, "Alice".into())
        );
        assert_eq!(
            contributor(
                Some("anon:1"),
                None,
                Some("anonymous"),
                Some("127.0.0.1"),
                Some("anon"),
                Some("1")
            ),
            Contributor::Ip("127.0.0.1".into())
        );
        assert_eq!(
            contributor(
                Some("imp:2"),
                None,
                Some("imported"),
                Some("imported>Harej"),
                Some("imp"),
                Some("2")
            ),
            Contributor::User(0, "imported>Harej".into())
        );
        assert_eq!(
            contributor(None, None, None, None, None, None),
            Contributor::Hidden
        );
    }

    #[test]
    fn comments_compose_as_wikibase_does() {
        assert_eq!(
            comment_text(
                Some("wbsetlabel-add"),
                Some("1|en"),
                None,
                None,
                Some("Six")
            )
            .as_deref(),
            Some("/* wbsetlabel-add:1|en */ Six")
        );
        assert_eq!(comment_text(None, None, None, None, None), None);
        assert_eq!(
            comment_text(None, None, Some("move"), Some("move"), None).as_deref(),
            Some("/* move/move */")
        );
    }

    #[test]
    fn titles_take_the_sites_names() {
        let site = SiteInfo {
            namespaces: vec![
                SiteNamespace {
                    key: 0,
                    case: "first-letter".into(),
                    name: String::new(),
                },
                SiteNamespace {
                    key: 4,
                    case: "first-letter".into(),
                    name: "Librarybase".into(),
                },
            ],
            ..SiteInfo::default()
        };
        assert_eq!(prefixed(&site, 0, "Home"), "Home");
        assert_eq!(prefixed(&site, 4, "Home"), "Librarybase:Home");
        assert_eq!(
            prefixed(&site, 120, "Q1"),
            "120:Q1",
            "a namespace the site does not name"
        );
    }
}
