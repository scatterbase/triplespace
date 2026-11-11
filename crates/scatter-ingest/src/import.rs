//! The general import job of 0008 §9 (10 §6; 0005 A119): the pages of any MediaWiki's
//! XML export, read through `scatter-mediawiki-xml` by the surface that runs the job,
//! become page records in the tenant's `pages` partition. Unlike the adoption job
//! ([`crate::adopt`]), which keeps the source's page and revision IDs, an import goes
//! through the load path as every ingest does: the page IDs and revision IDs are the
//! tenant's own, minted as written (0008 A48), and only the source's revision ID, parent
//! and timestamp are kept, as upstream metadata in the record's content (10 §3.2).
//!
//! **Each revision keeps its original timestamp** as `source_time`; the record's
//! `appended_at` is still the time of the import. **Each revision is attributed to its
//! original author** under 0007 §5: the surface resolves a registered issuer's accounts
//! to their numeric keys, and every other author comes as a surrogate of kind `imported`
//! (or `anonymous` for an IP), which the job mints once per raw value and tenant, as the
//! adoption job does.
//!
//! **Importing onto an existing title** does what MediaWiki's importer does (0008 A48):
//! the file's revisions are merged into the page's history by timestamp, a revision the
//! page already holds (same timestamp and SHA-1 of the text) is skipped, and the newest
//! revision by timestamp becomes the current one. In an append-only log a revision can
//! only be appended, so a page's history is its records in order of appending, each
//! shown at its `source_time`; when the file's revisions are all older than the revision
//! the page already had as current, that revision is appended again by the job as a
//! restoring edit, so that the current text is the newest by timestamp. The skip rule
//! makes the job idempotent: an import that is interrupted and run again writes nothing
//! twice.
//!
//! What the job needs of the surface beyond the [`IngestStore`] is an [`ImportHost`]:
//! the page a title already names, with its revisions' timestamps and text hashes, which
//! only the surface's title index can answer. Entity pages never reach the job: the
//! census refuses them (`census`), since an entity's identifier is assigned by the load
//! path that creates it, never copied from a file (0008 A48).
//!
//! implements 0008 §9.1 (A50)
//! implements 0008 §9.4 (A49)

pub mod census;

use std::collections::BTreeMap;
use std::future::Future;

use scatter_projection::{Budget, Pipeline};
use scatter_wikibase_changeset::{JobHeader, Mode};
use serde::{Deserialize, Serialize};

use crate::IngestError;
pub use crate::adopt::surrogate;

use crate::adopt::Author;
use crate::job::{Job, Reject};
use crate::store::{IngestStore, Sequence};
use crate::write::{Attestation, RawRecord, Request, write_raw_many};

/// The page payload type (10 §3.2), which `scatter-pages` names too; this crate does
/// not depend on it (22 §2.1), so the job encodes the content part itself.
pub const PAYLOAD_PAGE: &str = "scatter:v0/page";

/// The edit summary of the restoring edit, which puts a page's own newest revision back in
/// front of older ones the import appended behind it.
pub const RESTORE_COMMENT: &str = "Restored the newest revision after an import";

/// One revision of a page to import, as the surface reads it from the export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRevision {
    /// The source's revision ID.
    pub source_revid: u64,
    /// The revision it followed on the source, as the file says.
    pub source_parentid: Option<u64>,
    /// The source's timestamp, as the file writes it (`2026-09-20T14:02:11Z`).
    pub source_time: String,
    /// The same, in µs since the epoch: what the merge orders and compares by.
    pub time: u64,
    /// The content model.
    pub model: String,
    /// The text; `None` where the source hid it.
    pub text: Option<String>,
    /// MediaWiki's base-36 SHA-1 of the text, the file's or computed; empty when hidden.
    pub sha1: String,
    /// A minor edit.
    pub minor: bool,
    /// The edit summary; `None` where the source hid it, or had none.
    pub comment: Option<String>,
    /// The parts the source hid: `text`, `comment`, `attestation`.
    pub withheld: Vec<String>,
    /// Who made it (0007 §5).
    pub author: Author,
}

/// One page to import, with its revisions in any order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPage {
    /// The namespace number, the tenant's.
    pub ns: i32,
    /// The title within the namespace, in canonical form.
    pub title: String,
    /// The revisions.
    pub revisions: Vec<ImportRevision>,
}

/// A revision a page already holds, as the merge compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldRevision {
    /// The revision's shown time, µs since the epoch: its `source_time` where it was
    /// adopted or imported, else when it was appended.
    pub time: u64,
    /// MediaWiki's base-36 SHA-1 of its text; empty where the record carries none.
    pub sha1: String,
}

