//! Reports (0047 §4; 21 §2; 03 §4.10, §6.1): the catalogue of the reports this build
//! serves, the `report` projection that keeps the projection-backed ones, and the reads
//! that serve every one of them.
//!
//! **Two backings.** An `index` report has no rows of its own: it is a paged query over a
//! `view` table, read live. A `projection` report's rows are `view.report_entry`, kept by
//! the `report` projection, a **view-derived** projection (03 §6.1):
//!
//! - **A build** ([`rebuild`]) scans the `view` tables each report reads, with set-based
//!   SQL, and replaces the tenant's entries. It is never a replay of the log.
//! - **Incremental upkeep** follows "the page projection's row changes" (0047 §4.3) through
//!   `ops.projection_work`: the page projection queues a key for every title and category
//!   a page write touches (its own title, old and new on a move; every link target it
//!   dropped or added; every category it left or joined), and the ACL projection queues a
//!   page's key, or the whole tenant's for a wider restriction ([`queue`]). The worker
//!   ([`drain`]) recomputes only those keys ([`recompute_title`], [`recompute_category`]),
//!   in no write's transaction.
//! - **Off until built.** A tenant's reports are on once `view.report_state` has rows for
//!   it. Until then nothing is queued, so an adoption or a bulk load costs the write path
//!   one guarded statement per page record and no rows; the first read builds
//!   ([`ensure_built`]).
//!
//! The rebuild and the per-key recompute are two implementations of each report's
//! condition; the tests hold them to the same answer.
//!
//! **Keys.** An entry's key is a title as `view.page_link` writes one, `{ns}:{title}`, so a
//! category's is `14:{name}`. Work keys are `title:{ns}:{title}`, `category:{name}`,
//! `page:{id}` (a page whose visibility changed: its title, subpages, link targets and
//! categories) and `tenant` (rebuild).
//!
//! **Public form** (0047 §4.5). Entries never list what an anonymous reader could not
//! see: a deleted page, a page behind a read restriction (its namespace's, a set's, its
//! own), and links and memberships from such pages are not counted. A private tenant's or
//! a restricted `pages` graph's reports are built as for a member, since the report page
//! itself is then behind that restriction ([`Public`]).
//!
//! **Not yet:** batch mode and the instance `reports` configuration (0047 §4.3), widening
//! to mirror graphs (§4.2), and the reports that need transclusions, files, sitelinks or
//! entity composition events. `LonelyPages` counts links only until transclusions are
//! projected.

use std::collections::{BTreeMap, BTreeSet};

use deadpool_postgres::Pool;
use scatter_log_postgres::PgClient;
use serde_json::{Value, json};

use crate::backend::PgCx;

fn sql(e: &tokio_postgres::Error) -> String {
    format!("reports: {e}")
}

/// The projection's name in `ops.projection_work` and lag reports.
pub const PROJECTION: &str = "report";

/// The only graph scope this build computes (0047 §4.2).
pub const LOCAL: &str = "local";

/// How a report is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backing {
    /// A live query over a `view` table.
    Index,
    /// Rows in `view.report_entry`.
    Projection,
}

impl Measure {
    /// As the API reports it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Count => "count",
            Self::Bytes => "bytes",
            Self::Time => "time",
        }
    }
}

impl Backing {
    /// As the registry writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Projection => "projection",
        }
    }
}

/// What a report's value is, for its readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// No value: an alphabetical list.
    None,
    /// A count: links, members, categories, revisions.
    Count,
    /// A size in bytes.
    Bytes,
    /// A time.
    Time,
}

/// A report this build serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The registry's `name`.
    pub name: &'static str,
    /// The registry's `mediawiki_name`, which `list=querypage` also accepts.
    pub mediawiki_name: Option<&'static str>,
    /// Its backing.
    pub backing: Backing,
    /// What its value measures.
    pub value: Measure,
}

const fn report(
    name: &'static str,
    mediawiki_name: Option<&'static str>,
    backing: Backing,
    value: Measure,
) -> Report {
    Report {
        name,
        mediawiki_name,
        backing,
        value,
    }
}

