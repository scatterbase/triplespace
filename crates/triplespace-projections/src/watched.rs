//! What a watch is of, and what it covers (16 §4.2–4.3), from `view` alone: the public half
//! of the watch set, which `triplespace-accounts` stores in `private.watch`.
//!
//! - **A watch's target** is the subject a page belongs to: an entity by ID for an entity's
//!   page or its talk page; an actor by key for a user page, a user talk page or a thread's
//!   home on one; a page by ID for a document page, a board or a thread, and for a talk
//!   page, its subject's; a title, `{ns}:{title}` in the subject namespace, for a page that
//!   does not exist yet. So the star on a subject and on its talk page toggle one row
//!   ([`of_page`], [`of_title`]).
//! - **What a watch set covers** is each target's *one target* set, expanded when it is
//!   read: an entity and its talk page; a page and its talk page; an actor's user page and
//!   user talk page; a title's page once there is one; and every thread attached, home or
//!   listing, to any talk page or board among them, so a thread that moves, is listed or is
//!   detached joins or leaves the set at once ([`expand`]).
//! - **Whose watches a change reaches**: a thread record reaches the thread and the subject
//!   of every page it is attached to; a page's record reaches the page's own target
//!   ([`reached_by_thread`], [`reached_by_page`]).

use scatter_log_postgres::PgClient;
use scatter_titles::Namespaces;

use crate::common::target_kind;
use crate::threads::{TARGET_ACTOR, TARGET_PAGE};

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// A watch target, `(kind, id)` in `view.activity`'s numbering.
pub type Target = (i16, String);

const USER: i32 = 2;
const USER_TALK: i32 = 3;

/// The subject namespace of a talk namespace; `None` for a namespace that is no other's talk.
#[must_use]
pub fn subject_ns(ns: i32) -> Option<i32> {
    Namespaces::embedded()
        .iter()
        .find(|n| n.talk == Some(ns) && n.id != ns)
        .map(|n| n.id)
}

async fn actor_named<C: PgClient>(
    client: &C,
    tenant: &str,
    name: &str,
) -> Result<Option<String>, String> {
    Ok(client
        .query_opt(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = $2",
            &[&tenant, &name],
        )
        .await
        .map_err(|e| sql(&e))?
        .map(|r| r.get(0)))
}