/// The page a title already names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldPage {
    /// Its page ID.
    pub page_id: u64,
    /// Its current content model.
    pub model: String,
    /// Its revisions in order of appending, the current one last.
    pub revisions: Vec<HeldRevision>,
    /// The current revision's text, for the restoring edit; `None` where it has none.
    pub current_text: Option<String>,
}

/// What the job needs of the surface: the page a title already names.
pub trait ImportHost<S: IngestStore> {
    /// The page `title` in `ns` names for the tenant, with its revisions, or `None`.
    fn existing(
        &self,
        store: &S,
        cx: &mut S::Cx,
        tenant: &str,
        ns: i32,
        title: &str,
    ) -> impl Future<Output = Result<Option<HeldPage>, IngestError>> + Send;
}

/// An import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Where the file came from: `upload`, a site alias, or a URL.
    pub source: String,
    /// The file's identity: its name, or the export's `<base>`.
    pub version: Option<String>,
    /// Whether the file carried full history, for the job's record.
    pub history: bool,
    /// The reason the submitter gave, for the job's record.
    pub summary: Option<String>,
    /// The job ID the runner minted at submission (0104 §4), under which the start record
    /// is written; `None` mints one at the start.
    pub job_id: Option<u64>,
}

/// A job's counts by outcome.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImportCounts {
    /// Pages created.
    pub created: u64,
    /// Pages that existed, and took revisions into their history.
    pub merged: u64,
    /// Revisions written, the restoring edits excluded.
    pub revisions: u64,
    /// Revisions skipped because the page already held them.
    pub present: u64,
    /// Restoring edits written.
    pub restored: u64,
    /// Pages rejected.
    pub rejected: u64,
}

/// What an import produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOutcome {
    /// The job ID.
    pub job_id: u64,
    /// The counts.
    pub counts: ImportCounts,
    /// The rejects, in order.
    pub rejects: Vec<Reject>,
    /// What each page came to, in the file's order, for the surface's report.
    pub pages: Vec<PageReport>,
}

/// What one page came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageReport {
    /// The namespace number.
    pub ns: i32,
    /// The title within the namespace.
    pub title: String,
    /// The page's ID: minted when created, the existing page's when merged.
    pub page_id: u64,
    /// Whether the job created the page.
    pub created: bool,
    /// Revisions written.
    pub revisions: u64,
    /// Revisions the page already held.
    pub present: u64,
    /// Whether the page's own newest revision was restored on top.
    pub restored: bool,
}

/// The content part of a `create` or `edit`, as `scatter-pages` encodes it (10 §3.2):
/// the source fields flattened in, empty ones left out.
fn content(
    op: &str,
    page: Option<(&ImportPage, &str)>,
    r: &ImportRevision,
    model_change: Option<&str>,
    in_file: bool,
) -> serde_json::Value {
    let mut c = serde_json::Map::new();
    c.insert("op".into(), op.into());
    if let Some((page, model)) = page {
        c.insert("ns".into(), page.ns.into());
        c.insert("title".into(), page.title.clone().into());
        c.insert("model".into(), model.into());
    } else if let Some(m) = model_change {
        c.insert("model".into(), m.into());
    }
    if r.minor {
        c.insert("minor".into(), true.into());
    }
    c.insert("source_revid".into(), r.source_revid.into());
    if let Some(p) = r.source_parentid.filter(|_| in_file) {
        c.insert("source_parentid".into(), p.into());
    }
    c.insert("source_time".into(), r.source_time.clone().into());
    if !r.withheld.is_empty() {
        c.insert("withheld".into(), r.withheld.clone().into());
    }
    serde_json::Value::Object(c)
}