/// The reports this build serves, in the registry's spelling.
pub const REPORTS: &[Report] = &[
    // Projection-backed: links and categories.
    report(
        "WantedPages",
        Some("Wantedpages"),
        Backing::Projection,
        Measure::Count,
    ),
    report(
        "WantedCategories",
        Some("Wantedcategories"),
        Backing::Projection,
        Measure::Count,
    ),
    report(
        "MostLinkedPages",
        Some("Mostlinked"),
        Backing::Projection,
        Measure::Count,
    ),
    report(
        "MostLinkedCategories",
        Some("Mostlinkedcategories"),
        Backing::Projection,
        Measure::Count,
    ),
    report(
        "MostCategories",
        Some("Mostcategories"),
        Backing::Projection,
        Measure::Count,
    ),
    report(
        "DeadendPages",
        Some("Deadendpages"),
        Backing::Projection,
        Measure::None,
    ),
    report(
        "LonelyPages",
        Some("Lonelypages"),
        Backing::Projection,
        Measure::None,
    ),
    report(
        "UncategorizedPages",
        Some("Uncategorizedpages"),
        Backing::Projection,
        Measure::None,
    ),
    report(
        "UncategorizedCategories",
        Some("Uncategorizedcategories"),
        Backing::Projection,
        Measure::None,
    ),
    report(
        "UncategorizedTemplates",
        Some("Uncategorizedtemplates"),
        Backing::Projection,
        Measure::None,
    ),
    report(
        "UnusedCategories",
        Some("Unusedcategories"),
        Backing::Projection,
        Measure::None,
    ),
    // Index-backed: redirects, page columns, properties.
    report(
        "ListRedirects",
        Some("Listredirects"),
        Backing::Index,
        Measure::None,
    ),
    report(
        "BrokenRedirects",
        Some("BrokenRedirects"),
        Backing::Index,
        Measure::None,
    ),
    report(
        "DoubleRedirects",
        Some("DoubleRedirects"),
        Backing::Index,
        Measure::None,
    ),
    report(
        "ShortPages",
        Some("Shortpages"),
        Backing::Index,
        Measure::Bytes,
    ),
    report(
        "LongPages",
        Some("Longpages"),
        Backing::Index,
        Measure::Bytes,
    ),
    report(
        "AncientPages",
        Some("Ancientpages"),
        Backing::Index,
        Measure::Time,
    ),
    report(
        "FewestRevisions",
        Some("Fewestrevisions"),
        Backing::Index,
        Measure::Count,
    ),
    report(
        "MostRevisions",
        Some("Mostrevisions"),
        Backing::Index,
        Measure::Count,
    ),
    report("ListProperties", None, Backing::Index, Measure::None),
];

/// The report a name names: the registry name or MediaWiki's, either case.
#[must_use]
pub fn by_name(name: &str) -> Option<&'static Report> {
    REPORTS.iter().find(|r| {
        r.name.eq_ignore_ascii_case(name)
            || r.mediawiki_name
                .is_some_and(|m| m.eq_ignore_ascii_case(name))
    })
}

/// The projection-backed reports.
fn projection_reports() -> impl Iterator<Item = &'static Report> {
    REPORTS.iter().filter(|r| r.backing == Backing::Projection)
}

/// What an anonymous reader cannot see, for the public form of entries (0047 §4.5): the
/// hidden namespaces and pages. A tenant-wide or `pages`-graph restriction is not applied
/// here; the report page is behind it instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Public {
    /// Hidden namespaces.
    pub namespaces: Vec<i32>,
    /// Hidden pages.
    pub pages: Vec<i64>,
    /// Hidden entities.
    pub entities: Vec<String>,
}

impl Public {
    /// The anonymous reader's hidden set in a tenant, now.
    ///
    /// # Errors
    ///
    /// The database's.
    pub async fn of<C: PgClient>(client: &C, tenant: &str) -> Result<Self, String> {
        let now = scatter_actors::Timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX)),
        );
        let h = crate::acl_read::hidden(client, tenant, &scatter_actors::Effective::default(), now)
            .await?;
        Ok(Self {
            namespaces: h.namespaces.iter().copied().collect(),
            pages: h.page_ids(),
            entities: h.entities.iter().cloned().collect(),
        })
    }
}

// --- Queueing -----------------------------------------------------------------------

/// Queues report work for a record's keys, if the tenant's reports are on. One statement;
/// nothing is written for a tenant whose reports have never been built.
///
/// # Errors
///
/// The database's.
pub async fn queue(
    cx: &PgCx,
    tenant: &str,
    partition: u64,
    offset: u64,
    keys: &BTreeSet<String>,
) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    let keys: Vec<&String> = keys.iter().collect();
    cx.execute(
        "INSERT INTO ops.projection_work (projection, tenant, partition, \"offset\", key)
         SELECT $1, $2, $3, $4, k FROM unnest($5::text[]) k
         WHERE EXISTS (SELECT 1 FROM view.report_state WHERE tenant = $2)",
        &[
            &PROJECTION,
            &tenant,
            &scatter_log_postgres::ids::partition_to_db(partition),
            &i64::try_from(offset).map_err(|e| e.to_string())?,
            &keys,
        ],
    )
    .await
    .map_err(|e| sql(&e))?;
    Ok(())
}

/// The work key of a title.
#[must_use]
pub fn title_key(ns: i32, title: &str) -> String {
    format!("title:{ns}:{title}")
}

