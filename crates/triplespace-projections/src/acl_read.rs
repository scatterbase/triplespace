//! The current ACLs along a target's enclosure chain, read from `view.acl` (0016 §4; 0023
//! §2; 0056 §2–§5), for the routes that decide a read with
//! [`scatter_actors::read_decision`] and the writes that refuse a deleted target.
//!
//! - [`page_chain`]: a page, its namespace, the `pages` graph and the tenant; for a talk
//!   page, also its subject when the subject's deletion took the talk page with it.
//! - [`thread_chain`]: a thread, and its home talk page or board, which encloses it
//!   (09 §4.4), with that talk page's own chain, and for a user talk page the user's
//!   `actor` ACL, which hides their name and so their talk page (09 §6.3); [`thread_readable`], whether a principal
//!   may read a thread now, as a notification's recipient must (16 §5.2).
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
pub fn acl_of(r: &tokio_postgres::Row) -> Result<(String, Acl), String> {
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
/// Not yet covered: `statement` and `property` targets, which nothing writes yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hidden {
    /// The tenant itself is restricted (a private tenant, 09 §4.7): nothing is readable.
    pub tenant: bool,
    /// The `pages` graph is restricted: no page is readable.
    pub all_pages: bool,
    /// Namespaces whose pages are absent.
    pub namespaces: BTreeSet<i32>,
    /// Pages and threads that are absent, by page ID: those restricted themselves, the
    /// members of a hidden set, the subpages of either, and the threads they are home to.
    pub pages: BTreeSet<u64>,
    /// The keys of hidden `namespace` and `set` targets, whose own log events (a
    /// `protect/*` naming `acl:namespace:4`) are absent with them (09 §5.3).
    pub acl_keys: BTreeSet<String>,
    /// Entities that are absent, by ID: those restricted themselves and the members of a
    /// hidden set. An entity namespace among [`Self::namespaces`] hides its entities too
    /// ([`Self::entity`]).
    pub entities: BTreeSet<String>,
    /// Graphs restricted: an entity held by one is absent (`local`, `mirror/wikidata`, …).
    pub graphs: BTreeSet<String>,
}

impl Hidden {
    /// Nothing hidden: for writes, which decide with the target's own chain, and for
    /// principals that satisfy every restriction.
    pub const NONE: Self = Self {
        tenant: false,
        all_pages: false,
        namespaces: BTreeSet::new(),
        pages: BTreeSet::new(),
        acl_keys: BTreeSet::new(),
        entities: BTreeSet::new(),
        graphs: BTreeSet::new(),
    };

    /// Whether nothing is hidden, so a query may skip the filter.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.tenant
            && !self.all_pages
            && self.namespaces.is_empty()
            && self.pages.is_empty()
            && self.acl_keys.is_empty()
            && self.entities.is_empty()
            && self.graphs.is_empty()
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

    /// Whether an entity is absent: the tenant, its namespace, the graph holding it,
    /// itself or a set it is in. `graph` is the graph whose record serves it (`local`, or a
    /// mirror); `None` when unknown, which counts no graph restriction.
    #[must_use]
    pub fn entity(&self, id: &str, graph: Option<&str>) -> bool {
        self.tenant
            || self.entities.contains(id)
            || graph.is_some_and(|g| self.graphs.contains(g))
            || entity_namespace(id).is_some_and(|ns| self.namespaces.contains(&ns))
    }

    /// Whether every entity is absent, so an entity list is empty.
    #[must_use]
    pub fn every_entity(&self) -> bool {
        self.tenant
    }

    /// The hidden entity IDs, for an `= ANY($n)` parameter.
    #[must_use]
    pub fn entity_ids(&self) -> Vec<String> {
        self.entities.iter().cloned().collect()
    }

    /// The hidden entity namespaces, for an `= ANY($n)` parameter over `view.entity.type`:
    /// the types whose namespace is hidden.
    #[must_use]
    pub fn entity_types(&self) -> Vec<String> {
        ENTITY_NAMESPACES
            .iter()
            .filter(|(_, ns)| self.namespaces.contains(ns))
            .map(|(t, _)| (*t).to_string())
            .collect()
    }
}