/// Runs an import: each page in its own unit of work, the job's start and finish records
/// in their own. Returns what it wrote; a failure leaves the job `running` with the pages
/// committed so far, and a second run skips them (the skip rule above).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_import<S: IngestStore, H: ImportHost<S>>(
    store: &S,
    pipeline: &Pipeline<S>,
    host: &H,
    tenant: &str,
    attestation: Attestation,
    import: Import,
    pages: impl IntoIterator<Item = ImportPage>,
    now: u64,
) -> Result<ImportOutcome, IngestError> {
    let mut cx = store.begin().await?;
    let mut header = JobHeader::local(&import.source);
    header.version.clone_from(&import.version);
    header.mode = Some(Mode::Import);
    header.graph = "pages".into();
    header.params.insert(
        "history".into(),
        if import.history { "full" } else { "current" }.into(),
    );
    if let Some(summary) = &import.summary {
        header
            .params
            .insert("summary".into(), summary.clone().into());
    }
    let mut job = match import.job_id {
        Some(id) => {
            Job::start_as(
                store,
                pipeline,
                &mut cx,
                tenant,
                id,
                header,
                attestation,
                now,
            )
            .await?
        }
        None => Job::start(store, pipeline, &mut cx, tenant, header, attestation, now).await?,
    };
    store.commit(cx).await?;

    let job_attestation = job.record_attestation();
    let mut counts = ImportCounts::default();
    let mut rejects = Vec::new();
    let mut reports = Vec::new();
    let mut surrogates: BTreeMap<(String, String), String> = BTreeMap::new();
    for (i, page) in pages.into_iter().enumerate() {
        let mut cx = store.begin().await?;
        let outcome = import_page(
            store,
            pipeline,
            host,
            &mut cx,
            tenant,
            &job_attestation,
            &page,
            &mut surrogates,
            now,
        )
        .await;
        match outcome {
            Ok(PageOutcome::Created {
                page_id,
                revisions,
                present,
            }) => {
                store.commit(cx).await?;
                counts.created += 1;
                counts.revisions += revisions;
                counts.present += present;
                reports.push(PageReport {
                    ns: page.ns,
                    title: page.title.clone(),
                    page_id,
                    created: true,
                    revisions,
                    present,
                    restored: false,
                });
            }
            Ok(PageOutcome::Merged {
                page_id,
                revisions,
                present,
                restored,
            }) => {
                store.commit(cx).await?;
                counts.merged += 1;
                counts.revisions += revisions;
                counts.present += present;
                counts.restored += u64::from(restored);
                reports.push(PageReport {
                    ns: page.ns,
                    title: page.title.clone(),
                    page_id,
                    created: false,
                    revisions,
                    present,
                    restored,
                });
            }
            Ok(PageOutcome::Rejected(reason)) => {
                store.rollback(cx).await?;
                counts.rejected += 1;
                let r = Reject {
                    line: i + 1,
                    match_key: None,
                    reason,
                };
                job.reject(r.clone());
                rejects.push(r);
            }
            Err(e) => {
                let _ = store.rollback(cx).await;
                if let Ok(mut cx) = store.begin().await {
                    let _ = job
                        .fail(store, pipeline, &mut cx, &e.to_string(), now)
                        .await;
                    let _ = store.commit(cx).await;
                }
                return Err(e);
            }
        }
    }
    let mut cx = store.begin().await?;
    let mut extra = serde_json::Map::new();
    extra.insert(
        "import".into(),
        serde_json::to_value(&counts).map_err(|e| e.to_string())?,
    );
    let counts_record = crate::job::Counts {
        created: counts.created,
        merged: counts.merged,
        unchanged: counts.present,
        rejected: counts.rejected,
        other: counts.restored,
        ..Default::default()
    };
    job.finish(store, pipeline, &mut cx, &counts_record, extra, now)
        .await?;
    store.commit(cx).await?;
    Ok(ImportOutcome {
        job_id: job.id,
        counts,
        rejects,
        pages: reports,
    })
}

/// What one page came to.
enum PageOutcome {
    Created {
        page_id: u64,
        revisions: u64,
        present: u64,
    },
    Merged {
        page_id: u64,
        revisions: u64,
        present: u64,
        restored: bool,
    },
    Rejected(String),
}