/// The work key of a category.
#[must_use]
pub fn category_key(name: &str) -> String {
    format!("category:{name}")
}

// --- Per-key recompute ----------------------------------------------------------------

/// The page rows a recompute needs: visible means an anonymous reader may read it.
struct PageAt {
    page_id: i64,
    is_redirect: bool,
    visible: bool,
}

async fn page_at<C: PgClient>(
    c: &C,
    tenant: &str,
    public: &Public,
    ns: i32,
    title: &str,
) -> Result<Option<PageAt>, String> {
    let row = c
        .query_opt(
            "SELECT page_id, is_redirect,
                    NOT (ns = ANY($4) OR page_id = ANY($5)) AS visible
             FROM view.page
             WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted
               AND latest_offset IS NOT NULL",
            &[&tenant, &ns, &title, &public.namespaces, &public.pages],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.map(|r| PageAt {
        page_id: r.get(0),
        is_redirect: r.get(1),
        visible: r.get(2),
    }))
}

/// Visible pages linking to a title.
async fn incoming<C: PgClient>(
    c: &C,
    tenant: &str,
    public: &Public,
    key: &str,
) -> Result<i64, String> {
    let row = c
        .query_one(
            "SELECT COUNT(DISTINCT l.from_page) FROM view.page_link l
             JOIN view.page s ON s.tenant = l.tenant AND s.page_id = l.from_page
             WHERE l.tenant = $1 AND l.target_kind = 6 AND l.target_id = $2
               AND NOT s.deleted AND s.latest_offset IS NOT NULL
               AND NOT (s.ns = ANY($3) OR s.page_id = ANY($4))",
            &[&tenant, &key, &public.namespaces, &public.pages],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.get(0))
}