/// Each entity type's namespace (namespaces.toml; 0017 §1), as `activity::title_of` maps
/// them.
pub const ENTITY_NAMESPACES: &[(&str, i32)] = &[
    ("item", 120),
    ("property", 122),
    ("lexeme", 146),
    ("domain", 210),
    ("keyword", 212),
    ("notation", 216),
];

/// The namespace of an entity by its ID: a keyed type's by its prefix, a property's by
/// `P`, a lexeme's by `L`, an item's otherwise (foreign IDs, `WDQ42`, are items unless
/// their provider's type says otherwise, which the resolver knows and this does not).
#[must_use]
pub fn entity_namespace(id: &str) -> Option<i32> {
    if let Some((kind, _)) = id.split_once(':') {
        return ENTITY_NAMESPACES
            .iter()
            .find(|(t, _)| *t == kind)
            .map(|(_, ns)| *ns);
    }
    let local =
        id.trim_start_matches(|c: char| c.is_ascii_uppercase() && c != 'Q' && c != 'P' && c != 'L');
    Some(match local.chars().next() {
        Some('P') => 122,
        Some('L') => 146,
        _ => 120,
    })
}

/// Whether a confidential `read` restriction on this ACL is in force at `at` and the
/// principal does not satisfy it: the test [`scatter_actors::read_decision`] makes for
/// one ACL of a chain. A deleted confidential target keeps its restriction in
/// [`Acl::confidential`], and is hidden by it as before (09 §5.2).
fn refuses(acl: &Acl, effective: &Effective, at: Timestamp) -> bool {
    acl.read_restrictions_at(at).any(|(kind, r)| {
        kind == ReadKind::Confidential && !r.group.as_ref().is_some_and(|g| effective.reads_in(g))
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
             WHERE tenant = $1 AND (read_kind = 'confidential' OR extra ? 'confidential'
                                    OR target LIKE 'acl:actor:%')",
            &[&tenant],
        )
        .await
        .map_err(|e| sql(&e))?;
    let mut h = Hidden::default();
    // Confidential pages whose restriction took their talk page (`talk`; 09 §4.4).
    let mut talk_of: Vec<u64> = Vec::new();
    for r in &rows {
        let (_, mut acl) = acl_of(r)?;
        let members = std::mem::take(&mut acl.members);
        if !refuses(&acl, effective, at) {
            continue;
        }
        match acl.target {
            AclTarget::Tenant(t) if t == tenant => h.tenant = true,
            AclTarget::Graph(g) if g == "pages" => h.all_pages = true,
            AclTarget::Graph(g) => {
                h.graphs.insert(g);
            }
            AclTarget::Entity(e) => {
                h.entities.insert(e);
            }
            AclTarget::Namespace(n) => {
                h.acl_keys.insert(AclTarget::Namespace(n).key());
                if let Ok(n) = i32::try_from(n) {
                    h.namespaces.insert(n);
                }
            }
            AclTarget::Page(p) => {
                h.pages.insert(p);
                if acl.talk {
                    talk_of.push(p);
                }
            }
            AclTarget::Set(id) => {
                h.acl_keys.insert(AclTarget::Set(id).key());
                for m in &members {
                    match m {
                        scatter_actors::acl::SetMember::Page(p) => {
                            h.pages.insert(*p);
                        }
                        scatter_actors::acl::SetMember::Entity(e) => {
                            h.entities.insert(e.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    hide_user_pages(client, tenant, effective, at, &rows, &mut h).await?;
    hide_talk_pages(client, tenant, &talk_of, &mut h).await?;
    if h.every_page() || (h.pages.is_empty() && h.namespaces.is_empty()) {
        return Ok(h);
    }
    // A page encloses its subpages, where its namespace has them (09 §4.4). The subpages
    // of each root are a range of the byte-order title index (migration 0020): `Root/` up
    // to the character after `/`, one index scan per root, rather than a prefix test
    // against every page.
    if !h.pages.is_empty() {
        let roots = h.page_ids();
        let subs = client
            .query(
                "SELECT p.page_id FROM view.page r
                 JOIN view.page p ON p.tenant = r.tenant AND p.ns = r.ns
                  AND p.title COLLATE \"C\" > (r.title || '/') COLLATE \"C\"
                  AND p.title COLLATE \"C\" < (r.title || '0') COLLATE \"C\"
                 WHERE r.tenant = $1 AND r.page_id = ANY($2) AND r.ns = ANY($3)",
                &[&tenant, &roots, &subpage_namespaces()],
            )
            .await
            .map_err(|e| sql(&e))?;
        for r in subs {
            if let Ok(p) = u64::try_from(r.get::<_, i64>(0)) {
                h.pages.insert(p);
            }
        }
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

/// A hidden user's own user and user-talk pages carry the name (09 §6.3): absent for
/// those outside the actor ACL's group, with their subpages.
async fn hide_user_pages<C: PgClient>(
    client: &C,
    tenant: &str,
    effective: &Effective,
    at: Timestamp,
    rows: &[tokio_postgres::Row],
    h: &mut Hidden,
) -> Result<(), String> {
    let mut hidden_names: Vec<String> = Vec::new();
    for r in rows {
        let (key, acl) = acl_of(r)?;
        if let Some(actor) = key.strip_prefix("acl:actor:")
            && acl
                .read_restrictions_at(at)
                .any(|(_, r)| !r.group.as_deref().is_some_and(|g| effective.reads_in(g)))
        {
            hidden_names.push(actor.to_string());
        }
    }
    if hidden_names.is_empty() {
        return Ok(());
    }
    let pages = client
        .query(
            "SELECT p.page_id FROM view.page p
             JOIN view.actor a ON a.tenant = p.tenant AND a.actor_key = ANY($2) AND a.name IS NOT NULL
             WHERE p.tenant = $1 AND p.ns IN (2, 3)
               AND (p.title = a.name OR left(p.title, length(a.name) + 1) = a.name || '/')",
            &[&tenant, &hidden_names],
        )
        .await
        .map_err(|e| sql(&e))?;
    for r in pages {
        if let Ok(p) = u64::try_from(r.get::<_, i64>(0)) {
            h.pages.insert(p);
        }
    }
    Ok(())
}

/// The talk pages of confidential pages whose restriction took them (`talk`; 09 §4.4).
async fn hide_talk_pages<C: PgClient>(
    client: &C,
    tenant: &str,
    talk_of: &[u64],
    h: &mut Hidden,
) -> Result<(), String> {
    if talk_of.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = talk_of.iter().map(u64::to_string).collect();
    let talks = client
        .query(
            "SELECT page_id FROM view.talk_page
             WHERE tenant = $1 AND target_kind = $2 AND target_id = ANY($3)",
            &[&tenant, &TARGET_PAGE, &ids],
        )
        .await
        .map_err(|e| sql(&e))?;
    for r in talks {
        if let Ok(p) = u64::try_from(r.get::<_, i64>(0)) {
            h.pages.insert(p);
        }
    }
    Ok(())
}

/// Whether the tenant is private to the principal: a `tenant` ACL whose confidential
/// `read` restriction it does not satisfy (09 §4.7). Everything of the tenant is then
/// absent for it; this is the one lookup the entity routes need, where [`hidden`] serves
/// page reads.
pub async fn tenant_hidden<C: PgClient>(
    client: &C,
    tenant: &str,
    effective: &Effective,
    at: Timestamp,
) -> Result<bool, String> {
    Ok(
        current(client, tenant, &[AclTarget::Tenant(tenant.to_string())])
            .await?
            .iter()
            .any(|a| refuses(a, effective, at)),
    )
}

/// Which of these actors' names the principal may not see: those with an `actor` ACL
/// whose `read` restriction is in force at `at` and names a group `reads` does not pass
/// (09 §6.3). The decision is made per request from the ACL, so the
/// suppression group keeps seeing the name and an expired hide lapses by itself. Asked
/// for the actors of the rows being served, never the tenant's whole list.
pub async fn hidden_actors<C: PgClient>(
    client: &C,
    tenant: &str,
    keys: &[String],
    reads: impl Fn(&str) -> bool,
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
            .any(|(_, r)| !r.group.as_deref().is_some_and(&reads));
        if refused && let Some(actor) = key.strip_prefix("acl:actor:") {
            out.insert(actor.to_string());
        }
    }
    Ok(out)
}

/// The namespaces where `/` makes subpages (namespaces.toml).
fn subpage_namespaces() -> Vec<i32> {
    scatter_titles::Namespaces::embedded()
        .iter()
        .filter(|n| n.subpages)
        .map(|n| n.id)
        .collect()
}

/// The titles of a subpage's ancestors, nearest first: `A/B/C` → `A/B`, `A`.
fn ancestor_titles(title: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut t = title;
    while let Some((head, _)) = t.rsplit_once('/') {
        if head.is_empty() {
            break;
        }
        out.push(head.to_string());
        t = head;
    }
    out
}

/// The pages enclosing a title as subpages enclose (09 §4.4), where its namespace has them.
async fn ancestors<C: PgClient>(
    client: &C,
    tenant: &str,
    ns: i32,
    title: &str,
) -> Result<Vec<u64>, String> {
    if !subpage_namespaces().contains(&ns) {
        return Ok(Vec::new());
    }
    let titles = ancestor_titles(title);
    if titles.is_empty() {
        return Ok(Vec::new());
    }
    Ok(client
        .query(
            "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = ANY($3)",
            &[&tenant, &ns, &titles],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .filter_map(|r| u64::try_from(r.get::<_, i64>(0)).ok())
        .collect())
}

/// The sets any of these pages is a member of (0056 §3; `view.set_member`).
async fn sets_of<C: PgClient>(client: &C, tenant: &str, pages: &[u64]) -> Result<Vec<u64>, String> {
    if pages.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = pages.iter().map(u64::to_string).collect();
    Ok(client
        .query(
            "SELECT DISTINCT set_id FROM view.set_member
             WHERE tenant = $1 AND kind = 'page' AND id = ANY($2)",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .filter_map(|r| u64::try_from(r.get::<_, i64>(0)).ok())
        .collect())
}

/// An ancestor page's ACL as it encloses a subpage: everything but a deletion, which is
/// of the page alone, as in MediaWiki; a confidential restriction the deletion kept is
/// still in force. `None` when nothing is left.
fn as_enclosing(mut acl: Acl) -> Option<Acl> {
    if acl.read_kind == Some(ReadKind::Moderation) {
        acl.restrictions.remove(scatter_actors::acl::READ);
        acl.read_kind = None;
        if let Some(c) = acl.confidential.take() {
            acl.restrictions
                .insert(scatter_actors::acl::READ.to_string(), c);
            acl.read_kind = Some(ReadKind::Confidential);
        }
    }
    acl.talk = false;
    (!acl.restrictions.is_empty()).then_some(acl)
}

/// What encloses a page or a title beyond its own ACL: its ancestors' ACLs as they
/// enclose (see [`as_enclosing`]), and the ACLs of every set it or an ancestor is in.
async fn enclosing<C: PgClient>(
    client: &C,
    tenant: &str,
    page: Option<u64>,
    ns: i32,
    title: &str,
) -> Result<Vec<Acl>, String> {
    let parents = ancestors(client, tenant, ns, title).await?;
    let mut members: Vec<u64> = page.into_iter().collect();
    members.extend(&parents);
    let sets = sets_of(client, tenant, &members).await?;
    let mut targets: Vec<AclTarget> = parents.iter().copied().map(AclTarget::Page).collect();
    targets.extend(sets.into_iter().map(AclTarget::Set));
    let found = current(client, tenant, &targets).await?;
    Ok(found
        .into_iter()
        .filter_map(|a| match a.target {
            AclTarget::Page(_) => as_enclosing(a),
            _ => Some(a),
        })
        .collect())
}

/// The chain of a page: the page, its namespace, the `pages` graph and the tenant; for a
/// talk page, its subject's ACL too when that ACL took the talk page with it (`talk`).
pub async fn page_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: u64,
) -> Result<Vec<Acl>, String> {
    let id = i64::try_from(page_id).map_err(|e| e.to_string())?;
    let row: Option<(i32, String)> = client
        .query_opt(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
        .map(|r| (r.get(0), r.get(1)));
    let mut targets = vec![AclTarget::Page(page_id)];
    if let Some((ns, _)) = &row {
        targets.push(AclTarget::Namespace(i64::from(*ns)));
    }
    targets.push(AclTarget::Graph("pages".into()));
    targets.push(AclTarget::Tenant(tenant.to_string()));
    let mut chain = current(client, tenant, &targets).await?;
    // Its ancestors and its sets (09 §4.4).
    if let Some((ns, title)) = &row {
        chain.extend(enclosing(client, tenant, Some(page_id), *ns, title).await?);
    } else {
        let sets: Vec<AclTarget> = sets_of(client, tenant, &[page_id])
            .await?
            .into_iter()
            .map(AclTarget::Set)
            .collect();
        chain.extend(current(client, tenant, &sets).await?);
    }
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

/// The namespaces and sets that enclose a title along the containment axis, as a scoped
/// group's `within` is tested against them (09 §3.2, §3.4): the namespace, and the sets
/// the page (if any) or one of its ancestors is in.
pub async fn title_scope<C: PgClient>(
    client: &C,
    tenant: &str,
    ns: i32,
    title: &str,
    page: Option<u64>,
) -> Result<Vec<AclTarget>, String> {
    let mut members: Vec<u64> = page.into_iter().collect();
    members.extend(ancestors(client, tenant, ns, title).await?);
    let mut out = vec![AclTarget::Namespace(i64::from(ns))];
    out.extend(
        sets_of(client, tenant, &members)
            .await?
            .into_iter()
            .map(AclTarget::Set),
    );
    Ok(out)
}

/// [`title_scope`] for a page by ID; for a thread, its home talk page's scope as well,
/// and that talk page's subject page's, since a scope encloses the threads its pages are
/// home to.
pub async fn page_scope<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: u64,
) -> Result<Vec<AclTarget>, String> {
    let id = i64::try_from(page_id).map_err(|e| e.to_string())?;
    let row = client
        .query_opt(
            "SELECT p.ns, p.title, t.talk_page_id FROM view.page p
             LEFT JOIN view.thread t ON t.tenant = p.tenant AND t.page_id = p.page_id
             WHERE p.tenant = $1 AND p.page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?;
    let Some(row) = row else {
        return Ok(sets_of(client, tenant, &[page_id])
            .await?
            .into_iter()
            .map(AclTarget::Set)
            .collect());
    };
    let (ns, title): (i32, String) = (row.get(0), row.get(1));
    let mut out = title_scope(client, tenant, ns, &title, Some(page_id)).await?;
    if let Some(home) = row
        .get::<_, Option<i64>>(2)
        .and_then(|h| u64::try_from(h).ok())
        .filter(|h| *h != page_id)
    {
        let subject = client
            .query_opt(
                "SELECT target_id FROM view.talk_page
                 WHERE tenant = $1 AND page_id = $2 AND target_kind = $3",
                &[
                    &tenant,
                    &i64::try_from(home).unwrap_or(i64::MAX),
                    &TARGET_PAGE,
                ],
            )
            .await
            .map_err(|e| sql(&e))?
            .and_then(|r| r.get::<_, String>(0).parse::<u64>().ok());
        for p in std::iter::once(home).chain(subject) {
            for t in Box::pin(page_scope(client, tenant, p)).await? {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
    }
    Ok(out)
}

/// The sets an entity is a member of (`view.set_member`).
async fn entity_sets_of<C: PgClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Vec<u64>, String> {
    Ok(client
        .query(
            "SELECT DISTINCT set_id FROM view.set_member
             WHERE tenant = $1 AND kind = 'entity' AND id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .filter_map(|r| u64::try_from(r.get::<_, i64>(0)).ok())
        .collect())
}

/// The chain of an entity (09 §4.4): the entity, its namespace, the graph serving it (when
/// known), its sets and the tenant.
pub async fn entity_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    id: &str,
    graph: Option<&str>,
) -> Result<Vec<Acl>, String> {
    let mut targets = vec![AclTarget::Entity(id.to_string())];
    if let Some(ns) = entity_namespace(id) {
        targets.push(AclTarget::Namespace(i64::from(ns)));
    }
    if let Some(g) = graph {
        targets.push(AclTarget::Graph(g.to_string()));
    }
    targets.extend(
        entity_sets_of(client, tenant, id)
            .await?
            .into_iter()
            .map(AclTarget::Set),
    );
    targets.push(AclTarget::Tenant(tenant.to_string()));
    current(client, tenant, &targets).await
}

/// The namespaces and sets that enclose an entity, as a scoped group's `within` is tested
/// against them (09 §3.4).
pub async fn entity_scope<C: PgClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Vec<AclTarget>, String> {
    let mut out: Vec<AclTarget> = entity_namespace(id)
        .map(|ns| AclTarget::Namespace(i64::from(ns)))
        .into_iter()
        .collect();
    out.extend(
        entity_sets_of(client, tenant, id)
            .await?
            .into_iter()
            .map(AclTarget::Set),
    );
    Ok(out)
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
/// the namespace, the `pages` graph and the tenant, and the title's ancestors and their
/// sets where the namespace has subpages.
pub async fn title_chain<C: PgClient>(
    client: &C,
    tenant: &str,
    ns: i32,
    title: &str,
    reserved: Option<u64>,
) -> Result<Vec<Acl>, String> {
    let mut targets: Vec<AclTarget> = reserved.into_iter().map(AclTarget::Page).collect();
    targets.push(AclTarget::Namespace(i64::from(ns)));
    targets.push(AclTarget::Graph("pages".into()));
    targets.push(AclTarget::Tenant(tenant.to_string()));
    let mut chain = current(client, tenant, &targets).await?;
    // A subpage is created inside its ancestors and their sets (09 §4.4).
    chain.extend(enclosing(client, tenant, reserved, ns, title).await?);
    Ok(chain)
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
            "SELECT talk_page_id, target_kind, target_id FROM view.thread WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
    else {
        return Ok(None);
    };
    let home: Option<i64> = row.get(0);
    let mut targets = vec![AclTarget::Page(thread), AclTarget::Namespace(NS_THREAD_I64)];
    // A thread on a user talk page is enclosed by the user: a hidden name hides the
    // threads on their talk page, as it hides the page (09 §6.3).
    if row.get::<_, i16>(1) == crate::threads::TARGET_ACTOR
        && let Ok(key) = scatter_actors::ActorKey::parse(&row.get::<_, String>(2))
    {
        targets.push(AclTarget::Actor(key));
    }
    targets.extend(
        sets_of(client, tenant, &[thread])
            .await?
            .into_iter()
            .map(AclTarget::Set),
    );
    let mut chain = current(client, tenant, &targets).await?;
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

/// Whether a principal may read a thread now: it exists and its chain reads as
/// `Readable` to them. A thread deleted, or under a confidential restriction they do not
/// satisfy, does not (16 §5.2; 09 §5.3).
///
/// # Errors
///
/// A database failure.
pub async fn thread_readable<C: PgClient>(
    client: &C,
    tenant: &str,
    who: &Effective,
    thread: u64,
) -> Result<bool, String> {
    let at = Timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX)),
    );
    Ok(match thread_chain(client, tenant, thread).await? {
        Some(chain) => {
            scatter_actors::read_decision(who, &chain, at) == scatter_actors::ReadDecision::Readable
        }
        None => false,
    })
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
