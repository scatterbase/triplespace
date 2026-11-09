//! The talk pages of an adopted or imported wiki, converted comment by comment into
//! threads (0090 §10; 0035 A13; 0008 §9): a talk page's revisions are replayed through a
//! line-level blame, its current text split into comments, each post attributed to the
//! account that wrote it (0090 §6–§7), and the planned records written through the one
//! thread writer as an import (14 §1.2).
//!
//! The adoption job writes entity and document page revisions, and the import job writes
//! document pages (`crates/scatter-ingest`); this runs after either, in the same command
//! or request, because the thread writer lives above the ingester (22 §1.2). A talk page
//! is converted once: a page whose threads already carry its `imported_from.page` is
//! skipped, so a re-run converts nothing twice.
//!
//! The home of a converted page is its subject's talk page: an actor target for `User
//! talk:Name` ([`Target::Actor`], 0019 §1), and the subject page's ID for every other talk
//! namespace. A subject the job did not write (a user with no account, a page it deferred)
//! leaves the talk page deferred, with the reason counted.
//!
//! Who a comment's author is depends on where the page came from ([`Authors`]): an
//! adopted wiki's accounts are the tenant's own, under its issuer (0035 §5); an imported
//! page's authors are a registered issuer's accounts where the source is one, and
//! `imported` surrogates otherwise (0007 §5), minted here as the import job mints them.

use std::collections::BTreeMap;

use scatter_actors::ActorKey;
use scatter_ingest::IngestStore;
use scatter_ingest::adopt::Author;
use scatter_ingest::import::surrogate;
use scatter_ingest::write::{Attestation, Request};
use scatter_log_postgres::PgClient;
use scatter_mediawiki_xml::{DumpPage, Revision as DumpRevision};
use scatter_projection::{Backend, Pipeline};
use scatter_talk::{Blame, Conversion, Markup, Options, Revision, Site, Stamp, attribute, convert};
use scatter_talk_import::{PlannedThread, Source, plan};
use scatter_threads::{Limits, Target, Writer};
use scatter_titles::Namespaces;
use triplespace_projections::PgCx;
use triplespace_projections::threads::{read_record, target_columns};

use crate::PgIngest;
use crate::threads::{ThreadError, ThreadWrite, write};