async fn put<C: PgClient>(
    c: &C,
    tenant: &str,
    report: &str,
    key: &str,
    listed: bool,
    sort: Option<i64>,
) -> Result<(), String> {
    match listed.then_some(sort) {
        Some(sort) => {
            let (ns, title) = key.split_once(':').unwrap_or(("0", key));
            let detail = json!({"ns": ns.parse::<i64>().unwrap_or(0), "title": title});
            c.execute(
                "INSERT INTO view.report_entry (tenant, report, scope, key, sort_value, detail)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (tenant, report, scope, key)
                 DO UPDATE SET sort_value = EXCLUDED.sort_value, detail = EXCLUDED.detail",
                &[&tenant, &report, &LOCAL, &key, &sort, &detail],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
        None => {
            c.execute(
                "DELETE FROM view.report_entry
                 WHERE tenant = $1 AND report = $2 AND scope = $3 AND key = $4",
                &[&tenant, &report, &LOCAL, &key],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
    }
    Ok(())
}

/// Recomputes every title-keyed entry for one title: whether it is wanted or most linked,
/// and, for the visible page holding it, whether it is a dead end, lonely, uncategorized,
/// or in many categories. A category page's title also recomputes its category.
///
/// # Errors
///
/// The database's.
pub async fn recompute_title<C: PgClient>(
    c: &C,
    tenant: &str,
    public: &Public,
    ns: i32,
    title: &str,
) -> Result<(), String> {
    let key = format!("{ns}:{title}");
    let page = page_at(c, tenant, public, ns, title).await?;
    let links_in = incoming(c, tenant, public, &key).await?;
    let wanted = page.is_none() && ns >= 0 && links_in > 0;
    put(c, tenant, "WantedPages", &key, wanted, Some(links_in)).await?;
    let most = links_in > 1 && page.as_ref().is_none_or(|p| p.visible);
    put(c, tenant, "MostLinkedPages", &key, most, Some(links_in)).await?;

    let (mut deadend, mut lonely, mut uncat, mut cats_n) = (false, false, false, 0i64);
    if let Some(p) = page.as_ref().filter(|p| p.visible) {
        let row = c
            .query_one(
                "SELECT (SELECT COUNT(*) FROM view.page_link WHERE tenant = $1 AND from_page = $2),
                        (SELECT COUNT(DISTINCT category) FROM view.page_category
                          WHERE tenant = $1 AND page_id = $2)",
                &[&tenant, &p.page_id],
            )
            .await
            .map_err(|e| sql(&e))?;
        let (out, cats): (i64, i64) = (row.get(0), row.get(1));
        let content = ns == 0 && !p.is_redirect;
        deadend = content && out == 0;
        lonely = content && links_in == 0;
        uncat = !p.is_redirect && cats == 0;
        cats_n = cats;
    }
    put(c, tenant, "DeadendPages", &key, deadend, None).await?;
    put(c, tenant, "LonelyPages", &key, lonely, None).await?;
    put(
        c,
        tenant,
        "UncategorizedPages",
        &key,
        uncat && ns == 0,
        None,
    )
    .await?;
    put(
        c,
        tenant,
        "UncategorizedCategories",
        &key,
        uncat && ns == 14,
        None,
    )
    .await?;
    put(
        c,
        tenant,
        "UncategorizedTemplates",
        &key,
        uncat && ns == 10,
        None,
    )
    .await?;
    put(c, tenant, "MostCategories", &key, cats_n > 1, Some(cats_n)).await?;
    if ns == 14 {
        recompute_category(c, tenant, public, title).await?;
    }
    Ok(())
}

/// Recomputes one category's entries: wanted (members, no page), most linked (members),
/// unused (a page, no members).
///
/// # Errors
///
/// The database's.
pub async fn recompute_category<C: PgClient>(
    c: &C,
    tenant: &str,
    public: &Public,
    name: &str,
) -> Result<(), String> {
    let key = format!("14:{name}");
    let members: i64 = c
        .query_one(
            "SELECT COUNT(DISTINCT m.page_id) FROM view.page_category m
             JOIN view.page s ON s.tenant = m.tenant AND s.page_id = m.page_id
             WHERE m.tenant = $1 AND m.category = $2
               AND NOT s.deleted AND s.latest_offset IS NOT NULL
               AND NOT (s.ns = ANY($3) OR s.page_id = ANY($4))",
            &[&tenant, &name, &public.namespaces, &public.pages],
        )
        .await
        .map_err(|e| sql(&e))?
        .get(0);
    let page = page_at(c, tenant, public, 14, name).await?;
    let wanted = page.is_none() && members > 0;
    put(c, tenant, "WantedCategories", &key, wanted, Some(members)).await?;
    let most = members > 0 && page.as_ref().is_none_or(|p| p.visible);
    put(c, tenant, "MostLinkedCategories", &key, most, Some(members)).await?;
    let unused = page.as_ref().is_some_and(|p| p.visible && !p.is_redirect) && members == 0;
    put(c, tenant, "UnusedCategories", &key, unused, None).await?;
    Ok(())
}

/// The keys a page whose visibility changed reaches: its title and its subpages', its link
/// targets and its categories.
async fn expand_page<C: PgClient>(
    c: &C,
    tenant: &str,
    page_id: i64,
) -> Result<BTreeSet<String>, String> {
    let mut keys = BTreeSet::new();
    let Some(row) = c
        .query_opt(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &page_id],
        )
        .await
        .map_err(|e| sql(&e))?
    else {
        return Ok(keys);
    };
    let (ns, title): (i32, String) = (row.get(0), row.get(1));
    let rows = c
        .query(
            "SELECT p.page_id, p.ns, p.title FROM view.page p
             WHERE p.tenant = $1 AND p.ns = $2 AND (p.title = $3 OR p.title LIKE $4)",
            &[&tenant, &ns, &title, &format!("{}/%", like_escape(&title))],
        )
        .await
        .map_err(|e| sql(&e))?;
    let ids: Vec<i64> = rows.iter().map(|r| r.get(0)).collect();
    for r in &rows {
        keys.insert(title_key(r.get(1), &r.get::<_, String>(2)));
    }
    for r in c
        .query(
            "SELECT target_id FROM view.page_link
             WHERE tenant = $1 AND from_page = ANY($2) AND target_kind = 6",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        keys.insert(format!("title:{}", r.get::<_, String>(0)));
    }
    for r in c
        .query(
            "SELECT DISTINCT category FROM view.page_category WHERE tenant = $1 AND page_id = ANY($2)",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        keys.insert(category_key(&r.get::<_, String>(0)));
    }
    Ok(keys)
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Recomputes one work key.
async fn apply_key<C: PgClient>(
    c: &C,
    tenant: &str,
    public: &Public,
    key: &str,
) -> Result<(), String> {
    if let Some(rest) = key.strip_prefix("title:") {
        if let Some((ns, title)) = rest.split_once(':')
            && let Ok(ns) = ns.parse::<i32>()
        {
            recompute_title(c, tenant, public, ns, title).await?;
        }
    } else if let Some(name) = key.strip_prefix("category:") {
        recompute_category(c, tenant, public, name).await?;
    }
    Ok(())
}

// --- Builds --------------------------------------------------------------------------

/// The set-based queries of a build: `(report, SQL)`, each selecting `(key, sort_value)`
/// with `$1` the tenant, `$2` the hidden namespaces and `$3` the hidden pages.
const BUILD: &[(&str, &str)] = &[
    (
        "WantedPages",
        "SELECT i.key, i.n FROM incoming i
         WHERE split_part(i.key, ':', 1)::int >= 0
           AND NOT EXISTS (SELECT 1 FROM alive a WHERE a.key = i.key)",
    ),
    (
        "MostLinkedPages",
        "SELECT i.key, i.n FROM incoming i
         WHERE i.n > 1 AND (NOT EXISTS (SELECT 1 FROM alive a WHERE a.key = i.key)
                            OR EXISTS (SELECT 1 FROM vis v WHERE v.key = i.key))",
    ),
    (
        "DeadendPages",
        "SELECT v.key, NULL::bigint FROM vis v
         WHERE v.ns = 0 AND NOT v.is_redirect
           AND NOT EXISTS (SELECT 1 FROM view.page_link l WHERE l.tenant = $1 AND l.from_page = v.page_id)",
    ),
    (
        "LonelyPages",
        "SELECT v.key, NULL::bigint FROM vis v
         WHERE v.ns = 0 AND NOT v.is_redirect
           AND NOT EXISTS (SELECT 1 FROM incoming i WHERE i.key = v.key)",
    ),
    (
        "UncategorizedPages",
        "SELECT v.key, NULL::bigint FROM vis v
         WHERE v.ns = 0 AND NOT v.is_redirect
           AND NOT EXISTS (SELECT 1 FROM view.page_category c WHERE c.tenant = $1 AND c.page_id = v.page_id)",
    ),
    (
        "UncategorizedCategories",
        "SELECT v.key, NULL::bigint FROM vis v
         WHERE v.ns = 14 AND NOT v.is_redirect
           AND NOT EXISTS (SELECT 1 FROM view.page_category c WHERE c.tenant = $1 AND c.page_id = v.page_id)",
    ),
    (
        "UncategorizedTemplates",
        "SELECT v.key, NULL::bigint FROM vis v
         WHERE v.ns = 10 AND NOT v.is_redirect
           AND NOT EXISTS (SELECT 1 FROM view.page_category c WHERE c.tenant = $1 AND c.page_id = v.page_id)",
    ),
    (
        "MostCategories",
        "SELECT v.key, n FROM vis v
         JOIN LATERAL (SELECT COUNT(DISTINCT c.category) AS n FROM view.page_category c
                       WHERE c.tenant = $1 AND c.page_id = v.page_id) x ON true
         WHERE n > 1",
    ),
    (
        "WantedCategories",
        "SELECT '14:' || m.category, m.n FROM members m
         WHERE NOT EXISTS (SELECT 1 FROM alive a WHERE a.key = '14:' || m.category)",
    ),
    (
        "MostLinkedCategories",
        "SELECT '14:' || m.category, m.n FROM members m
         WHERE NOT EXISTS (SELECT 1 FROM alive a WHERE a.key = '14:' || m.category)
            OR EXISTS (SELECT 1 FROM vis v WHERE v.key = '14:' || m.category)",
    ),
    (
        "UnusedCategories",
        "SELECT v.key, NULL::bigint FROM vis v
         WHERE v.ns = 14 AND NOT v.is_redirect
           AND NOT EXISTS (SELECT 1 FROM members m WHERE m.category = v.title)",
    ),
];

/// The common tables of a build's queries.
const BUILD_WITH: &str = "WITH
  alive AS (SELECT ns::text || ':' || title AS key FROM view.page
            WHERE tenant = $1 AND NOT deleted AND latest_offset IS NOT NULL),
  vis AS (SELECT page_id, ns, title, is_redirect, ns::text || ':' || title AS key FROM view.page
          WHERE tenant = $1 AND NOT deleted AND latest_offset IS NOT NULL
            AND NOT (ns = ANY($2) OR page_id = ANY($3))),
  incoming AS (SELECT l.target_id AS key, COUNT(DISTINCT l.from_page) AS n
               FROM view.page_link l JOIN vis s ON s.page_id = l.from_page
               WHERE l.tenant = $1 AND l.target_kind = 6 GROUP BY l.target_id),
  members AS (SELECT c.category, COUNT(DISTINCT c.page_id) AS n
              FROM view.page_category c JOIN vis s ON s.page_id = c.page_id
              WHERE c.tenant = $1 GROUP BY c.category)
";

/// Rebuilds every projection-backed report of a tenant from `view`, in one transaction,
/// and records each report's state. The named operation "rebuild projection `report` for
/// tenant T from `view`" (03 §6.1).
///
/// # Errors
///
/// The database's.
pub async fn rebuild(pool: &Pool, tenant: &str) -> Result<u64, String> {
    let mut conn = pool.get().await.map_err(|e| e.to_string())?;
    let tx = conn.transaction().await.map_err(|e| sql(&e))?;
    let public = Public::of(&*tx, tenant).await?;
    tx.execute(
        "DELETE FROM view.report_entry WHERE tenant = $1 AND scope = $2",
        &[&tenant, &LOCAL],
    )
    .await
    .map_err(|e| sql(&e))?;
    let mut total = 0u64;
    for (report, select) in BUILD {
        let n = tx
            .execute(
                &format!(
                    "{BUILD_WITH}
                     INSERT INTO view.report_entry (tenant, report, scope, key, sort_value, detail)
                     SELECT $1, '{report}', 'local', q.key, q.sort,
                            jsonb_build_object('ns', split_part(q.key, ':', 1)::int,
                                               'title', substr(q.key, strpos(q.key, ':') + 1))
                     FROM ({select}) AS q(key, sort)"
                ),
                &[&tenant, &public.namespaces, &public.pages],
            )
            .await
            .map_err(|e| sql(&e))?;
        tx.execute(
            "INSERT INTO view.report_state (tenant, report, scope, mode, computed_at, row_count)
             VALUES ($1, $2, $3, 'projection', now(), $4)
             ON CONFLICT (tenant, report, scope)
             DO UPDATE SET mode = 'projection', computed_at = now(), row_count = EXCLUDED.row_count",
            &[&tenant, report, &LOCAL, &i64::try_from(n).unwrap_or(i64::MAX)],
        )
        .await
        .map_err(|e| sql(&e))?;
        total += n;
    }
    tx.commit().await.map_err(|e| sql(&e))?;
    Ok(total)
}

/// Turns a tenant's reports on, building them, unless they are on already. The state rows
/// are committed before the build reads `view`, so a page written meanwhile either is in
/// the build or queues its work, and none is lost between the two.
///
/// # Errors
///
/// The database's.
pub async fn ensure_built(pool: &Pool, tenant: &str) -> Result<bool, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let on = conn
        .query_opt(
            "SELECT 1 FROM view.report_state WHERE tenant = $1 LIMIT 1",
            &[&tenant],
        )
        .await
        .map_err(|e| sql(&e))?
        .is_some();
    if on {
        return Ok(false);
    }
    for r in projection_reports() {
        conn.execute(
            "INSERT INTO view.report_state (tenant, report, scope, mode)
             VALUES ($1, $2, $3, 'projection') ON CONFLICT DO NOTHING",
            &[&tenant, &r.name, &LOCAL],
        )
        .await
        .map_err(|e| sql(&e))?;
    }
    drop(conn);
    rebuild(pool, tenant).await?;
    Ok(true)
}

/// Applies up to `limit` items of queued report work, oldest first, in batches of one
/// transaction each; returns how many it applied. Keys repeated in a batch are computed
/// once; `tenant` rebuilds the tenant instead.
///
/// # Errors
///
/// The database's.
pub async fn drain(pool: &Pool, tenant: Option<&str>, limit: usize) -> Result<usize, String> {
    let mut done = 0;
    while done < limit {
        let batch = (limit - done).min(500);
        let mut conn = pool.get().await.map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| sql(&e))?;
        let rows = tx
            .query(
                "DELETE FROM ops.projection_work
                 WHERE id IN (SELECT id FROM ops.projection_work
                              WHERE projection = $1 AND ($2::text IS NULL OR tenant = $2)
                              ORDER BY id LIMIT $3 FOR UPDATE SKIP LOCKED)
                 RETURNING tenant, key",
                &[
                    &PROJECTION,
                    &tenant,
                    &i64::try_from(batch).unwrap_or(i64::MAX),
                ],
            )
            .await
            .map_err(|e| sql(&e))?;
        if rows.is_empty() {
            break;
        }
        done += rows.len();
        let mut by_tenant: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in &rows {
            by_tenant.entry(r.get(0)).or_default().insert(r.get(1));
        }
        let mut rebuilds = Vec::new();
        for (t, keys) in by_tenant {
            if keys.contains("tenant") {
                rebuilds.push(t);
                continue;
            }
            let public = Public::of(&*tx, &t).await?;
            let mut all = BTreeSet::new();
            for k in keys {
                if let Some(id) = k.strip_prefix("page:").and_then(|i| i.parse::<i64>().ok()) {
                    all.extend(expand_page(&*tx, &t, id).await?);
                } else {
                    all.insert(k);
                }
            }
            for k in &all {
                apply_key(&*tx, &t, &public, k).await?;
            }
        }
        tx.commit().await.map_err(|e| sql(&e))?;
        drop(conn);
        for t in rebuilds {
            rebuild(pool, &t).await?;
        }
    }
    Ok(done)
}

// --- Reading -------------------------------------------------------------------------

/// One row of a report, as `list=querypage` and `GET /reports/{name}` give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The namespace of the page or title listed.
    pub ns: i32,
    /// Its title, without the namespace.
    pub title: String,
    /// The value: a count, bytes, or a time (ISO 8601), as a string; empty when none.
    pub value: String,
    /// More: a redirect's target, a double redirect's middle and end, a label.
    pub extra: Value,
}

/// A report's state in a tenant, for `GET /reports` and the report page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// `index`, `projection` or `batch`.
    pub mode: String,
    /// When its last full build finished.
    pub computed_at: Option<String>,
    /// How many entries it has.
    pub row_count: Option<i64>,
    /// Work queued for the tenant's reports and not yet applied.
    pub waiting: i64,
}

