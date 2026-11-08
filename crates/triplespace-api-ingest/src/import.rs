//! The import of a MediaWiki XML export into a running tenant (0008 §9, A48; 10 §6;
//! 21 §4.3), as `triplespace import` and `action=import` both run it: the pages of the
//! file read into what the job takes ([`read_page`]), the template census with the
//! expander's registry and the tenant's title index ([`census`]), the job itself over
//! Postgres ([`import_pages`], with [`PgImportHost`] answering which titles exist), and
//! the talk pages converted after it ([`crate::talk_import`]).
//!
//! A page is accepted when its namespace holds pages and allows its content model, with
//! the registry's own namespaces, as the adoption job accepts them (0008 A41; 0042 A19):
//! the MediaWiki namespace's messages come as Interface pages (0095 A1), talk pages become
//! threads, pages in reserved namespaces are deferred, and entity pages are refused
//! (0008 A48). The first revision of a page is a `create` under the page's current title,
//! since an export lists a page's revisions under its current title; each later one is an
//! `edit`, with the model when it changed; the parts the source hid are left out and
//! named in `withheld`.

use std::collections::BTreeMap;

use scatter_ingest::adopt::Author;
use scatter_ingest::import::census::{Census, PageClass, Status, TextUses};
use scatter_ingest::import::{
    HeldPage, HeldRevision, Import, ImportHost, ImportOutcome, ImportPage, ImportRevision,
    run_import,
};
use scatter_ingest::write::Attestation;
use scatter_ingest::{IngestError, IngestStore as _};
use scatter_log_postgres::PgClient;
use scatter_mediawiki_xml::{DumpPage, Revision, sha1_base36};
use scatter_pages::{PageOp, validate, withheld};
use scatter_projection::Pipeline;
use scatter_titles::Namespaces;
use scatter_wikitext_expand::census as scanner;
use triplespace_projections::PgCx;
use triplespace_projections::pages::{PageRecord, shown_time};

use crate::PgIngest;
use crate::talk_import::Authors;

/// The registered issuer a source wiki is (0008 A50): a registered, non-template issuer
/// with numeric actors whose code is `source`, whose providers include `source` (a site
/// alias doubles as a provider slug, `enwiki`), or, given the source's host, whose users
/// API or actor IRIs are at that host. `None` for a wiki the registry does not know, whose
/// authors become `imported` surrogates.
#[must_use]
pub fn issuer_for(source: &str, host: Option<&str>) -> Option<String> {
    use scatter_actors::issuer::{ActorModel, Issuer, IssuerRegistry};
    let usable = |i: &Issuer| !i.template && !i.builtin && i.actor_model == ActorModel::Numeric;
    let reg = IssuerRegistry::default_registry();
    if let Some(i) = reg.by_code(source).filter(|i| usable(i)) {
        return Some(i.code.clone());
    }
    if let Some(i) = reg
        .issuers()
        .iter()
        .find(|i| usable(i) && i.providers.iter().any(|p| p == source))
    {
        return Some(i.code.clone());
    }
    let host = host?;
    let host_of = |url: &str| -> Option<String> {
        let rest = url.split("://").nth(1)?;
        Some(rest.split('/').next()?.to_ascii_lowercase())
    };
    reg.issuers()
        .iter()
        .find(|i| {
            usable(i)
                && [i.users_api.as_deref(), i.iri.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|u| host_of(u).as_deref() == Some(&host.to_ascii_lowercase()))
        })
        .map(|i| i.code.clone())
}