/// Why a talk page could not be converted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TalkError {
    /// The thread writer refused.
    #[error(transparent)]
    Thread(#[from] ThreadError),
    /// The store refused.
    #[error(transparent)]
    Ingest(#[from] scatter_ingest::IngestError),
    /// The database refused.
    #[error("database: {0}")]
    Db(#[from] tokio_postgres::Error),
    /// The log refused.
    #[error(transparent)]
    Log(#[from] scatter_log::store::StoreError),
    /// A projection, or the unit of work, failed.
    #[error(transparent)]
    Projection(#[from] scatter_projection::ProjectionError),
    /// A record could not be read.
    #[error("{0}")]
    Record(String),
}

/// How the conversion of one talk page ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkOutcome {
    /// Converted: how many threads and posts were written.
    Converted {
        /// Threads created, the header included.
        threads: u64,
        /// Records written, `create`s included.
        posts: u64,
    },
    /// Already converted by an earlier run.
    Present,
    /// Not converted, and why.
    Deferred(String),
}

/// The counts of a run.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TalkCounts {
    /// Pages converted.
    pub pages: u64,
    /// Pages already present.
    pub present: u64,
    /// Threads created.
    pub threads: u64,
    /// Records written.
    pub posts: u64,
    /// Pages deferred, by reason.
    pub deferred: BTreeMap<String, u64>,
}

impl TalkCounts {
    fn add(&mut self, outcome: &TalkOutcome) {
        match outcome {
            TalkOutcome::Converted { threads, posts } => {
                self.pages += 1;
                self.threads += threads;
                self.posts += posts;
            }
            TalkOutcome::Present => self.present += 1,
            TalkOutcome::Deferred(why) => *self.deferred.entry(why.clone()).or_default() += 1,
        }
    }
}

/// Whether a dump page is a talk page this run converts: a talk namespace whose model is
/// `triplespace-talk` here (0008 §9), carrying wikitext on the source.
#[must_use]
pub fn is_talk_page(page: &DumpPage) -> bool {
    let Ok(ns) = i32::try_from(page.ns) else {
        return false;
    };
    let Some(namespace) = Namespaces::embedded().get(ns) else {
        return false;
    };
    namespace.default_model.as_deref() == Some("triplespace-talk")
        && page
            .revisions
            .last()
            .and_then(|r| r.model.as_deref())
            .is_none_or(|m| m == "wikitext")
}

/// Who the source's named authors are here.
#[derive(Debug, Clone)]
pub enum Authors<'a> {
    /// An adopted wiki's: the tenant's own accounts under its issuer, by the source's user
    /// ID; a name the survey did not see is credited to the job (0090 §6).
    Adopted {
        /// The tenant's issuer code.
        issuer: &'a str,
        /// The source's accounts, user ID → name.
        accounts: &'a BTreeMap<u64, String>,
    },
    /// An imported file's (0008 §9; 0007 §5): a registered issuer's accounts by the
    /// source's user ID where the source is one, else `imported` surrogates under the
    /// import's prefix, as the import job attributes its page revisions.
    Imported {
        /// The registered issuer the source is, if any.
        issuer: Option<&'a str>,
        /// The source's accounts as the file names them, name → user ID.
        accounts: &'a BTreeMap<String, u64>,
        /// The interwiki prefix an imported name carries: `imported` for an upload, the
        /// site alias otherwise.
        prefix: &'a str,
    },
}

impl Authors<'_> {
    /// The author a name resolves to.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Author {
        match self {
            Authors::Adopted { issuer, accounts } => accounts
                .iter()
                .find(|(_, n)| n.as_str() == name)
                .map_or(Author::Job, |(id, _)| {
                    Author::Actor(ActorKey::numeric(issuer, *id).to_string())
                }),
            Authors::Imported {
                issuer,
                accounts,
                prefix,
            } => match (issuer, accounts.get(name)) {
                (Some(issuer), Some(id)) => {
                    Author::Actor(ActorKey::numeric(issuer, *id).to_string())
                }
                _ => Author::Surrogate {
                    kind: "imported".into(),
                    raw: format!("{prefix}>{name}"),
                },
            },
        }
    }
}

/// What the conversion needs of the job: the tenant, the source wiki, who its authors
/// are, and who runs it.
pub struct TalkContext<'a> {
    /// The tenant slug.
    pub tenant: &'a str,
    /// The source wiki's base URL or name, for `imported_from.repository`.
    pub source: &'a str,
    /// The source's authors.
    pub authors: Authors<'a>,
    /// The subsidiary running the job, whose posts are those nobody else can be credited for.
    pub actor: &'a str,
    /// The job ID, carried on every record's attestation.
    pub job: Option<u64>,
    /// The site, for signature parsing (0090 §5).
    pub site: &'a Site,
}

/// The actor key of an author, minting a surrogate's record where it has none yet.
async fn actor_key(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    ctx: &TalkContext<'_>,
    author: Author,
    surrogates: &mut BTreeMap<(String, String), String>,
    now: u64,
) -> Result<Option<String>, TalkError> {
    Ok(match author {
        Author::Job => Some(ctx.actor.to_string()),
        Author::Actor(key) => Some(key),
        Author::Hidden => None,
        Author::Surrogate { kind, raw } => {
            let mut attestation = Attestation::by(ctx.actor);
            attestation.job = ctx.job;
            Some(
                surrogate(
                    store,
                    pipeline,
                    cx,
                    ctx.tenant,
                    (&kind, &raw),
                    surrogates,
                    &attestation,
                    now,
                )
                .await?,
            )
        }
    })
}