/// The target a page is watched as, or `None` for a page that does not exist.
///
/// # Errors
///
/// A database failure.
pub async fn of_page<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: u64,
) -> Result<Option<Target>, String> {
    let id = i64::try_from(page_id).map_err(|e| e.to_string())?;
    // An entity's page.
    if let Some(r) = client
        .query_opt(
            "SELECT id FROM view.entity WHERE tenant IN ($1, '') AND page_id = $2
             ORDER BY (tenant = $1) DESC LIMIT 1",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        return Ok(Some((target_kind::ENTITY, r.get(0))));
    }
    // A talk page or a board: its subject.
    if let Some(r) = client
        .query_opt(
            "SELECT target_kind, target_id FROM view.talk_page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        let (kind, target): (i16, String) = (r.get(0), r.get(1));
        if kind == TARGET_ACTOR {
            return Ok(Some((target_kind::ACTOR, target)));
        }
        if kind == TARGET_PAGE {
            let subject: u64 = target.parse().map_err(|_| "a page target's ID")?;
            if subject == page_id {
                return Ok(Some((target_kind::PAGE, target)));
            }
            return Box::pin(of_page(client, tenant, subject)).await;
        }
    }
    let Some(r) = client
        .query_opt(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
    else {
        return Ok(None);
    };
    let (ns, title): (i32, String) = (r.get(0), r.get(1));
    // A user's own page or user talk page, not a subpage.
    if (ns == USER || ns == USER_TALK)
        && !title.contains('/')
        && let Some(key) = actor_named(client, tenant, &title).await?
    {
        return Ok(Some((target_kind::ACTOR, key)));
    }
    // A talk page no thread has made one yet: its subject's.
    if let Some(s) = subject_ns(ns) {
        return Box::pin(of_title(client, tenant, s, &title))
            .await
            .map(Some);
    }
    Ok(Some((target_kind::PAGE, page_id.to_string())))
}

/// The target a title is watched as, whether a page holds it or not: the page's, an
/// entity's, a user's, or the title itself in its subject namespace.
///
/// # Errors
///
/// A database failure.
pub async fn of_title<C: PgClient>(
    client: &C,
    tenant: &str,
    ns: i32,
    title: &str,
) -> Result<Target, String> {
    let subject = subject_ns(ns).unwrap_or(ns);
    if let Some(r) = client
        .query_opt(
            "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted",
            &[&tenant, &subject, &title],
        )
        .await
        .map_err(|e| sql(&e))?
        && let Ok(p) = u64::try_from(r.get::<_, i64>(0))
        && let Some(t) = Box::pin(of_page(client, tenant, p)).await?
    {
        return Ok(t);
    }
    if let Some(id) = Namespaces::embedded()
        .get(subject)
        .and_then(|n| n.entity_id(title))
        && client
            .query_opt(
                "SELECT 1 FROM view.entity WHERE tenant IN ($1, '') AND id = $2",
                &[&tenant, &id],
            )
            .await
            .map_err(|e| sql(&e))?
            .is_some()
    {
        return Ok((target_kind::ENTITY, id));
    }
    if subject == USER
        && !title.contains('/')
        && let Some(key) = actor_named(client, tenant, title).await?
    {
        return Ok((target_kind::ACTOR, key));
    }
    // A talk page that exists while its subject does not is watched as itself.
    if subject != ns
        && let Some(r) = client
            .query_opt(
                "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted",
                &[&tenant, &ns, &title],
            )
            .await
            .map_err(|e| sql(&e))?
    {
        return Ok((target_kind::PAGE, r.get::<_, i64>(0).to_string()));
    }
    Ok((target_kind::TITLE, format!("{subject}:{title}")))
}

/// What a watch set covers: the entity IDs and page IDs whose activity rows are in it, and
/// for each, the watch it is there for, as an index into the targets expanded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expanded {
    /// Entity IDs.
    pub entities: Vec<String>,
    /// Page IDs, as text.
    pub pages: Vec<String>,
    /// `(target kind, target ID)` of an activity row → the index of the watch that covers it.
    pub origin: std::collections::HashMap<(i16, String), usize>,
}

impl Expanded {
    /// The watch an activity row is in the set for.
    #[must_use]
    pub fn watch_of(&self, kind: i16, id: &str) -> Option<usize> {
        self.origin.get(&(kind, id.to_string())).copied()
    }
}