/// Writes an actor record for each of a registered issuer's accounts the file names, where
/// the tenant has none yet (0008 A50), so that the history shows their names: `{issuer}:{id}`
/// with the name the file gives, as adoption records the adopted wiki's accounts (0035 §5).
pub async fn ensure_accounts(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    tenant: &str,
    issuer: &str,
    accounts: &BTreeMap<String, u64>,
    attestation: &Attestation,
    now: u64,
) -> Result<u64, IngestError> {
    use scatter_actors::actor::ActorRecord;
    use scatter_actors::key::ActorKey;
    use scatter_ingest::store::Sequence;
    use scatter_ingest::write::append_projected;
    use scatter_projection::{Backend as _, Budget};
    let mut cx = store.begin().await?;
    let Some(actors) = store.partition(&mut cx, tenant, "actors").await? else {
        store.rollback(cx).await?;
        return Ok(0);
    };
    let mut written = 0u64;
    for (name, id) in accounts {
        let key = ActorKey::numeric(issuer, *id).to_string();
        if store.latest_for_key(&mut cx, actors, &key).await?.is_some() {
            continue;
        }
        let content = serde_json::to_value(ActorRecord::registered(name))
            .map_err(|e| IngestError::Store(e.to_string()))?;
        let draft = scatter_log::store::Draft {
            appended_at: now,
            payload_type: triplespace_projections::actors::payload::ACTOR.into(),
            key: Some(key),
            revid: None,
            logid: Some(store.next_id(&mut cx, tenant, &Sequence::Log).await?),
            page_id: None,
            body: scatter_log::body::Body::core(
                &scatter_log::cbor::Value::from_json(&content),
                &scatter_log::cbor::Value::Null,
                &attestation.to_value(),
            )
            .map_err(|e| IngestError::Store(e.to_string()))?,
        };
        append_projected(store, pipeline, &mut cx, actors, draft, Budget::NONE).await?;
        written += 1;
    }
    store.commit(cx).await?;
    Ok(written)
}

/// The content models of entity pages, which only adoption brings (0008 A48).
pub const ENTITY_MODELS: &[&str] = &["wikibase-item", "wikibase-property", "wikibase-lexeme"];

/// What the import will do with a dump page (10 §6).
#[must_use]
pub fn classify(page: &DumpPage) -> PageClass {
    let model = page
        .revisions
        .last()
        .and_then(|r| r.model.as_deref())
        .unwrap_or("wikitext");
    if ENTITY_MODELS.contains(&model) {
        return PageClass::Entity;
    }
    if crate::talk_import::is_talk_page(page) {
        return PageClass::Talk;
    }
    let Ok(ns) = i32::try_from(page.ns) else {
        return PageClass::Deferred(format!("namespace {} is out of range", page.ns));
    };
    let Some(namespace) = Namespaces::embedded().get(ns) else {
        return PageClass::Deferred(format!("namespace {ns} is not registered here"));
    };
    if namespace.kind != scatter_titles::Kind::Pages {
        return PageClass::Deferred(format!("`{}` holds no pages here", namespace.canonical));
    }
    let model = page
        .revisions
        .last()
        .and_then(|r| r.model.clone())
        .or_else(|| namespace.default_model.clone())
        .unwrap_or_else(|| "wikitext".into());
    if ns == scatter_titles::INTERFACE {
        if let Some(why) = not_a_message(page, &model) {
            return PageClass::Ignored(why);
        }
        return PageClass::Page;
    }
    if !namespace.allows_model(&model) {
        return PageClass::Deferred(format!(
            "`{}` pages are not `{model}` here",
            namespace.canonical
        ));
    }
    PageClass::Page
}

/// Why a MediaWiki-namespace page is not a message the instance can use: a revision of a
/// model other than `wikitext` or `text`.
fn not_a_message(page: &DumpPage, model: &str) -> Option<String> {
    page.revisions
        .iter()
        .map(|r| r.model.as_deref().unwrap_or(model))
        .find(|m| !matches!(*m, "wikitext" | "text"))
        .map(|other| format!("`MediaWiki` pages of model `{other}`"))
}

/// The parts of a revision the source hid, as `withheld` names them.
fn hidden_parts(r: &Revision) -> Vec<String> {
    [
        (r.text_hidden, withheld::TEXT),
        (r.comment_hidden, withheld::COMMENT),
        (r.user_hidden, withheld::ATTESTATION),
    ]
    .into_iter()
    .filter(|(hidden, _)| *hidden)
    .map(|(_, part)| part.to_string())
    .collect()
}

/// The author of a dump revision (0007 §5): hidden, an IP's `anonymous` surrogate, or a
/// named account as [`Authors`] resolves it.
#[must_use]
pub fn author_of(r: &Revision, authors: &Authors<'_>) -> Author {
    if r.user_hidden {
        return Author::Hidden;
    }
    match (&r.user, &r.ip) {
        (Some((_, name)), _) => authors.resolve(name),
        (None, Some(ip)) => Author::Surrogate {
            kind: "anonymous".into(),
            raw: ip.clone(),
        },
        (None, None) => Author::Hidden,
    }
}

/// A page's title without its namespace prefix.
fn local_title(page: &DumpPage) -> String {
    if page.ns == 0 {
        return page.title.clone();
    }
    page.title
        .split_once(':')
        .map_or_else(|| page.title.clone(), |(_, t)| t.to_string())
}

