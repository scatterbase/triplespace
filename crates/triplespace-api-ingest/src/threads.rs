//! The one writer of thread records (0019 §1–§4; 14 §1.2–§1.5, §3.6, §4.2), shared by the
//! thread API and the import jobs.
//!
//! [`write`] replays the thread's records into its fold, checks the new record against it
//! with what only the store knows (whether a target exists, how many threads are pinned on
//! a page), mints what the server mints (the thread's page ID, its title, and a talk page
//! ID for each target that has none yet) into the attestation map, and appends the record
//! with the base-offset check, projecting it in the same unit of work. The caller commits
//! or rolls back.
//!
//! Statuses are the registry's defaults; a tenant's own `thread-status` records are not read
//! yet. Boards do not exist yet, so every page takes homes.

use std::collections::{BTreeSet, HashMap};

use scatter_ingest::store::Sequence;
use scatter_ingest::{
    IngestError, IngestStore, MintedValue, RawRecord, Request, Written, write_raw,
};
use scatter_log::cbor::Value;
use scatter_log_postgres::PgClient;
use scatter_projection::Pipeline;
use scatter_threads::{
    Check, Invalid, Limits, Minted, NS_THREAD, PAYLOAD_THREAD, Pages, Statuses, Target, Thread,
    ThreadOp, Writer, mint_title, normalize_subject,
};
use triplespace_projections::PgCx;
use triplespace_projections::threads::{own_talk_page, read_record, target_columns};

use crate::PgIngest;

/// Page IDs at and above this are provider-ranged (0015 §2): a mirrored entity's talk page
/// exists without a local record (14 §1.3).
const PROVIDER_RANGED: u64 = 1 << 40;

/// One thread write.
#[derive(Debug, Clone)]
pub struct ThreadWrite {
    /// The thread's page ID; `None` for a `create`.
    pub thread: Option<u64>,
    /// The operation.
    pub op: ThreadOp,
    /// The text part, for `create`, `post` and `edit`.
    pub text: Option<String>,
    /// Who is writing.
    pub writer: Writer,
    /// The tenant's limits.
    pub limits: Limits,
    /// A `create`'s title date, `YYYY-MM-DD`, when an import supplies it (0054 §5); the
    /// request's own UTC date otherwise.
    pub date: Option<String>,
}

/// What was written.
#[derive(Debug)]
pub struct ThreadWritten {
    /// The thread's page ID.
    pub thread: u64,
    /// The record's revision ID: a post's identity.
    pub revid: u64,
    /// The append.
    pub written: Written,
}

