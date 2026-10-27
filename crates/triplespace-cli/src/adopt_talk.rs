//! The talk pages of an adopted wiki, converted comment by comment into threads within the
//! adoption run (0090 §10; 0035 A13): a talk page's revisions are replayed through a
//! line-level blame, its current text split into comments, each post attributed to the
//! adopted account that wrote it (0090 §6–§7), and the planned records written through
//! the one thread writer as an import (14 §1.2).
//!
//! The adoption job writes entity and document page revisions (`crates/scatter-ingest`);
//! this runs after it, in the same `triplespace adopt`, because the thread writer lives
//! above the ingester (22 §1.2). A talk page is converted once: a page whose threads
//! already carry its `imported_from.page` is skipped, so a re-run adopts nothing twice.
//!
//! The home of a converted page is its subject's talk page: an actor target for `User
//! talk:Name` ([`Target::Actor`], 0019 §1), and the subject page's ID for every other talk
//! namespace. A subject the adoption did not write (a user with no account, a page it
//! deferred) leaves the talk page deferred, with the reason counted.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use scatter_actors::ActorKey;
use scatter_adapter_wikibase::xml_dump::{DumpPage, Revision as DumpRevision};
use scatter_ingest::IngestStore;
use scatter_ingest::write::{Attestation, Request};
use scatter_log_postgres::PgClient;
use scatter_projection::{Backend, Pipeline};
use scatter_talk::{Blame, Conversion, Markup, Options, Revision, Site, Stamp, attribute, convert};
use scatter_talk_import::{PlannedThread, Source, plan};
use scatter_threads::{Limits, Target, Writer};
use scatter_titles::Namespaces;
use triplespace_api_ingest::PgIngest;
use triplespace_api_ingest::threads::{ThreadWrite, write};
use triplespace_projections::PgCx;
use triplespace_projections::threads::{read_record, target_columns};

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
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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

/// What the conversion needs of the adoption: the tenant, the source wiki, the accounts the
/// dump named, and who runs the job.
pub struct TalkContext<'a> {
    /// The tenant slug.
    pub tenant: &'a str,
    /// The tenant's issuer code (0079 §1), which the adopted accounts' keys carry.
    pub issuer: &'a str,
    /// The source wiki's base URL, for `imported_from.repository`.
    pub source: &'a str,
    /// The source's accounts by name, from the survey: name → user ID.
    pub accounts: &'a BTreeMap<u64, String>,
    /// The subsidiary running the job, whose posts are those nobody else can be credited for.
    pub actor: &'a str,
    /// The job ID, carried on every record's attestation.
    pub job: Option<u64>,
    /// The site, for signature parsing (0090 §5).
    pub site: &'a Site,
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
    now: u64,
) -> Result<TalkOutcome> {
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
    let Some(home) = home_of(cx, ctx, page).await? else {
        return Ok(TalkOutcome::Deferred(
            "its subject was not adopted (no account, or a deferred page)".into(),
        ));
    };
    if already_converted(store, cx, ctx.tenant, &home, &page.title).await? {
        return Ok(TalkOutcome::Present);
    }

    // History: the blame over the dump's revisions, oldest first (0090 §7).
    let mut revisions: Vec<&DumpRevision> = page.revisions.iter().collect();
    revisions.sort_by_key(|r| r.id);
    let mut blame = Blame::new(markup, ctx.site);
    let by_name: HashMap<&str, u64> = ctx
        .accounts
        .iter()
        .map(|(id, name)| (name.as_str(), *id))
        .collect();
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
    };
    let planned = plan(&conv, &page.title, &source);
    let (mut threads, mut posts) = (0u64, 0u64);
    for t in &planned {
        posts += write_thread(store, pipeline, cx, ctx, &by_name, t, &home, now).await?;
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
    by_name: &HashMap<&str, u64>,
    t: &PlannedThread,
    home: &Target,
    now: u64,
) -> Result<u64> {
    let date = t.date.map(|d| {
        let (y, m, dd, _, _) = d.civil();
        format!("{y:04}-{m:02}-{dd:02}")
    });
    let (mut thread, mut revids) = (None, Vec::new());
    for (k, r) in t.records.iter().enumerate() {
        // The author: the adopted account the attribution named, else the job (0090 §6).
        let actor = r
            .author
            .as_deref()
            .and_then(|name| by_name.get(name))
            .map_or_else(
                || ctx.actor.to_string(),
                |id| ActorKey::numeric(ctx.issuer, *id).to_string(),
            );
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
        let out = write(store, pipeline, cx, &request, w)
            .await
            .with_context(|| format!("thread `{}`, record {k}", t.subject))?;
        thread = Some(out.thread);
        revids.push(out.revid);
    }
    Ok(u64::try_from(t.records.len()).unwrap_or(u64::MAX))
}

/// Converts every talk page of the dump, each in its own transaction, and prints what it
/// did. Pages are read from the dump a second time, after the adoption job, so that every
/// subject page and account the threads need is already there.
pub async fn convert_all(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    ctx: &TalkContext<'_>,
    pages: impl Iterator<Item = DumpPage>,
    now: u64,
) -> Result<TalkCounts> {
    let mut counts = TalkCounts::default();
    for page in pages {
        let mut cx = Backend::begin(store).await.context("begin")?;
        let outcome = match convert_page(store, pipeline, &mut cx, ctx, &page, now).await {
            Ok(o) => o,
            Err(e) => {
                let _ = Backend::rollback(store, cx).await;
                eprintln!("deferring talk page {} (`{}`): {e:#}", page.id, page.title);
                counts.add(&TalkOutcome::Deferred(
                    "the write failed (see stderr)".into(),
                ));
                continue;
            }
        };
        if let TalkOutcome::Deferred(why) = &outcome {
            let _ = Backend::rollback(store, cx).await;
            eprintln!("deferring talk page {} (`{}`): {why}", page.id, page.title);
        } else {
            Backend::commit(store, cx).await.context("commit")?;
        }
        counts.add(&outcome);
    }
    Ok(counts)
}

/// The home target of a talk page: the actor for a user talk page, the subject page's ID
/// otherwise; `None` when the subject is not here.
async fn home_of(cx: &mut PgCx, ctx: &TalkContext<'_>, page: &DumpPage) -> Result<Option<Target>> {
    let ns = i32::try_from(page.ns).unwrap_or(0);
    let namespaces = Namespaces::embedded();
    let Some(namespace) = namespaces.get(ns) else {
        return Ok(None);
    };
    let subject_ns = ns - 1;
    let (_, name) = page.title.split_once(':').unwrap_or(("", &page.title));
    if subject_ns == 2 {
        // User talk: the account's actor key, if the source account was adopted.
        let root = name.split('/').next().unwrap_or(name);
        let id = ctx
            .accounts
            .iter()
            .find(|(_, n)| n.as_str() == root)
            .map(|(id, _)| *id);
        return Ok(match id {
            Some(id) if name == root => {
                let key = ActorKey::numeric(ctx.issuer, id).to_string();
                let present = cx
                    .query_opt(
                        "SELECT 1 FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                        &[&ctx.tenant, &key.as_str()],
                    )
                    .await?
                    .is_some();
                present.then_some(Target::Actor(key))
            }
            _ => None,
        });
    }
    let _ = namespace;
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
) -> Result<bool> {
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
        if let Some(rec) = read_record(first).map_err(anyhow::Error::msg)?
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