/// Reads a dump page the census classed [`PageClass::Page`] as what the job takes;
/// `Err` names why it cannot be imported, for the rejects.
pub fn read_page(page: &DumpPage, authors: &Authors<'_>) -> Result<ImportPage, String> {
    let ns =
        i32::try_from(page.ns).map_err(|_| format!("namespace {} is out of range", page.ns))?;
    let interface = ns == scatter_titles::INTERFACE;
    let namespace = Namespaces::embedded()
        .get(ns)
        .ok_or_else(|| format!("namespace {ns} is not registered here"))?;
    let default_model = namespace
        .default_model
        .clone()
        .unwrap_or_else(|| "wikitext".into());
    let title = local_title(page);
    let mut revisions = Vec::with_capacity(page.revisions.len());
    for r in &page.revisions {
        let model = if interface {
            "text".to_string()
        } else {
            r.model.clone().unwrap_or_else(|| default_model.clone())
        };
        let op = if revisions.is_empty() {
            PageOp::Create {
                ns,
                title: title.clone(),
                model: model.clone(),
                minor: r.minor,
                source: None,
            }
        } else {
            PageOp::Edit {
                model: Some(model.clone()),
                minor: r.minor,
                source: None,
            }
        };
        validate(&op, Some("")).map_err(|e| format!("revision {}: {e}", r.id))?;
        let time = triplespace_projections::feed::parse_timestamp(&r.timestamp)
            .ok_or_else(|| format!("revision {} has no timestamp", r.id))?;
        let text = (!r.text_hidden).then(|| r.text.clone());
        let sha1 = match &text {
            Some(t) => r
                .sha1
                .clone()
                .filter(|s| s.len() == 31)
                .unwrap_or_else(|| sha1_base36(t)),
            None => String::new(),
        };
        revisions.push(ImportRevision {
            source_revid: r.id,
            source_parentid: r.parent_id,
            source_time: r.timestamp.clone(),
            time,
            model,
            text,
            sha1,
            minor: r.minor,
            comment: if r.comment_hidden {
                None
            } else {
                r.comment.clone()
            },
            withheld: hidden_parts(r),
            author: author_of(r, authors),
        });
    }
    Ok(ImportPage {
        ns,
        title,
        revisions,
    })
}

/// The scanner the census runs: `scatter-wikitext-expand`'s, over its registry.
#[must_use]
pub fn scan(text: &str) -> TextUses {
    let u = scanner::scan(text);
    let status = |s: scanner::Status| match s {
        scanner::Status::Implemented => Status::Implemented,
        scanner::Status::Chip => Status::Chip,
        scanner::Status::Ignored => Status::Ignored,
        scanner::Status::Unregistered => Status::Unregistered,
    };
    TextUses {
        templates: u.templates,
        modules: u.modules,
        functions: u
            .functions
            .into_iter()
            .map(|(k, s)| (k, status(s)))
            .collect(),
        tags: u.tags.into_iter().map(|(k, s)| (k, status(s))).collect(),
    }
}

/// The census of a file's pages, before the job (10 §6): the pure census over the
/// classified pages, then the titles that already exist here from the tenant's title
/// index.
pub async fn census<'a, C: PgClient>(
    db: &C,
    tenant: &str,
    pages: impl IntoIterator<Item = &'a DumpPage>,
) -> Result<Census, tokio_postgres::Error> {
    let mut c =
        scatter_ingest::import::census::census(pages.into_iter().map(|p| (p, classify(p))), &scan);
    let namespaces: Vec<i32> = c
        .titles
        .iter()
        .filter_map(|(ns, _)| i32::try_from(*ns).ok())
        .collect();
    let titles: Vec<String> = c
        .titles
        .iter()
        .map(|(ns, title)| {
            if *ns == 0 {
                title.clone()
            } else {
                title
                    .split_once(':')
                    .map_or_else(|| title.clone(), |(_, t)| t.to_string())
            }
        })
        .collect();
    if !titles.is_empty() {
        let rows = db
            .query(
                "SELECT t.ns, t.title FROM unnest($2::int[], $3::text[]) AS t(ns, title)
                 JOIN view.page p ON p.tenant = $1 AND p.ns = t.ns AND p.title = t.title
                 WHERE NOT p.deleted AND p.latest_offset IS NOT NULL
                 ORDER BY t.ns, t.title",
                &[&tenant, &namespaces, &titles],
            )
            .await?;
        let names = Namespaces::embedded();
        c.existing = rows
            .iter()
            .map(|r| {
                let ns: i32 = r.get(0);
                let title: String = r.get(1);
                match names.get(ns) {
                    Some(n) if ns != 0 => format!("{}:{title}", n.canonical),
                    _ => title,
                }
            })
            .collect();
    }
    Ok(c)
}