/// Converts one talk page and writes its threads. The caller commits.
///
/// # Errors
///
/// When the store refuses a write; the caller rolls back.
pub async fn convert_page(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    ctx: &TalkContext<'_>,
    page: &DumpPage,
    surrogates: &mut BTreeMap<(String, String), String>,
    now: u64,
) -> Result<TalkOutcome, TalkError> {
    let markup = Markup::embedded();
    let Some(current) = page.latest() else {
        return Ok(TalkOutcome::Deferred("no revisions".into()));
    };
    if current.text_hidden {
        return Ok(TalkOutcome::Deferred(
            "the source hid the current text".into(),
        ));
    }
    if page.redirect.is_some() || is_redirect(&current.text) {
        // A talk page that redirects holds no discussion; threads live on the target,
        // which is converted on its own. Nothing is written for it.
        return Ok(TalkOutcome::Deferred("a redirect".into()));
    }
    let Some(home) = home_of(store, pipeline, cx, ctx, page, surrogates, now).await? else {
        return Ok(TalkOutcome::Deferred(
            "its subject is not here (no account, or a deferred page)".into(),
        ));
    };
    if already_converted(store, cx, ctx.tenant, &home, &page.title).await? {
        return Ok(TalkOutcome::Present);
    }

    // History: the blame over the dump's revisions, oldest first (0090 §7).
    let mut revisions: Vec<&DumpRevision> = page.revisions.iter().collect();
    revisions.sort_by_key(|r| r.id);
    let mut blame = Blame::new(markup, ctx.site);
    for r in &revisions {
        if r.text_hidden {
            continue;
        }
        let Some(stamp) = stamp_of(&r.timestamp) else {
            continue;
        };
        blame.push(
            Revision {
                id: r.id,
                actor: actor_name(r),
                timestamp: stamp,
            },
            &r.text,
        );
    }

    // The current text, split and attributed.
    let text = &current.text;
    let mut conv: Conversion = convert(text, markup, ctx.site, &Options::default());
    attribute(&mut conv, text, Some(&blame), "", markup, ctx.site);

    let source = Source {
        repository: Some(ctx.source.to_string()),
        page: page.title.clone(),
        revision: Some(current.id),
        archive: None,
        revision_time: stamp_of(&current.timestamp),
    };
    let planned = plan(&conv, &page.title, &source);
    let (mut threads, mut posts) = (0u64, 0u64);
    for t in &planned {
        posts += write_thread(store, pipeline, cx, ctx, t, &home, surrogates, now).await?;
        threads += 1;
    }
    Ok(TalkOutcome::Converted { threads, posts })
}

/// Writes one planned thread's records in order (0090 §8), returning how many.
#[allow(clippy::too_many_arguments)]
async fn write_thread(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    ctx: &TalkContext<'_>,
    t: &PlannedThread,
    home: &Target,
    surrogates: &mut BTreeMap<(String, String), String>,
    now: u64,
) -> Result<u64, TalkError> {
    let date = t.date.map(|d| {
        let (y, m, dd, _, _) = d.civil();
        format!("{y:04}-{m:02}-{dd:02}")
    });
    let (mut thread, mut revids) = (None, Vec::new());
    for (k, r) in t.records.iter().enumerate() {
        // The author: the account the attribution named, else the job (0090 §6).
        let author = r
            .author
            .as_deref()
            .filter(|n| !n.is_empty())
            .map_or(Author::Job, |name| ctx.authors.resolve(name));
        let actor = actor_key(store, pipeline, cx, ctx, author, surrogates, now)
            .await?
            .unwrap_or_else(|| ctx.actor.to_string());
        let mut attestation = Attestation::by(&actor);
        attestation.job = ctx.job;
        let request = Request::new(ctx.tenant, attestation, now);
        let w = ThreadWrite {
            thread,
            op: t.op(k, home, &revids),
            text: Some(r.text.clone()),
            writer: Writer::Import,
            limits: Limits::default(),
            date: date.clone(),
            signer: None,
        };
        let out = write(store, pipeline, cx, &request, w).await?;
        thread = Some(out.thread);
        revids.push(out.revid);
    }
    Ok(u64::try_from(t.records.len()).unwrap_or(u64::MAX))
}

