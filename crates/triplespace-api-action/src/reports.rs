//! Serving reports (0047 §4, §11; 21 §2): what `list=querypage` and `GET /reports/{name}`
//! share. The reports and their storage are [`triplespace_projections::reports`].
//!
//! - **Who may read one.** Entries are stored in their public form (0047 §4.5), so every
//!   viewer who may read the tenant's pages sees the same report. A caller to whom every
//!   page is hidden (an outsider of a private tenant, or of a restricted `pages` graph) is
//!   refused, since the report page is behind that restriction.
//! - **A projection report is built on first read**, then kept by the worker (03 §6.1),
//!   never by a read: the report's state says how much work is waiting, and the page
//!   shows it.

use scatter_actors::Effective;
use serde_json::{Map, Value, json};
use triplespace_projections::reports::{self, Backing, Report, Row, State};

use crate::App;
use crate::pages::Site;
use crate::response::ApiError;

/// The most rows one read returns.
pub const MAX_LIMIT: i64 = 500;

/// Which rows of a report: from `offset`, `limit` of them, labels in `lang`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// The first row, from 0.
    pub offset: i64,
    /// How many, at most [`MAX_LIMIT`].
    pub limit: i64,
    /// The language of labels.
    pub lang: String,
}

/// A page of a report.
#[derive(Debug, Clone)]
pub struct Page {
    /// The report.
    pub report: &'static Report,
    /// Its state.
    pub state: State,
    /// The rows.
    pub rows: Vec<Row>,
    /// Whether more follow.
    pub more: bool,
}

/// A report by the name a request gives, or the error that says it is not served.
///
/// # Errors
///
/// `ts-report-unavailable` for a name this build serves no report under.
pub fn named(name: &str) -> Result<&'static Report, ApiError> {
    reports::by_name(name).ok_or_else(|| {
        ApiError::new(
            "ts-report-unavailable",
            format!("No report called \"{name}\" is served by this instance."),
        )
    })
}

/// A page of a report for a caller.
///
/// # Errors
///
/// `permissiondenied` for a caller to whom every page is hidden; the store's error.
pub async fn page(
    app: &App,
    db: &tokio_postgres::Client,
    tenant: &str,
    effective: &Effective,
    report: &'static Report,
    window: &Window,
) -> Result<Page, ApiError> {
    let hidden = triplespace_projections::acl_read::hidden(
        db,
        tenant,
        effective,
        scatter_actors::Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)?;
    if hidden.every_page() {
        return Err(ApiError::permission_denied("read this wiki's reports"));
    }
    if report.backing == Backing::Projection {
        reports::ensure_built(app.pool(), tenant)
            .await
            .map_err(ApiError::internal)?;
    }
    let state = reports::state(db, tenant, report)
        .await
        .map_err(ApiError::internal)?;
    let (rows, more) = reports::read(
        db,
        tenant,
        report,
        window.offset,
        window.limit.clamp(1, MAX_LIMIT),
        &window.lang,
    )
    .await
    .map_err(ApiError::internal)?;
    Ok(Page {
        report,
        state,
        rows,
        more,
    })
}

/// A row in MediaWiki's `list=querypage` shape: `value`, `ns` and `title` (prefixed), and
/// Triplespace's fields for what MediaWiki puts elsewhere: a redirect's target, a double
/// redirect's end, a property's label.
#[must_use]
pub fn row_json(site: &Site, row: &Row) -> Value {
    let mut m = Map::new();
    m.insert("value".into(), json!(row.value));
    m.insert("ns".into(), json!(row.ns));
    m.insert("title".into(), json!(site.prefixed(row.ns, &row.title)));
    let titled = |v: &Value| -> Option<String> {
        let title = v["title"].as_str()?;
        if let Some(iw) = v["interwiki"].as_str() {
            return Some(format!("{iw}:{title}"));
        }
        let ns = i32::try_from(v["ns"].as_i64()?).ok()?;
        Some(site.prefixed(ns, title))
    };
    if let Some(t) = titled(&row.extra["target"]) {
        m.insert("ts-target".into(), json!(t));
        if let Some(f) = row.extra["target"]["fragment"].as_str() {
            m.insert("ts-fragment".into(), json!(f));
        }
    }
    if let Some(t) = titled(&row.extra["final"]) {
        m.insert("ts-final".into(), json!(t));
    }
    if let Some(l) = row.extra["label"].as_str() {
        m.insert("ts-label".into(), json!(l));
    }
    Value::Object(m)
}

/// A report's state as `GET /reports` and `list=querypage` report it.
#[must_use]
pub fn state_json(report: &Report, state: &State) -> Value {
    json!({
        "name": report.name,
        "mediawiki_name": report.mediawiki_name,
        "backing": report.backing.name(),
        "measure": report.value.name(),
        "mode": state.mode,
        "scope": reports::LOCAL,
        "computed_at": state.computed_at,
        "row_count": state.row_count,
        "waiting": state.waiting,
    })
}
