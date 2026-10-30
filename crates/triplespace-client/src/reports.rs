//! Reports through the Action API (0047 §4, §11): one page of a report's rows from
//! `list=querypage`, with the report's state (`ts-state`), for the site's report pages.

use serde_json::Value;

use crate::api::{CacheInfo, Fetched, check};
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// What a report page asks: the report, where to start, how many.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportQuery {
    /// `qppage`: the report's name.
    pub name: String,
    /// `qpoffset`.
    pub offset: u32,
    /// `qplimit`.
    pub limit: u32,
}

/// A report's state, as `ts-state` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportState {
    /// `index` (a live query) or `projection` (kept as pages change).
    pub backing: String,
    /// What a row's value is: `none`, `count`, `bytes` or `time`.
    pub measure: String,
    /// `index`, `projection` or `batch`.
    pub mode: String,
    /// When its last full build finished, ISO 8601.
    pub computed_at: Option<String>,
    /// Changes queued for the wiki's reports and not yet applied.
    pub waiting: i64,
}

/// A row of a report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportRow {
    /// The page or title, prefixed.
    pub title: String,
    /// Its namespace.
    pub ns: i64,
    /// The value: a count, bytes, or a time; empty when the report has none.
    pub value: String,
    /// A redirect's target, prefixed (or `interwiki:title`).
    pub target: Option<String>,
    /// The target's section.
    pub fragment: Option<String>,
    /// A double redirect's final target.
    pub last: Option<String>,
    /// A property's label.
    pub label: Option<String>,
}

/// A page of a report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportPage {
    /// The state.
    pub state: ReportState,
    /// The rows.
    pub rows: Vec<ReportRow>,
    /// The offset the next page starts at, if one follows.
    pub next: Option<u32>,
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

impl Client {
    /// `list=querypage`: a page of a report's rows, and the report's state.
    ///
    /// # Errors
    ///
    /// The API's refusal (`ts-report-unavailable`, `permissiondenied`), or the API away or
    /// unreadable.
    pub async fn report(
        &self,
        incoming: &Incoming,
        ask: &ReportQuery,
    ) -> Result<Fetched<ReportPage>, ClientError> {
        let name: String = url::form_urlencoded::byte_serialize(ask.name.as_bytes()).collect();
        let path = format!(
            "/w/api.php?action=query&list=querypage&qppage={name}&qpoffset={}&qplimit={}&format=json&formatversion=2",
            ask.offset, ask.limit
        );
        let r = self.get(incoming, &path).await?;
        let v = check(&r)?;
        let qp = &v["query"]["querypage"];
        let Some(results) = qp["results"].as_array() else {
            return Err(ClientError::Unexpected(
                "`list=querypage` answered without `results`".into(),
            ));
        };
        let rows = results
            .iter()
            .map(|row| ReportRow {
                title: text(&row["title"]).unwrap_or_default(),
                ns: row["ns"].as_i64().unwrap_or_default(),
                value: match &row["value"] {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                },
                target: text(&row["ts-target"]),
                fragment: text(&row["ts-fragment"]),
                last: text(&row["ts-final"]),
                label: text(&row["ts-label"]),
            })
            .collect();
        let s = &qp["ts-state"];
        let state = ReportState {
            backing: text(&s["backing"]).unwrap_or_default(),
            measure: text(&s["measure"]).unwrap_or_else(|| "none".into()),
            mode: text(&s["mode"]).unwrap_or_default(),
            computed_at: text(&s["computed_at"]),
            waiting: s["waiting"].as_i64().unwrap_or_default(),
        };
        let next = v["continue"]["qpoffset"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok());
        Ok(Fetched {
            value: ReportPage { state, rows, next },
            cache: CacheInfo::of(&r),
        })
    }
}