/// A report's state.
///
/// # Errors
///
/// The database's.
pub async fn state<C: PgClient>(c: &C, tenant: &str, report: &Report) -> Result<State, String> {
    let waiting: i64 = c
        .query_one(
            "SELECT COUNT(*) FROM ops.projection_work WHERE projection = $1 AND tenant = $2",
            &[&PROJECTION, &tenant],
        )
        .await
        .map_err(|e| sql(&e))?
        .get(0);
    if report.backing == Backing::Index {
        return Ok(State {
            mode: "index".into(),
            computed_at: None,
            row_count: None,
            waiting: 0,
        });
    }
    let row = c
        .query_opt(
            "SELECT mode, to_char(computed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                    (SELECT COUNT(*) FROM view.report_entry e
                      WHERE e.tenant = s.tenant AND e.report = s.report AND e.scope = s.scope)
             FROM view.report_state s WHERE tenant = $1 AND report = $2 AND scope = $3",
            &[&tenant, &report.name, &LOCAL],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(match row {
        Some(r) => State {
            mode: r.get(0),
            computed_at: r.get(1),
            row_count: Some(r.get(2)),
            waiting,
        },
        None => State {
            mode: "projection".into(),
            computed_at: None,
            row_count: None,
            waiting,
        },
    })
}