/// The activity rows a watch set covers (see the module documentation).
///
/// # Errors
///
/// A database failure.
#[allow(clippy::too_many_lines)]
pub async fn expand<C: PgClient>(
    client: &C,
    tenant: &str,
    targets: &[Target],
) -> Result<Expanded, String> {
    use std::collections::HashMap;
    let mut x = Expanded::default();
    // Page ID → watch index; actor key → index; entity page ID → index.
    let mut page_of: HashMap<i64, usize> = HashMap::new();
    let mut actor_of: HashMap<String, usize> = HashMap::new();
    let mut entity_ids: Vec<String> = Vec::new();
    for (i, (kind, id)) in targets.iter().enumerate() {
        match *kind {
            target_kind::ENTITY => {
                x.origin
                    .entry((target_kind::ENTITY, id.clone()))
                    .or_insert(i);
                entity_ids.push(id.clone());
            }
            target_kind::PAGE => {
                if let Ok(p) = id.parse::<i64>() {
                    page_of.entry(p).or_insert(i);
                }
            }
            target_kind::ACTOR => {
                actor_of.entry(id.clone()).or_insert(i);
            }
            target_kind::TITLE => {
                if let Some((ns, title)) = id.split_once(':')
                    && let Ok(ns) = ns.parse::<i32>()
                {
                    let talk = Namespaces::embedded()
                        .get(ns)
                        .and_then(|n| n.talk)
                        .unwrap_or(ns);
                    for r in client
                        .query(
                            "SELECT page_id FROM view.page WHERE tenant = $1 AND ns IN ($2, $3) AND title = $4 AND NOT deleted",
                            &[&tenant, &ns, &talk, &title],
                        )
                        .await
                        .map_err(|e| sql(&e))?
                    {
                        page_of.entry(r.get(0)).or_insert(i);
                    }
                }
            }
            _ => {}
        }
    }
    // An actor's user page and user talk page, by name.
    let actors: Vec<String> = actor_of.keys().cloned().collect();
    for r in client
        .query(
            "SELECT p.page_id, a.actor_key FROM view.page p
             JOIN view.actor a ON a.tenant = p.tenant AND a.actor_key = ANY($2) AND a.name = p.title
             WHERE p.tenant = $1 AND p.ns IN (2, 3) AND NOT p.deleted",
            &[&tenant, &actors],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        let key: String = r.get(1);
        if let Some(i) = actor_of.get(&key) {
            page_of.entry(r.get(0)).or_insert(*i);
        }
    }
    // The talk pages: of pages, and of entities by their page ID; of actors by key.
    let mut subjects: HashMap<String, usize> =
        page_of.iter().map(|(p, i)| (p.to_string(), *i)).collect();
    for r in client
        .query(
            "SELECT page_id::text, id FROM view.entity WHERE tenant IN ($1, '') AND id = ANY($2)",
            &[&tenant, &entity_ids],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        let id: String = r.get(1);
        if let Some(i) = x.origin.get(&(target_kind::ENTITY, id)) {
            subjects.entry(r.get(0)).or_insert(*i);
        }
    }
    let subject_ids: Vec<String> = subjects.keys().cloned().collect();
    for r in client
        .query(
            "SELECT page_id, target_kind, target_id FROM view.talk_page WHERE tenant = $1
               AND ((target_kind = $2 AND target_id = ANY($3)) OR (target_kind = $4 AND target_id = ANY($5)))",
            &[&tenant, &TARGET_PAGE, &subject_ids, &TARGET_ACTOR, &actors],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        let (kind, target): (i16, String) = (r.get(1), r.get(2));
        let i = if kind == TARGET_ACTOR {
            actor_of.get(&target)
        } else {
            subjects.get(&target)
        };
        if let Some(i) = i {
            page_of.entry(r.get(0)).or_insert(*i);
        }
    }
    // Every thread attached to any of them, for the watch of the page it is attached to.
    let talks: Vec<i64> = page_of.keys().copied().collect();
    for r in client
        .query(
            "SELECT thread_id, talk_page_id FROM view.thread_attachment
             WHERE tenant = $1 AND talk_page_id = ANY($2) ORDER BY home DESC",
            &[&tenant, &talks],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        let talk: i64 = r.get(1);
        if let Some(i) = page_of.get(&talk).copied() {
            page_of.entry(r.get(0)).or_insert(i);
        }
    }
    let mut pages: Vec<(i64, usize)> = page_of.into_iter().collect();
    pages.sort_unstable();
    for (p, i) in pages {
        x.pages.push(p.to_string());
        x.origin.insert((target_kind::PAGE, p.to_string()), i);
    }
    x.entities = entity_ids;
    Ok(x)
}

/// The targets whose watches a thread's record reaches: the thread, and the subject of
/// every page it is attached to now.
///
/// # Errors
///
/// A database failure.
pub async fn reached_by_thread<C: PgClient>(
    client: &C,
    tenant: &str,
    thread: u64,
) -> Result<Vec<Target>, String> {
    let mut out = vec![(target_kind::PAGE, thread.to_string())];
    let id = i64::try_from(thread).map_err(|e| e.to_string())?;
    let talks: Vec<i64> = client
        .query(
            "SELECT talk_page_id FROM view.thread_attachment WHERE tenant = $1 AND thread_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for t in talks {
        if let Ok(t) = u64::try_from(t)
            && let Some(target) = of_page(client, tenant, t).await?
            && !out.contains(&target)
        {
            out.push(target);
        }
    }
    Ok(out)
}

/// The targets whose watches a document page's record reaches: the page's own, and the
/// title's, for a watch made before the page existed.
///
/// # Errors
///
/// A database failure.
pub async fn reached_by_page<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: u64,
) -> Result<Vec<Target>, String> {
    let mut out = Vec::new();
    if let Some(t) = of_page(client, tenant, page_id).await? {
        out.push(t);
    }
    let id = i64::try_from(page_id).map_err(|e| e.to_string())?;
    if let Some(r) = client
        .query_opt(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        let (ns, title): (i32, String) = (r.get(0), r.get(1));
        let subject = subject_ns(ns).unwrap_or(ns);
        let t = (target_kind::TITLE, format!("{subject}:{title}"));
        if !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}