/// The tenant's title index, for the job: the page a title names and the timestamps and
/// text hashes of its revisions, read from its records.
#[derive(Debug, Clone, Copy, Default)]
pub struct PgImportHost;

impl ImportHost<PgIngest> for PgImportHost {
    async fn existing(
        &self,
        store: &PgIngest,
        cx: &mut PgCx,
        tenant: &str,
        ns: i32,
        title: &str,
    ) -> Result<Option<HeldPage>, IngestError> {
        let db = |e: tokio_postgres::Error| IngestError::Store(e.to_string());
        let Some(row) = cx
            .query_opt(
                "SELECT page_id, content_model FROM view.page
                 WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted AND latest_offset IS NOT NULL",
                &[&tenant, &ns, &title],
            )
            .await
            .map_err(db)?
        else {
            return Ok(None);
        };
        let page_id = u64::try_from(row.get::<_, i64>(0)).unwrap_or_default();
        let mut model: String = row.get::<_, Option<String>>(1).unwrap_or_default();
        let Some(partition) = store.partition(cx, tenant, "pages").await? else {
            return Ok(None);
        };
        let records =
            scatter_log_postgres::log::read_by_key(cx, partition, &page_id.to_string()).await?;
        let mut revisions = Vec::new();
        let mut current_text = None;
        for (_, record) in &records {
            let page = PageRecord::read(record).map_err(IngestError::Store)?;
            let Some(op) = page.op else { continue };
            match &op {
                PageOp::Create { model: m, .. } | PageOp::Edit { model: Some(m), .. } => {
                    model.clone_from(m);
                }
                PageOp::Edit { model: None, .. } => {}
                PageOp::Move { .. } => continue,
            }
            revisions.push(HeldRevision {
                time: shown_time(record.header(), &op),
                sha1: page.text.as_deref().map(sha1_base36).unwrap_or_default(),
            });
            current_text = page.text;
        }
        Ok(Some(HeldPage {
            page_id,
            model,
            revisions,
            current_text,
        }))
    }
}

/// What a run of the job over a file came to, beside the job's own outcome.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Read {
    /// Pages read for the job.
    pub pages: u64,
    /// Pages left out before the job, by reason: the census's deferred and ignored, and
    /// those the reader could not take.
    pub left_out: BTreeMap<String, u64>,
    /// Entity pages refused.
    pub entities: u64,
}