/// A page of a report: up to `limit` rows from `offset`, and whether there are more.
///
/// # Errors
///
/// The database's.
pub async fn read<C: PgClient>(
    c: &C,
    tenant: &str,
    report: &Report,
    offset: i64,
    limit: i64,
    lang: &str,
) -> Result<(Vec<Row>, bool), String> {
    let public = Public::of(c, tenant).await?;
    let mut rows = match report.backing {
        Backing::Projection => c
            .query(
                "SELECT (detail->>'ns')::int, detail->>'title', COALESCE(sort_value::text, '')
                 FROM view.report_entry WHERE tenant = $1 AND report = $2 AND scope = $3
                 ORDER BY sort_value DESC, key OFFSET $4 LIMIT $5",
                &[&tenant, &report.name, &LOCAL, &offset, &(limit + 1)],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| Row {
                ns: r.get(0),
                title: r.get(1),
                value: r.get(2),
                extra: Value::Null,
            })
            .collect::<Vec<_>>(),
        Backing::Index => read_index(c, tenant, &public, report, offset, limit + 1, lang).await?,
    };
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok((rows, more))
}

/// The visible-page condition on `p`, with `$2` and `$3` the hidden namespaces and pages.
const VISIBLE: &str = "p.tenant = $1 AND NOT p.deleted AND p.latest_offset IS NOT NULL
                       AND NOT (p.ns = ANY($2) OR p.page_id = ANY($3))";