/// Converts every talk page given, each in its own transaction. Pages are read from the
/// dump a second time, after the job, so that every subject page and account the threads
/// need is already there. A page whose write fails is deferred with the error, which is
/// also written to stderr, and the rest go on.
pub async fn convert_all(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    ctx: &TalkContext<'_>,
    pages: impl Iterator<Item = DumpPage>,
    now: u64,
) -> Result<TalkCounts, TalkError> {
    let mut counts = TalkCounts::default();
    let mut surrogates = BTreeMap::new();
    for page in pages {
        let mut cx = Backend::begin(store).await?;
        let outcome =
            match convert_page(store, pipeline, &mut cx, ctx, &page, &mut surrogates, now).await {
                Ok(o) => o,
                Err(e) => {
                    let _ = Backend::rollback(store, cx).await;
                    eprintln!("deferring talk page {} (`{}`): {e}", page.id, page.title);
                    counts.add(&TalkOutcome::Deferred(format!("the write failed: {e}")));
                    continue;
                }
            };
        if let TalkOutcome::Deferred(why) = &outcome {
            let _ = Backend::rollback(store, cx).await;
            eprintln!("deferring talk page {} (`{}`): {why}", page.id, page.title);
        } else {
            Backend::commit(store, cx).await?;
        }
        counts.add(&outcome);
    }
    Ok(counts)
}

/// The home target of a talk page: the actor for a user talk page, the subject page's ID
/// otherwise; `None` when the subject is not here.
async fn home_of(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    ctx: &TalkContext<'_>,
    page: &DumpPage,
    surrogates: &mut BTreeMap<(String, String), String>,
    now: u64,
) -> Result<Option<Target>, TalkError> {
    let ns = i32::try_from(page.ns).unwrap_or(0);
    let namespaces = Namespaces::embedded();
    if namespaces.get(ns).is_none() {
        return Ok(None);
    }
    let subject_ns = ns - 1;
    let (_, name) = page.title.split_once(':').unwrap_or(("", &page.title));
    if subject_ns == 2 {
        // User talk: the account's actor key, where the account is here; a subpage of
        // a user talk page has no home of its own.
        let root = name.split('/').next().unwrap_or(name);
        if name != root {
            return Ok(None);
        }
        let author = match &ctx.authors {
            Authors::Adopted { .. } => match ctx.authors.resolve(root) {
                Author::Job => return Ok(None),
                a => a,
            },
            Authors::Imported { .. } => ctx.authors.resolve(root),
        };
        let Some(key) = actor_key(store, pipeline, cx, ctx, author, surrogates, now).await? else {
            return Ok(None);
        };
        let present = cx
            .query_opt(
                "SELECT 1 FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                &[&ctx.tenant, &key.as_str()],
            )
            .await?
            .is_some();
        return Ok(present.then_some(Target::Actor(key)));
    }
    let subject_title = if subject_ns == 0 {
        name.to_string()
    } else {
        match namespaces.get(subject_ns) {
            Some(s) => format!("{}:{name}", s.canonical),
            None => return Ok(None),
        }
    };
    let row = cx
        .query_opt(
            "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted",
            &[&ctx.tenant, &subject_ns, &subject_title.as_str()],
        )
        .await?;
    Ok(row.map(|r| Target::Page(u64::try_from(r.get::<_, i64>(0)).unwrap_or(0))))
}