/// Runs the import job over a file's pages (10 §6 steps 1–2): each page the census
/// classes as a document page is read and written; talk pages are left for
/// [`crate::talk_import::convert_all`], which the caller runs after, and entity pages
/// are refused.
#[allow(clippy::too_many_arguments)]
pub async fn import_pages(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    tenant: &str,
    attestation: Attestation,
    import: Import,
    authors: &Authors<'_>,
    pages: impl IntoIterator<Item = DumpPage>,
    now: u64,
) -> Result<(ImportOutcome, Read), IngestError> {
    let mut read = Read::default();
    let mut unreadable: Vec<(usize, String)> = Vec::new();
    let items: Vec<ImportPage> = pages
        .into_iter()
        .enumerate()
        .filter_map(|(i, page)| match classify(&page) {
            PageClass::Page => match read_page(&page, authors) {
                Ok(p) => {
                    read.pages += 1;
                    Some(p)
                }
                Err(why) => {
                    unreadable.push((i + 1, format!("`{}`: {why}", page.title)));
                    *read.left_out.entry(why).or_default() += 1;
                    None
                }
            },
            PageClass::Talk => None,
            PageClass::Entity => {
                read.entities += 1;
                None
            }
            PageClass::Deferred(why) | PageClass::Ignored(why) => {
                *read.left_out.entry(why).or_default() += 1;
                None
            }
        })
        .collect();
    let mut outcome = run_import(
        store,
        pipeline,
        &PgImportHost,
        tenant,
        attestation,
        import,
        items,
        now,
    )
    .await?;
    for (line, reason) in unreadable {
        outcome.counts.rejected += 1;
        outcome.rejects.push(scatter_ingest::Reject {
            line,
            match_key: None,
            reason,
        });
    }
    Ok((outcome, read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_log::cbor::Value;

    fn revision(id: u64, parent: Option<u64>, text: &str) -> Revision {
        Revision {
            id,
            parent_id: parent,
            timestamp: format!("2026-01-0{id}T00:00:00Z"),
            user: Some((7, "Harej".into())),
            model: Some("wikitext".into()),
            text: text.into(),
            comment: Some(format!("edit {id}")),
            ..Revision::default()
        }
    }

    fn page(ns: i64, title: &str, revisions: Vec<Revision>) -> DumpPage {
        DumpPage {
            title: title.into(),
            ns,
            id: 10,
            redirect: None,
            revisions,
        }
    }

    #[test]
    fn classes_follow_the_namespace_registry() {
        assert_eq!(
            classify(&page(4, "Project:Scope", vec![revision(1, None, "x")])),
            PageClass::Page
        );
        assert_eq!(
            classify(&page(3, "User talk:Harej", vec![revision(1, None, "hi")])),
            PageClass::Talk
        );
        let mut item = revision(1, None, "{}");
        item.model = Some("wikibase-item".into());
        assert_eq!(
            classify(&page(120, "Item:Q1", vec![item])),
            PageClass::Entity
        );
        let mut css = revision(1, None, "body {}");
        css.model = Some("css".into());
        assert!(matches!(
            classify(&page(8, "MediaWiki:Common.css", vec![css])),
            PageClass::Ignored(_)
        ));
        assert_eq!(
            classify(&page(
                8,
                "MediaWiki:Sidebar",
                vec![revision(1, None, "* x")]
            )),
            PageClass::Page
        );
        assert!(matches!(
            classify(&page(999, "Foo:Bar", vec![revision(1, None, "x")])),
            PageClass::Deferred(_)
        ));
    }

    #[test]
    fn a_page_is_read_with_its_history_and_authors() {
        let accounts: BTreeMap<String, u64> = [("Harej".to_string(), 7)].into();
        let authors = Authors::Imported {
            issuer: None,
            accounts: &accounts,
            prefix: "imported",
        };
        let mut second = revision(2, Some(1), "two");
        second.comment_hidden = true;
        second.comment = None;
        second.sha1 = Some("x".into());
        let mut third = revision(3, Some(2), "");
        third.text_hidden = true;
        third.user = None;
        third.ip = Some("203.0.113.9".into());
        let p = read_page(
            &page(
                4,
                "Project:Scope",
                vec![revision(1, None, "one"), second, third],
            ),
            &authors,
        )
        .unwrap();
        assert_eq!((p.ns, p.title.as_str()), (4, "Scope"));
        assert_eq!(p.revisions.len(), 3);
        assert_eq!(p.revisions[0].sha1, sha1_base36("one"));
        assert_eq!(
            p.revisions[1].sha1,
            sha1_base36("two"),
            "a malformed sha1 is recomputed"
        );
        assert_eq!(p.revisions[1].comment, None);
        assert_eq!(p.revisions[1].withheld, vec!["comment".to_string()]);
        assert_eq!(
            p.revisions[0].author,
            Author::Surrogate {
                kind: "imported".into(),
                raw: "imported>Harej".into()
            }
        );
        assert_eq!(
            p.revisions[2].author,
            Author::Surrogate {
                kind: "anonymous".into(),
                raw: "203.0.113.9".into()
            }
        );
        assert_eq!(p.revisions[2].text, None);
        assert_eq!(p.revisions[2].sha1, "");
        assert_eq!(p.revisions[2].withheld, vec!["text".to_string()]);
        assert!(p.revisions[0].time < p.revisions[1].time);
        // An interface message is read as `text`.
        let m = read_page(
            &page(8, "MediaWiki:Sidebar", vec![revision(1, None, "* x")]),
            &authors,
        )
        .unwrap();
        assert_eq!(m.revisions[0].model, "text");
        assert_eq!(m.title, "Sidebar");
        // A registered issuer's account.
        let authors = Authors::Imported {
            issuer: Some("lb"),
            accounts: &accounts,
            prefix: "lb",
        };
        let p = read_page(&page(0, "Alpha", vec![revision(1, None, "a")]), &authors).unwrap();
        assert_eq!(p.revisions[0].author, Author::Actor("lb:7".into()));
        let _ = Value::Null;
    }

    #[test]
    fn the_scanner_maps_statuses() {
        let u = scan("{{Cite web}} {{#if:a|b}} <ref>x</ref> {{#foo:x}}");
        assert!(u.templates.contains("Cite web"));
        assert_eq!(u.functions["#if"], Status::Implemented);
        assert_eq!(u.functions["#foo"], Status::Unregistered);
        assert_eq!(u.tags["ref"], Status::Implemented);
    }
}

/// An import as a surface asks for it (10 §6): what `action=import`, `Special:Import`,
/// `triplespace import` and the runner all hand to [`run_request`]. The pages travel
/// beside it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImportRequest {
    /// Where the file came from: `upload`, a site alias, or a URL.
    pub source: String,
    /// The file's identity: its name, or the export's `<base>`.
    #[serde(default)]
    pub version: Option<String>,
    /// The imported authors' prefix (`interwikiprefix`).
    pub prefix: String,
    /// The registered issuer the source is, if any (0008 A50).
    #[serde(default)]
    pub issuer: Option<String>,
    /// The job's reason.
    #[serde(default)]
    pub summary: Option<String>,
    /// The change tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether to leave the talk pages unconverted.
    #[serde(default)]
    pub skip_talk: bool,
}

/// What an import came to: the census it ran under, the job's outcome, what the reader
/// left out, and the talk conversion.
#[derive(Debug, Clone)]
pub struct ImportReport {
    /// The census.
    pub census: Census,
    /// The job.
    pub outcome: ImportOutcome,
    /// What the reader took and left.
    pub read: Read,
    /// The talk pages converted (0090 §10).
    pub talk: crate::talk_import::TalkCounts,
}

/// A stage of an import, for a running job's progress (0010 §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The census.
    Census,
    /// The accounts and the pages.
    Pages,
    /// The talk pages.
    Talk,
}