#[allow(clippy::too_many_lines)]
async fn read_index<C: PgClient>(
    c: &C,
    tenant: &str,
    public: &Public,
    report: &Report,
    offset: i64,
    limit: i64,
    lang: &str,
) -> Result<Vec<Row>, String> {
    let page_column = |order: &str, value: &str| {
        format!(
            "SELECT p.ns, p.title, {value} FROM view.page p
             WHERE {VISIBLE} AND p.ns = 0 AND NOT p.is_redirect
             ORDER BY {order}, p.title COLLATE \"C\" OFFSET $4 LIMIT $5"
        )
    };
    let simple = |q: String| async move {
        Ok::<_, String>(
            c.query(
                &q,
                &[&tenant, &public.namespaces, &public.pages, &offset, &limit],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| Row {
                ns: r.get(0),
                title: r.get(1),
                value: r.get(2),
                extra: Value::Null,
            })
            .collect::<Vec<_>>(),
        )
    };
    match report.name {
        "ShortPages" => simple(page_column("p.len ASC NULLS FIRST", "COALESCE(p.len, 0)::text")).await,
        "LongPages" => simple(page_column("p.len DESC NULLS LAST", "COALESCE(p.len, 0)::text")).await,
        "AncientPages" => {
            simple(page_column(
                "p.latest_at ASC NULLS FIRST",
                "COALESCE(to_char(p.latest_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), '')",
            ))
            .await
        }
        "FewestRevisions" => simple(page_column("p.revisions ASC", "p.revisions::text")).await,
        "MostRevisions" => simple(page_column("p.revisions DESC", "p.revisions::text")).await,
        "ListRedirects" | "BrokenRedirects" | "DoubleRedirects" => {
            let condition = match report.name {
                "BrokenRedirects" => {
                    "AND r.interwiki IS NULL AND r.target_ns >= 0 AND NOT EXISTS (
                       SELECT 1 FROM view.page t WHERE t.tenant = $1 AND t.ns = r.target_ns
                         AND t.title = r.target_title AND NOT t.deleted AND t.latest_offset IS NOT NULL)"
                }
                "DoubleRedirects" => "AND m.page_id IS NOT NULL",
                _ => "",
            };
            let q = format!(
                "SELECT p.ns, p.title, r.target_ns, r.target_title, r.interwiki, r.fragment,
                        m.ns, m.title, r2.target_ns, r2.target_title, r2.interwiki
                 FROM view.redirect r
                 JOIN view.page p ON p.tenant = r.tenant AND p.page_id = r.page_id
                 LEFT JOIN view.page m ON m.tenant = r.tenant AND m.ns = r.target_ns
                      AND m.title = r.target_title AND m.is_redirect AND NOT m.deleted
                      AND m.latest_offset IS NOT NULL
                 LEFT JOIN view.redirect r2 ON r2.tenant = m.tenant AND r2.page_id = m.page_id
                 WHERE r.tenant = $1 AND {VISIBLE} {condition}
                 ORDER BY p.ns, p.title COLLATE \"C\" OFFSET $4 LIMIT $5"
            );
            Ok(c
                .query(
                    &q,
                    &[&tenant, &public.namespaces, &public.pages, &offset, &limit],
                )
                .await
                .map_err(|e| sql(&e))?
                .iter()
                .map(|r| {
                    let target = json!({
                        "ns": r.get::<_, Option<i32>>(2),
                        "title": r.get::<_, String>(3),
                        "interwiki": r.get::<_, Option<String>>(4),
                        "fragment": r.get::<_, Option<String>>(5),
                    });
                    let mut extra = json!({"target": target});
                    if report.name == "DoubleRedirects" {
                        extra["final"] = json!({
                            "ns": r.get::<_, Option<i32>>(8),
                            "title": r.get::<_, Option<String>>(9),
                            "interwiki": r.get::<_, Option<String>>(10),
                        });
                    }
                    Row {
                        ns: r.get(0),
                        title: r.get(1),
                        value: String::new(),
                        extra,
                    }
                })
                .collect())
        }
        "ListProperties" => {
            // Local properties (0047 §4.2), numbered, with their label in the language.
            let rows = c
                .query(
                    "SELECT e.id,
                       (SELECT text FROM view.term t WHERE t.tenant = e.tenant AND t.entity_id = e.id
                          AND t.kind = 1 AND t.lang = $5 LIMIT 1)
                     FROM view.entity e
                     WHERE e.tenant = $1 AND e.type = 'property' AND NOT e.deleted
                       AND e.id ~ '^P[0-9]+$' AND NOT (e.id = ANY($2))
                     ORDER BY substr(e.id, 2)::bigint OFFSET $3 LIMIT $4",
                    &[&tenant, &public.entities, &offset, &limit, &lang],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(rows
                .iter()
                .map(|r| Row {
                    ns: 122,
                    title: r.get(0),
                    value: String::new(),
                    extra: json!({"label": r.get::<_, Option<String>>(1)}),
                })
                .collect())
        }
        other => Err(format!("reports: no reader for `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct File {
        page: Vec<Entry>,
    }

    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        mediawiki_name: Option<String>,
        status: String,
        report: Option<toml::Value>,
    }

    /// The catalogue agrees with the registry, which is the authority: every report here
    /// is a served report there, with its MediaWiki name and its backing.
    #[test]
    fn the_catalogue_follows_the_registry() {
        let f: File = toml::from_str(include_str!("../../../docs/registry/special-pages.toml"))
            .expect("special-pages.toml parses");
        for r in REPORTS {
            let e = f
                .page
                .iter()
                .find(|e| e.name == r.name)
                .unwrap_or_else(|| panic!("{} is in the registry", r.name));
            assert_eq!(e.status, "served", "{}", r.name);
            assert_eq!(e.mediawiki_name.as_deref(), r.mediawiki_name, "{}", r.name);
            let backing = e
                .report
                .as_ref()
                .and_then(|t| t.get("backing"))
                .and_then(toml::Value::as_str);
            assert_eq!(backing, Some(r.backing.name()), "{}", r.name);
        }
        for (name, _) in BUILD {
            assert_eq!(
                by_name(name).map(|r| r.backing),
                Some(Backing::Projection),
                "{name}"
            );
        }
        assert_eq!(BUILD.len(), projection_reports().count());
        assert_eq!(by_name("wantedpages").map(|r| r.name), Some("WantedPages"));
        assert_eq!(
            by_name("Mostlinked").map(|r| r.name),
            Some("MostLinkedPages")
        );
        assert!(
            by_name("WantedTemplates").is_none(),
            "not served by this build"
        );
    }
}
