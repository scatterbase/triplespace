//! The current ACLs along a target's enclosure chain, read from `view.acl` (0016 §4; 0023
//! §2; 0056 §2–§5), for the routes that decide a read with
//! [`scatter_actors::read_decision`] and the writes that refuse a deleted target.
//!
//! - [`page_chain`]: a page, its namespace, the `pages` graph and the tenant; for a talk
//!   page, also its subject when the subject's deletion took the talk page with it.
//! - [`thread_chain`]: a thread, and its home talk page or board, which encloses it
//!   (09 §4.4), with that talk page's own chain.
//! - [`title_chain`]: a title no page holds yet, for a `create`: its reservation
//!   ([`reservation`], 09 §4.8), the namespace, the `pages` graph and the tenant.
//! - [`listing_chain`]: the page a thread is listed on through a page target, its talk
//!   page (a board is its own), for the `edit` that `create`, `attach`, `move` and `pin`
//!   need there (09 §2).
//! - [`record_acls`]: the `record` ACLs of a set of records, which hide parts of them.
//! - [`removed`]: whether a moderation restriction is in force anywhere on a chain, which
//!   is what makes a thread or page deleted for everyone, its deletion group included,
//!   when it comes to writing.
//! - [`hidden`]: what one principal may not read on a tenant because of confidential
//!   restrictions it does not satisfy (0056 §5; 09 §5.3), for the reads that list, count,
//!   link and look up pages by title, ID or revision.
//! - [`hidden_actors`]: which of some actors' names a principal may not see, because a
//!   username-hiding `actor` ACL it does not satisfy is in force (09 §6.3).

use std::collections::{BTreeSet, HashMap};

use scatter_actors::evaluate::Effective;
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
    for r in &rows {
        let (key, acl) = acl_of(r)?;
        by_key.insert(key, acl);
    }
    Ok(keys.iter().filter_map(|k| by_key.remove(k)).collect())
}

/// A `view.acl` row, selected as `target, restrictions, read_kind, extra`, as its ACL.
fn acl_of(r: &tokio_postgres::Row) -> Result<(String, Acl), String> {
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
    let acl: Acl =
        serde_json::from_value(Value::Object(v)).map_err(|e| format!("view.acl `{key}`: {e}"))?;
    Ok((key, acl))
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

/// What one principal may not read on a tenant: the targets of the confidential `read`
/// restrictions in force that it does not satisfy (0056 §5; 09 §5.3), and the threads
/// those enclose through their home talk page (09 §4.4). A target here is **absent** for
/// the principal: looked up, it is missing; listed, counted or linked, it is left out.
///
/// It is computed per request from the tenant's confidential ACLs, which are few (a
/// private tenant has one), so a list filters by it with one array parameter instead of
/// evaluating a chain per row. Moderation restrictions are not here: a deleted page is
/// already left out by `view.page.deleted` for everyone, and a hidden revision part by
/// `view.activity.visibility`.
///
/// Not yet covered: `set`, `entity`, `statement` and `property` targets, which nothing
/// writes yet, and subpages, which no chain encloses yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hidden {
    /// The tenant itself is restricted (a private tenant, 09 §4.7): nothing is readable.
    pub tenant: bool,
    /// The `pages` graph is restricted: no page is readable.
    pub all_pages: bool,
    /// Namespaces whose pages are absent.
    pub namespaces: BTreeSet<i32>,
    /// Pages and threads that are absent, by page ID.
    pub pages: BTreeSet<u64>,
}

impl Hidden {
    /// Nothing hidden: for writes, which decide with the target's own chain, and for
    /// principals that satisfy every restriction.
    pub const NONE: Self = Self {
        tenant: false,
        all_pages: false,
        namespaces: BTreeSet::new(),
        pages: BTreeSet::new(),
    };

    /// Whether nothing is hidden, so a query may skip the filter.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.tenant && !self.all_pages && self.namespaces.is_empty() && self.pages.is_empty()
    }

    /// Whether a page (or thread) in a namespace is absent.
    #[must_use]
    pub fn page(&self, ns: i32, page_id: u64) -> bool {
        self.tenant
            || self.all_pages
            || self.namespaces.contains(&ns)
            || self.pages.contains(&page_id)
    }

    /// Whether every page is absent, so a page list is empty.
    #[must_use]
    pub fn every_page(&self) -> bool {
        self.tenant || self.all_pages
    }

    /// The hidden page IDs as `bigint`s, for an `= ANY($n)` parameter.
    #[must_use]
    pub fn page_ids(&self) -> Vec<i64> {
        self.pages
            .iter()
            .filter_map(|p| i64::try_from(*p).ok())
            .collect()
    }

    /// The hidden namespaces, for an `= ANY($n)` parameter.
    #[must_use]
    pub fn namespace_list(&self) -> Vec<i32> {
        self.namespaces.iter().copied().collect()
    }
}

/// Whether a confidential `read` restriction on this ACL is in force at `at` and the
/// principal does not satisfy it: the test [`scatter_actors::read_decision`] makes for
/// one ACL of a chain. A deleted confidential target keeps its restriction in
/// [`Acl::confidential`], and is hidden by it as before (09 §5.2).
fn refuses(acl: &Acl, effective: &Effective, at: Timestamp) -> bool {
    acl.read_restrictions_at(at).any(|(kind, r)| {
        kind == ReadKind::Confidential && !r.group.as_ref().is_some_and(|g| effective.in_group(g))
    })
}