/// Why a thread write failed. The caller rolls back.
#[derive(Debug, thiserror::Error)]
pub enum ThreadError {
    /// The record does not fit the thread.
    #[error("{0}")]
    Invalid(Invalid),
    /// No thread has that page ID.
    #[error("no thread has page ID {0}")]
    NoThread(u64),
    /// The operation needs a base offset and the request has none (14 §1.2).
    #[error("`{0}` needs a base offset")]
    NeedsBase(&'static str),
    /// The thread changed since the request's base offset.
    #[error("the thread changed: its newest record is at offset {latest}")]
    Conflict {
        /// The newest record's offset.
        latest: u64,
    },
    /// The store refused.
    #[error("store: {0}")]
    Store(String),
}

impl From<IngestError> for ThreadError {
    fn from(e: IngestError) -> Self {
        match e {
            IngestError::Conflict { latest, .. } => ThreadError::Conflict { latest },
            other => ThreadError::Store(other.to_string()),
        }
    }
}

fn store_err(e: impl std::fmt::Display) -> ThreadError {
    ThreadError::Store(e.to_string())
}

/// The answers [`Pages`] needs for the targets one record names.
#[derive(Default)]
struct Facts {
    exists: HashMap<Target, bool>,
    pinned: HashMap<Target, usize>,
}

impl Pages for Facts {
    fn exists(&self, target: &Target) -> bool {
        self.exists.get(target).copied().unwrap_or(false)
    }
    fn takes_homes(&self, _: &Target) -> bool {
        true
    }
    fn pinned_on(&self, target: &Target) -> usize {
        self.pinned.get(target).copied().unwrap_or(0)
    }
}

fn targets(op: &ThreadOp) -> Vec<&Target> {
    match op {
        ThreadOp::Create { target, also, .. } => std::iter::once(target).chain(also).collect(),
        ThreadOp::Move { target, .. }
        | ThreadOp::Attach { target }
        | ThreadOp::Detach { target }
        | ThreadOp::Pin { target }
        | ThreadOp::Unpin { target } => vec![target],
        ThreadOp::Post { .. } | ThreadOp::Edit { .. } | ThreadOp::Rename { .. } => Vec::new(),
    }
}

/// The talk page ID a target has, if one was minted.
async fn talk_of(cx: &PgCx, tenant: &str, target: &Target) -> Result<Option<u64>, ThreadError> {
    let (kind, id) = target_columns(target);
    let row = cx
        .query_opt(
            "SELECT page_id FROM view.talk_page WHERE tenant = $1 AND target_kind = $2 AND target_id = $3",
            &[&tenant, &kind, &id],
        )
        .await
        .map_err(store_err)?;
    Ok(row.and_then(|r| u64::try_from(r.get::<_, i64>(0)).ok()))
}

async fn facts(cx: &PgCx, tenant: &str, op: &ThreadOp) -> Result<Facts, ThreadError> {
    let mut f = Facts::default();
    for t in targets(op) {
        let exists = match t {
            Target::Page(id) if *id >= PROVIDER_RANGED => true,
            Target::Page(id) => {
                let id = i64::try_from(*id).map_err(store_err)?;
                cx.query_opt(
                    "SELECT 1 FROM view.page WHERE tenant = $1 AND page_id = $2 AND NOT deleted",
                    &[&tenant, &id],
                )
                .await
                .map_err(store_err)?
                .is_some()
            }
            Target::Actor(key) => cx
                .query_opt(
                    "SELECT 1 FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                    &[&tenant, key],
                )
                .await
                .map_err(store_err)?
                .is_some(),
        };
        f.exists.insert(t.clone(), exists);
        let pinned = match talk_of(cx, tenant, t).await? {
            Some(talk) => {
                let talk = i64::try_from(talk).map_err(store_err)?;
                let n: i64 = cx
                    .query_one(
                        "SELECT count(*) FROM view.thread_attachment
                         WHERE tenant = $1 AND talk_page_id = $2 AND pinned IS NOT NULL",
                        &[&tenant, &talk],
                    )
                    .await
                    .map_err(store_err)?
                    .get(0);
                usize::try_from(n).unwrap_or(usize::MAX)
            }
            None => 0,
        };
        f.pinned.insert(t.clone(), pinned);
    }
    Ok(f)
}

/// The UTC date of a time in µs since the epoch, `YYYY-MM-DD`.
#[must_use]
pub fn utc_date(micros: u64) -> String {
    let days = i64::try_from(micros / 86_400_000_000).unwrap_or(0);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Mints a title in the `Thread` namespace that no live page holds.
/// `own` is the thread's current title, which a rename may keep.
async fn title(
    cx: &PgCx,
    tenant: &str,
    date: &str,
    subject: &str,
    own: Option<&str>,
) -> Result<String, ThreadError> {
    let base = format!("{date}/{subject}");
    let pattern = format!(
        "{} (%)",
        base.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let taken: BTreeSet<String> = cx
        .query(
            "SELECT title FROM view.page WHERE tenant = $1 AND ns = $2 AND NOT deleted
               AND (title = $3 OR title LIKE $4)",
            &[&tenant, &NS_THREAD, &base, &pattern],
        )
        .await
        .map_err(store_err)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    Ok(mint_title(date, subject, |t| {
        taken.contains(t) && Some(t) != own
    }))
}

async fn talk_or_mint(
    store: &PgIngest,
    cx: &mut PgCx,
    tenant: &str,
    target: &Target,
) -> Result<u64, ThreadError> {
    if let Some(t) = talk_of(cx, tenant, target).await? {
        return Ok(t);
    }
    // A thread or a board is its own talk page.
    if let Target::Page(id) = target
        && let Ok(page) = i64::try_from(*id)
        && let Some(row) = cx
            .query_opt(
                "SELECT ns FROM view.page WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &page],
            )
            .await
            .map_err(store_err)?
        && own_talk_page(row.get(0))
    {
        return Ok(*id);
    }
    Ok(store.next_id(cx, tenant, &Sequence::Page).await?)
}

/// Mints what the record needs: the page ID and title of a new thread, a renamed thread's
/// title, and the talk page of the target a record names.
async fn mint(
    store: &PgIngest,
    cx: &mut PgCx,
    tenant: &str,
    now: u64,
    w: &ThreadWrite,
    thread: Option<&Thread>,
) -> Result<(Option<u64>, Minted), ThreadError> {
    let mut page_id = w.thread;
    let mut minted = Minted::default();
    match &w.op {
        ThreadOp::Create {
            name, target, also, ..
        } => {
            page_id = Some(store.next_id(cx, tenant, &Sequence::Page).await?);
            let subject =
                normalize_subject(name).map_err(|e| ThreadError::Invalid(Invalid::Subject(e)))?;
            let date = w.date.clone().unwrap_or_else(|| utc_date(now));
            minted.title = Some(title(cx, tenant, &date, &subject, None).await?);
            minted.talk = Some(talk_or_mint(store, cx, tenant, target).await?);
            for t in also {
                let id = talk_or_mint(store, cx, tenant, t).await?;
                minted.also.push(id);
            }
        }
        ThreadOp::Rename { name } => {
            let subject =
                normalize_subject(name).map_err(|e| ThreadError::Invalid(Invalid::Subject(e)))?;
            let date = thread
                .and_then(|t| t.title.as_deref())
                .and_then(|t| t.split_once('/'))
                .map_or_else(|| utc_date(now), |(d, _)| d.to_string());
            let own = thread.and_then(|t| t.title.as_deref());
            minted.title = Some(title(cx, tenant, &date, &subject, own).await?);
        }
        ThreadOp::Move { target, .. } | ThreadOp::Attach { target } => {
            minted.talk = Some(talk_or_mint(store, cx, tenant, target).await?);
        }
        ThreadOp::Detach { target } | ThreadOp::Pin { target } | ThreadOp::Unpin { target } => {
            minted.talk = talk_of(cx, tenant, target).await?;
        }
        ThreadOp::Post { .. } | ThreadOp::Edit { .. } => {}
    }
    Ok((page_id, minted))
}

/// Writes one thread record (see the module documentation).
///
/// # Errors
///
/// A [`ThreadError`]; the caller rolls back.
pub async fn write(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    request: &Request,
    w: ThreadWrite,
) -> Result<ThreadWritten, ThreadError> {
    let tenant = request.tenant.as_str();
    let partition = store
        .partition(cx, tenant, "pages")
        .await?
        .ok_or_else(|| ThreadError::Store(format!("`{tenant}` has no pages partition")))?;
    if w.text.is_some() != w.op.has_text() {
        return Err(ThreadError::Invalid(Invalid::Text));
    }
    if w.op.needs_base() && request.base_offset.is_none() {
        return Err(ThreadError::NeedsBase(w.op.name()));
    }

    // The thread as its records leave it.
    let thread = match w.thread {
        None => None,
        Some(id) => {
            let records = scatter_log_postgres::log::read_by_key(&*cx, partition, &id.to_string())
                .await
                .map_err(store_err)?;
            let mut folded = Vec::with_capacity(records.len());
            for (_, r) in &records {
                if let Some(rec) = read_record(r).map_err(ThreadError::Store)? {
                    folded.push(rec);
                }
            }
            if folded.is_empty() {
                return Err(ThreadError::NoThread(id));
            }
            Some(Thread::replay(&folded).map_err(ThreadError::Invalid)?)
        }
    };

    // Check the record before minting anything.
    let facts = facts(cx, tenant, &w.op).await?;
    let check = Check {
        statuses: Statuses::embedded(),
        pages: &facts,
        limits: w.limits,
        writer: w.writer,
    };
    let actor = request.attestation.actor.clone();
    let mut rec = scatter_threads::Record {
        revid: u64::MAX,
        actor,
        time: i64::try_from(request.now).unwrap_or(i64::MAX),
        op: w.op.clone(),
        minted: Minted::default(),
        has_text: w.text.is_some(),
    };
    match &thread {
        None => {
            Thread::create(&rec, &check).map_err(ThreadError::Invalid)?;
        }
        Some(t) => t.validate(&rec, &check).map_err(ThreadError::Invalid)?,
    }

    // Mint the page ID, title and talk page IDs.
    let (page_id, minted) = mint(store, cx, tenant, request.now, &w, thread.as_ref()).await?;
    rec.minted = minted;

    let mut request = request.clone();
    let m = &rec.minted;
    if let Some(t) = &m.title {
        request
            .attestation
            .minted
            .push(("title".into(), MintedValue::Text(t.clone())));
    }
    if let Some(t) = m.talk {
        request
            .attestation
            .minted
            .push(("talk".into(), MintedValue::Id(t)));
    }
    if !m.also.is_empty() {
        request
            .attestation
            .minted
            .push(("also".into(), MintedValue::Ids(m.also.clone())));
    }
    let raw = RawRecord {
        graph: "pages".into(),
        payload_type: PAYLOAD_THREAD.into(),
        key: w.thread.map(|id| id.to_string()),
        page_id,
        content: Value::from_json(&w.op.to_json()),
        extra: vec![w.text.as_deref().map_or(Value::Null, Value::text)],
    };
    let written = write_raw(store, pipeline, cx, &request, raw).await?;
    let header = written.record.header();
    Ok(ThreadWritten {
        thread: header
            .page_id
            .ok_or_else(|| store_err("the record has no page ID"))?,
        revid: header
            .revid
            .ok_or_else(|| store_err("the record has no revision ID"))?,
        written,
    })
}