/// Writes one page's revisions, in timestamp order, creating the page or merging into
/// the one its title names. The revisions are decided one after another, from what the
/// page held before the job, and written together as one block ([`write_raw_many`];
/// 0013 A91), the restoring edit last.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn import_page<S: IngestStore, H: ImportHost<S>>(
    store: &S,
    pipeline: &Pipeline<S>,
    host: &H,
    cx: &mut S::Cx,
    tenant: &str,
    job_attestation: &Attestation,
    page: &ImportPage,
    surrogates: &mut BTreeMap<(String, String), String>,
    now: u64,
) -> Result<PageOutcome, IngestError> {
    if page.revisions.is_empty() {
        return Ok(PageOutcome::Rejected(format!(
            "`{}` (ns {}) has no revisions",
            page.title, page.ns
        )));
    }
    let mut revisions: Vec<&ImportRevision> = page.revisions.iter().collect();
    revisions.sort_by_key(|r| (r.time, r.source_revid));
    let in_file: Vec<u64> = revisions.iter().map(|r| r.source_revid).collect();
    let held = host
        .existing(store, cx, tenant, page.ns, &page.title)
        .await?;

    let (key, page_id, mut model, mut newest_held) = match &held {
        Some(h) => (
            Some(h.page_id.to_string()),
            Some(h.page_id),
            Some(h.model.clone()),
            h.revisions.last().map(|r| r.time),
        ),
        None => (None, None, None, None),
    };
    let mut written = 0u64;
    let mut present = 0u64;
    let mut last_written_time = None;
    let mut block: Vec<(Request, RawRecord)> = Vec::new();
    for r in revisions {
        if let Some(h) = &held
            && h.revisions
                .iter()
                .any(|held| held.time == r.time && held.sha1 == r.sha1)
        {
            present += 1;
            continue;
        }
        let actor = match &r.author {
            Author::Job => Some(job_attestation.actor.clone()),
            Author::Actor(k) => Some(k.clone()),
            Author::Hidden => None,
            Author::Surrogate { kind, raw } => Some(
                surrogate(
                    store,
                    pipeline,
                    cx,
                    tenant,
                    (kind, raw),
                    surrogates,
                    job_attestation,
                    now,
                )
                .await?,
            ),
        };
        let parent_in_file = r.source_parentid.is_some_and(|p| in_file.contains(&p));
        let content = match &model {
            None => content("create", Some((page, &r.model)), r, None, parent_in_file),
            Some(m) => content(
                "edit",
                None,
                r,
                (m != &r.model).then_some(r.model.as_str()),
                parent_in_file,
            ),
        };
        let mut request = Request::new(tenant, job_attestation.as_actor(actor.as_deref()), now);
        request.budget = Budget::NONE;
        request.comment = r.comment.clone().filter(|c| !c.is_empty());
        block.push((
            request,
            RawRecord {
                graph: "pages".into(),
                payload_type: PAYLOAD_PAGE.into(),
                key: key.clone(),
                page_id,
                content: scatter_log::cbor::Value::from_json(&content),
                extra: r
                    .text
                    .as_deref()
                    .map(scatter_log::cbor::Value::text)
                    .into_iter()
                    .collect(),
            },
        ));
        model = Some(r.model.clone());
        written += 1;
        last_written_time = Some(r.time);
        // A write the newest held revision no longer leads.
        if newest_held.is_some_and(|t| t < r.time) {
            newest_held = None;
        }
    }
    let Some(h) = held else {
        // A new page's ID is the tenant's next, and its records are keyed by it
        // (10 §3.1). A page of one revision leaves that to the write, as a record with
        // no key; one of several takes the ID first, for all of them.
        if block.len() > 1 {
            let id = store.next_id(cx, tenant, &Sequence::Page).await?;
            for (_, raw) in &mut block {
                raw.key = Some(id.to_string());
                raw.page_id = Some(id);
            }
        }
        let records = write_page(store, pipeline, cx, tenant, &block).await?;
        let page_id = records.first().and_then(|r| r.header().page_id);
        return Ok(PageOutcome::Created {
            page_id: page_id.unwrap_or_default(),
            revisions: written,
            present,
        });
    };
    // The newest revision by timestamp becomes the current one (0008 A48): when the
    // file's revisions all went behind the one the page already had, that one is appended
    // again, by the job.
    let restored = match (newest_held, last_written_time) {
        (Some(_), Some(_)) => {
            let mut request = Request::new(tenant, job_attestation.clone(), now);
            request.budget = Budget::NONE;
            request.comment = Some(RESTORE_COMMENT.into());
            let mut c = serde_json::Map::new();
            c.insert("op".into(), "edit".into());
            if model.as_deref() != Some(h.model.as_str()) {
                c.insert("model".into(), h.model.clone().into());
            }
            block.push((
                request,
                RawRecord {
                    graph: "pages".into(),
                    payload_type: PAYLOAD_PAGE.into(),
                    key: key.clone(),
                    page_id,
                    content: scatter_log::cbor::Value::from_json(&serde_json::Value::Object(c)),
                    extra: h
                        .current_text
                        .as_deref()
                        .map(scatter_log::cbor::Value::text)
                        .into_iter()
                        .collect(),
                },
            ));
            true
        }
        _ => false,
    };
    write_page(store, pipeline, cx, tenant, &block).await?;
    Ok(PageOutcome::Merged {
        page_id: h.page_id,
        revisions: written,
        present,
        restored,
    })
}

/// Writes a page's records to the tenant's `pages` partition as one block
/// ([`write_raw_many`]): what [`crate::write::write_raw`] writes for each in turn.
async fn write_page<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    tenant: &str,
    block: &[(Request, RawRecord)],
) -> Result<Vec<scatter_log::record::Record>, IngestError> {
    if block.is_empty() {
        return Ok(Vec::new());
    }
    let partition =
        store
            .partition(cx, tenant, "pages")
            .await?
            .ok_or_else(|| IngestError::NoPartition {
                tenant: tenant.to_string(),
                graph: "pages".into(),
            })?;
    write_raw_many(
        store,
        pipeline,
        cx,
        partition,
        &scatter_wikibase_changeset::Graph::Pages,
        block,
        Budget::NONE,
    )
    .await
}