impl Stage {
    /// The stage's name, as the job's progress records it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Census => "census",
            Self::Pages => "pages",
            Self::Talk => "talk",
        }
    }
}

/// Runs an import as a whole (10 §6 steps 1–3): the census, the registered issuer's
/// accounts, the pages under `job_id` where the runner minted one, and the talk pages.
/// `progress` is told each stage as it begins. Nothing is written when the census finds
/// nothing to import, and the error says so.
///
/// # Errors
///
/// The store's; a `Store` error when the file holds nothing to import.
#[allow(clippy::too_many_arguments)]
pub async fn run_request<C: PgClient>(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    db: &C,
    tenant: &str,
    actor: &str,
    request: &ImportRequest,
    job_id: Option<u64>,
    pages: Vec<DumpPage>,
    now: u64,
    progress: &mut (dyn FnMut(Stage) + Send),
) -> Result<ImportReport, IngestError> {
    progress(Stage::Census);
    let c = census(db, tenant, pages.iter())
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?;
    if c.imported == 0 && c.talk == 0 {
        return Err(IngestError::Store(
            "the file holds no page this wiki can import".into(),
        ));
    }
    progress(Stage::Pages);
    let accounts = crate::talk_import::accounts_of(pages.iter());
    let authors = Authors::Imported {
        issuer: request.issuer.as_deref(),
        accounts: &accounts,
        prefix: &request.prefix,
    };
    let mut attestation = Attestation::by(actor);
    attestation.tags.clone_from(&request.tags);
    if let Some(issuer) = &request.issuer {
        ensure_accounts(
            store,
            pipeline,
            tenant,
            issuer,
            &accounts,
            &attestation,
            now,
        )
        .await?;
    }
    let (outcome, read) = import_pages(
        store,
        pipeline,
        tenant,
        attestation,
        Import {
            source: request.source.clone(),
            version: request.version.clone(),
            history: !c.current_only,
            summary: request.summary.clone(),
            job_id,
        },
        &authors,
        pages.iter().cloned(),
        now,
    )
    .await?;
    let talk = if request.skip_talk || c.talk == 0 {
        crate::talk_import::TalkCounts::default()
    } else {
        progress(Stage::Talk);
        let site = scatter_talk::Site::english();
        let ctx = crate::talk_import::TalkContext {
            tenant,
            source: &request.source,
            authors,
            actor,
            job: Some(outcome.job_id),
            site: &site,
        };
        crate::talk_import::convert_all(
            store,
            pipeline,
            &ctx,
            pages.into_iter().filter(crate::talk_import::is_talk_page),
            now,
        )
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?
    };
    Ok(ImportReport {
        census: c,
        outcome,
        read,
        talk,
    })
}