/// What `effective` may not read on the tenant at `at` (see [`Hidden`]): one query for the
/// tenant's confidential ACLs and, when a page or namespace is hidden, one for the threads
/// whose home talk page it hides.
pub async fn hidden<C: PgClient>(
    client: &C,
    tenant: &str,
    effective: &Effective,
    at: Timestamp,
) -> Result<Hidden, String> {
    let rows = client
        .query(
            "SELECT target, restrictions, read_kind, extra FROM view.acl
             WHERE tenant = $1 AND (read_kind = 'confidential' OR extra ? 'confidential')",
            &[&tenant],
        )
        .await
        .map_err(|e| sql(&e))?;
    let mut h = Hidden::default();
    for r in &rows {
        let (_, acl) = acl_of(r)?;
        if !refuses(&acl, effective, at) {
            continue;
        }
        match acl.target {
            AclTarget::Tenant(t) if t == tenant => h.tenant = true,
            AclTarget::Graph(g) if g == "pages" => h.all_pages = true,
            AclTarget::Namespace(n) => {
                if let Ok(n) = i32::try_from(n) {
                    h.namespaces.insert(n);
                }
            }
            AclTarget::Page(p) => {
                h.pages.insert(p);
            }
            _ => {}
        }
    }
    if h.every_page() || (h.pages.is_empty() && h.namespaces.is_empty()) {
        return Ok(h);
    }
    // A thread is enclosed by its home talk page or board (09 §4.4).
    let homes = h.page_ids();
    let nss = h.namespace_list();
    let threads = client
        .query(
            "SELECT t.page_id FROM view.thread t
             LEFT JOIN view.page p ON p.tenant = t.tenant AND p.page_id = t.talk_page_id
             WHERE t.tenant = $1 AND (t.talk_page_id = ANY($2) OR p.ns = ANY($3))",
            &[&tenant, &homes, &nss],
        )
        .await
        .map_err(|e| sql(&e))?;
    for r in threads {
        if let Ok(p) = u64::try_from(r.get::<_, i64>(0)) {
            h.pages.insert(p);
        }
    }
    Ok(h)
}

/// Which of these actors' names the principal, a member of `groups`, may not see: those
/// with an `actor` ACL whose `read` restriction is in force at `at` and names a group the
/// principal is not in (09 §6.3). The decision is made per request from the ACL, so the
/// suppression group keeps seeing the name and an expired hide lapses by itself. Asked
/// for the actors of the rows being served, never the tenant's whole list.
pub async fn hidden_actors<C: PgClient>(
    client: &C,
    tenant: &str,
    keys: &[String],
    groups: &[String],
    at: Timestamp,
) -> Result<std::collections::HashSet<String>, String> {
    if keys.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let targets: Vec<String> = keys.iter().map(|k| format!("acl:actor:{k}")).collect();
    let rows = client
        .query(
            "SELECT target, restrictions, read_kind, extra FROM view.acl
             WHERE tenant IN ($1, '') AND target = ANY($2)",
            &[&tenant, &targets],
        )
        .await
        .map_err(|e| sql(&e))?;
    let mut out = std::collections::HashSet::new();
    for r in &rows {
        let (key, acl) = acl_of(r)?;
        let refused = acl
            .read_restrictions_at(at)
            .any(|(_, r)| !r.group.as_ref().is_some_and(|g| groups.contains(g)));
        if refused && let Some(actor) = key.strip_prefix("acl:actor:") {
            out.insert(actor.to_string());
        }
    }
    Ok(out)
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

/// The page ID a reservation holds a title under, while no revision has filled it.
pub async fn reservation<C: PgClient>(
    client: &C,
    tenant: &str,
    ns: i32,
    title: &str,
) -> Result<Option<u64>, String> {
    Ok(client
        .query_opt(
            "SELECT page_id FROM view.page
             WHERE tenant = $1 AND ns = $2 AND title = $3 AND reserved AND latest_offset IS NULL",
            &[&tenant, &ns, &title],
        )
        .await
        .map_err(|e| sql(&e))?
        .and_then(|r| u64::try_from(r.get::<_, i64>(0)).ok()))
}

/// The chain of a title no page holds: the reservation's ACL when one holds the title,
/// the namespace, the `pages` graph and the tenant.
pub async fn title_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    ns: i32,
    reserved: Option<u64>,
) -> Result<Vec<Acl>, String> {
    let mut targets: Vec<AclTarget> = reserved.into_iter().map(AclTarget::Page).collect();
    targets.push(AclTarget::Namespace(i64::from(ns)));
    targets.push(AclTarget::Graph("pages".into()));
    targets.push(AclTarget::Tenant(tenant.to_string()));
    current(client, tenant, &targets).await
}

/// The chain of the page a page target lists threads on: the target's talk page, which
/// for a board is the board; while the target has no talk page, the `pages` graph and the
/// tenant, since nothing more specific can be restricted yet.
pub async fn listing_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    target: u64,
) -> Result<Vec<Acl>, String> {
    let talk = client
        .query_opt(
            "SELECT page_id FROM view.talk_page
             WHERE tenant = $1 AND target_kind = $2 AND target_id = $3",
            &[&tenant, &TARGET_PAGE, &target.to_string()],
        )
        .await
        .map_err(|e| sql(&e))?
        .and_then(|r| u64::try_from(r.get::<_, i64>(0)).ok());
    match talk {
        Some(t) => page_chain(client, tenant, t).await,
        None => {
            current(
                client,
                tenant,
                &[
                    AclTarget::Graph("pages".into()),
                    AclTarget::Tenant(tenant.to_string()),
                ],
            )
            .await
        }
    }
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