/// Whether a thread on `home` already says it was imported from this talk page: the
/// `imported_from.page` of its `create` record (0090 §8).
async fn already_converted(
    store: &PgIngest,
    cx: &mut PgCx,
    tenant: &str,
    home: &Target,
    title: &str,
) -> Result<bool, TalkError> {
    let Some(partition) = store.partition(cx, tenant, "pages").await? else {
        return Ok(false);
    };
    let (kind, id) = target_columns(home);
    let rows = cx
        .query(
            "SELECT page_id FROM view.thread WHERE tenant = $1 AND target_kind = $2 AND target_id = $3",
            &[&tenant, &kind, &id.as_str()],
        )
        .await?;
    for row in rows {
        let thread: i64 = row.get(0);
        let records =
            scatter_log_postgres::log::read_by_key(cx, partition, &thread.to_string()).await?;
        let Some((_, first)) = records.first() else {
            continue;
        };
        if let Some(rec) = read_record(first).map_err(TalkError::Record)?
            && let scatter_threads::ThreadOp::Create {
                imported_from: Some(from),
                ..
            } = &rec.op
            && from.page.as_deref() == Some(title)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether wikitext is a redirect: `#REDIRECT [[…]]`, in any case, with leading space.
fn is_redirect(text: &str) -> bool {
    let head: String = text.trim_start().chars().take(9).collect();
    head.eq_ignore_ascii_case("#redirect")
}

/// The actor name a dump revision carries: the account's name, the IP, or a marker for a
/// hidden one, as the blame compares signatures against it.
fn actor_name(r: &DumpRevision) -> String {
    if let Some((_, name)) = &r.user {
        name.clone()
    } else if let Some(ip) = &r.ip {
        ip.clone()
    } else {
        String::new()
    }
}

/// A dump timestamp (`2026-09-20T14:02:11Z`) as a signature stamp, to the minute.
fn stamp_of(s: &str) -> Option<Stamp> {
    let micros = triplespace_projections::feed::parse_timestamp(s)?;
    Some(Stamp(i64::try_from(micros / 60_000_000).ok()?))
}

/// The accounts a dump names, name → user ID, from its revisions' contributors: what an
/// import's [`Authors::Imported`] resolves against.
#[must_use]
pub fn accounts_of<'a>(pages: impl Iterator<Item = &'a DumpPage>) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    for page in pages {
        for r in &page.revisions {
            if let Some((id, name)) = &r.user
                && *id > 0
            {
                out.entry(name.clone()).or_insert(*id);
            }
        }
    }
    out
}

/// The same from a name → ID map built page by page, for a streaming pass.
pub fn observe_accounts(into: &mut BTreeMap<String, u64>, page: &DumpPage) {
    for r in &page.revisions {
        if let Some((id, name)) = &r.user
            && *id > 0
        {
            into.entry(name.clone()).or_insert(*id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors_resolve_by_origin() {
        let adopted: BTreeMap<u64, String> = [(7, "Harej".to_string())].into();
        let a = Authors::Adopted {
            issuer: "lb",
            accounts: &adopted,
        };
        assert_eq!(a.resolve("Harej"), Author::Actor("lb:7".into()));
        assert_eq!(a.resolve("Nobody"), Author::Job);
        let imported: BTreeMap<String, u64> = [("Harej".to_string(), 7)].into();
        let i = Authors::Imported {
            issuer: None,
            accounts: &imported,
            prefix: "enwiki",
        };
        assert_eq!(
            i.resolve("Harej"),
            Author::Surrogate {
                kind: "imported".into(),
                raw: "enwiki>Harej".into()
            }
        );
        let i = Authors::Imported {
            issuer: Some("enwiki"),
            accounts: &imported,
            prefix: "enwiki",
        };
        assert_eq!(i.resolve("Harej"), Author::Actor("enwiki:7".into()));
        assert_eq!(
            i.resolve("Nobody"),
            Author::Surrogate {
                kind: "imported".into(),
                raw: "enwiki>Nobody".into()
            }
        );
    }
}
