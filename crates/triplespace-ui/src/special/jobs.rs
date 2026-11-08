//! `Special:Jobs` and `Special:Jobs/{id}` (0010 §9; 0104 §9; 19 §3.10): the running and
//! recent jobs, and one job's page, from `GET /jobs` and `GET /jobs/{id}` (18 §3.2). A
//! job's page has the parts 19 §3.10 lists: the identity line with the Job chip, the ID
//! and the status; the header with the actor, the account it ran for, the times and the
//! mode; the counts; the rejects; where it came from. A queued job shows its place in
//! the queue, a running one the runner's progress. The actions (Revert, Reject, Close)
//! wait on the routes of 0104 §3, which are not served yet.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::{ClientError, JobInfo};

use crate::Site;
use crate::codex::{Chip, ChipKind, ChipModel, MessageKind};
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::{clock, date};

const TITLE: &str = "Special:Jobs";

/// The statuses a job may be in, with their messages.
const STATUSES: &[(&str, &str)] = &[
    ("queued", "ts-job-status-queued"),
    ("running", "ts-job-status-running"),
    ("finished", "ts-job-status-finished"),
    ("failed", "ts-job-status-failed"),
    ("interrupted", "ts-job-status-interrupted"),
    ("reverted", "ts-job-status-reverted"),
    ("rejected", "ts-job-status-rejected"),
    ("closed", "ts-job-status-closed"),
];

/// The count fields of a finished job, in the order shown.
const COUNTS: &[(&str, &str)] = &[
    ("created", "ts-job-count-created"),
    ("merged", "ts-job-count-merged"),
    ("unchanged", "ts-job-count-unchanged"),
    ("rejected", "ts-job-count-rejected"),
    ("adopted", "ts-job-count-adopted"),
    ("tombstoned", "ts-job-count-tombstoned"),
    ("cleared", "ts-job-count-cleared"),
    ("other", "ts-job-count-other"),
];

/// An import's own counts, under `counts.import` (0008 §9).
const IMPORT_COUNTS: &[(&str, &str)] = &[
    ("revisions", "ts-job-count-revisions"),
    ("present", "ts-job-count-present"),
    ("restored", "ts-job-count-restored"),
];

/// A link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    /// Where.
    pub href: String,
    /// The text.
    pub text: String,
}

/// The job page's URL.
#[must_use]
pub fn href(id: u64) -> String {
    format!("/wiki/Special:Jobs/{id}")
}

/// A job's status as shown.
fn status_text(m: &Messages, status: &str) -> String {
    STATUSES
        .iter()
        .find(|(s, _)| *s == status)
        .map_or_else(|| status.to_string(), |(_, key)| m.get(key))
}

/// A time as shown: the clock and the date.
fn when(m: &Messages, iso: &str) -> String {
    format!("{}, {}", clock(iso), date(m, iso))
}

/// The actor, linked to their contributions, or the key alone.
fn actor_link(info: &JobInfo) -> Link {
    match &info.actor.name {
        Some(name) => Link {
            href: format!("/wiki/Special:Contributions/{}", title_url(name)),
            text: name.clone(),
        },
        None => Link {
            href: format!("/wiki/Special:Contributions/{}", title_url(&info.actor.key)),
            text: info.actor.key.clone(),
        },
    }
}

/// One row of `Special:Jobs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobRow {
    /// The job's page.
    pub job: Link,
    /// The status.
    pub status: String,
    /// The status's class suffix.
    pub status_class: String,
    /// The mode or kind.
    pub mode: String,
    /// The source and its version.
    pub source: String,
    /// The actor.
    pub actor: Link,
    /// When it was submitted or started.
    pub started: String,
    /// When it ended.
    pub finished: String,
    /// A queued job's place, or a running job's progress.
    pub note: Option<String>,
}

/// `Special:Jobs` as Plain's `special/jobs.html` draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobsPage {
    /// What the page lists.
    pub intro: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// The rows.
    pub rows: Vec<JobRow>,
}

/// The running job's progress, as said.
fn progress_text(m: &Messages, info: &JobInfo) -> Option<String> {
    let p = info.progress.as_ref()?;
    let stage = p.get("stage").and_then(|s| s.as_str())?;
    let stage = m.get(&format!("ts-job-stage-{stage}"));
    let pages = p
        .get("pages")
        .and_then(serde_json::Value::as_u64)
        .map(|n| n.to_string())
        .unwrap_or_default();
    Some(m.with("ts-job-progress", &[&stage, &pages]))
}

fn source_text(info: &JobInfo) -> String {
    match (&info.source, &info.version) {
        (Some(s), Some(v)) if !v.is_empty() => format!("{s} ({v})"),
        (Some(s), _) => s.clone(),
        (None, _) => String::new(),
    }
}

fn row_model(m: &Messages, info: &JobInfo) -> JobRow {
    let note = match info.status.as_str() {
        "queued" => info
            .position
            .map(|p| m.with("ts-job-position", &[&p.to_string()])),
        "running" => progress_text(m, info),
        _ => None,
    };
    JobRow {
        job: Link {
            href: href(info.id),
            text: m.with("ts-changes-job", &[&info.id.to_string()]),
        },
        status: status_text(m, &info.status),
        status_class: info.status.clone(),
        mode: info
            .mode
            .clone()
            .or_else(|| info.kind.clone())
            .unwrap_or_default(),
        source: source_text(info),
        actor: actor_link(info),
        started: info
            .started
            .as_deref()
            .or(info.submitted.as_deref())
            .map(|t| when(m, t))
            .unwrap_or_default(),
        finished: info
            .finished
            .as_deref()
            .map(|t| when(m, t))
            .unwrap_or_default(),
        note,
    }
}

/// The list's model.
#[must_use]
pub fn list_model(m: &Messages, jobs: &[JobInfo]) -> JobsPage {
    JobsPage {
        intro: m.get("ts-jobs-intro"),
        nothing: jobs.is_empty().then(|| m.get("ts-jobs-none")),
        columns: [
            "ts-jobs-col-job",
            "ts-jobs-col-status",
            "ts-jobs-col-mode",
            "ts-jobs-col-source",
            "ts-jobs-col-actor",
            "ts-jobs-col-started",
            "ts-jobs-col-finished",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: jobs.iter().map(|j| row_model(m, j)).collect(),
    }
}

/// A fact on the job page: a label and its value, the value a link where it is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact {
    /// The label.
    pub label: String,
    /// The value.
    pub value: String,
    /// Where the value links.
    pub href: Option<String>,
    /// Shown in monospace.
    pub mono: bool,
}

/// One reject as shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RejectRow {
    /// The line.
    pub line: String,
    /// The match key.
    pub key: String,
    /// The reason.
    pub reason: String,
}

/// `Special:Jobs/{id}` as Plain's `special/job.html` draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobPage {
    /// The Job chip.
    pub chip: ChipModel,
    /// "Job n".
    pub name: String,
    /// The status.
    pub status: String,
    /// The status's class suffix.
    pub status_class: String,
    /// A failed job's error, an interrupted job's note, a queued job's place or a running
    /// job's progress, as a message.
    pub notice: Option<String>,
    /// The header's facts.
    pub header: Vec<Fact>,
    /// The counts heading.
    pub counts_heading: String,
    /// The counts.
    pub counts: Vec<Fact>,
    /// The rejects heading.
    pub rejects_heading: String,
    /// The reject columns.
    pub reject_columns: Vec<String>,
    /// The rejects.
    pub rejects: Vec<RejectRow>,
    /// The origin heading.
    pub origin_heading: String,
    /// Where it came from.
    pub origin: Vec<Fact>,
    /// The link to the job's changes.
    pub changes: Link,
    /// Back to the list.
    pub list: Link,
}

fn fact(label: String, value: String) -> Fact {
    Fact {
        label,
        value,
        href: None,
        mono: false,
    }
}

/// The job page's model.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn job_model(m: &Messages, info: &JobInfo) -> JobPage {
    let actor = actor_link(info);
    let mut header = vec![Fact {
        label: m.get("ts-job-actor"),
        value: actor.text,
        href: Some(actor.href),
        mono: false,
    }];
    if let Some(r) = &info.requested_by {
        header.push(Fact {
            label: m.get("ts-job-requested-by"),
            value: r.clone(),
            href: Some(format!("/wiki/Special:Contributions/{}", title_url(r))),
            mono: false,
        });
    }
    if let Some(t) = &info.submitted {
        header.push(fact(m.get("ts-job-submitted"), when(m, t)));
    }
    if let Some(t) = &info.started {
        header.push(fact(m.get("ts-job-started"), when(m, t)));
    }
    if let Some(t) = &info.finished {
        header.push(fact(m.get("ts-job-finished"), when(m, t)));
    }
    if let Some(mode) = info.mode.as_ref().or(info.kind.as_ref()) {
        header.push(Fact {
            label: m.get("ts-job-mode"),
            value: mode.clone(),
            href: None,
            mono: true,
        });
    }
    if let Some(a) = info.attempts.filter(|a| *a > 1) {
        header.push(fact(m.get("ts-job-attempts"), a.to_string()));
    }
    let notice = match info.status.as_str() {
        "failed" => Some(m.with("ts-job-error", &[info.error.as_deref().unwrap_or_default()])),
        "interrupted" => Some(match info.interrupted_by {
            Some(by) => m.with("ts-job-interrupted-by", &[&by.to_string()]),
            None => m.get("ts-job-interrupted"),
        }),
        "queued" => info
            .position
            .map(|p| m.with("ts-job-position", &[&p.to_string()])),
        "running" => progress_text(m, info),
        _ => None,
    };
    let counted = |fields: &[(&str, &str)], c: &serde_json::Map<String, serde_json::Value>| {
        fields
            .iter()
            .filter_map(|(field, key)| {
                c.get(*field)
                    .and_then(serde_json::Value::as_u64)
                    .map(|n| fact(m.get(key), n.to_string()))
            })
            .collect::<Vec<_>>()
    };
    let mut counts: Vec<Fact> = info
        .counts
        .as_ref()
        .and_then(|c| c.as_object())
        .map(|c| counted(COUNTS, c))
        .unwrap_or_default();
    if let Some(i) = info
        .counts
        .as_ref()
        .and_then(|c| c.get("import"))
        .and_then(|c| c.as_object())
    {
        counts.extend(counted(IMPORT_COUNTS, i));
    }
    let mut origin = Vec::new();
    if let Some(s) = &info.source {
        origin.push(fact(m.get("ts-job-source"), s.clone()));
    }
    if let Some(v) = info.version.as_ref().filter(|v| !v.is_empty()) {
        origin.push(fact(m.get("ts-job-version"), v.clone()));
    }
    if let Some(a) = &info.adapter {
        origin.push(fact(m.get("ts-job-adapter"), a.clone()));
    }
    if let Some(g) = &info.graph {
        origin.push(Fact {
            label: m.get("ts-job-graph"),
            value: g.clone(),
            href: None,
            mono: true,
        });
    }
    if let Some(n) = info.checkpoint_size {
        origin.push(fact(m.get("ts-job-checkpoint"), n.to_string()));
    }
    JobPage {
        chip: Chip::new(ChipKind::Job, &m.get("ts-changes-chip-job")).model(),
        name: m.with("ts-changes-job", &[&info.id.to_string()]),
        status: status_text(m, &info.status),
        status_class: info.status.clone(),
        notice,
        header,
        counts_heading: m.get("ts-job-counts"),
        counts,
        rejects_heading: m.get("ts-job-rejects"),
        reject_columns: [
            "ts-job-rejects-col-line",
            "ts-job-rejects-col-match",
            "ts-job-rejects-col-reason",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rejects: info
            .rejects
            .iter()
            .map(|r| RejectRow {
                line: r.line.to_string(),
                key: r
                    .r#match
                    .as_ref()
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default(),
                reason: r.reason.clone(),
            })
            .collect(),
        origin_heading: m.get("ts-job-origin"),
        origin,
        changes: Link {
            href: format!(
                "/w/index.php?title=Special:RecentChanges&filtered=1&tag=job%3A{}&bots=1&days=30",
                info.id
            ),
            text: m.get("ts-job-changes"),
        },
        list: Link {
            href: "/wiki/Special:Jobs".into(),
            text: m.get("ts-jobs-title"),
        },
    }
}

/// `GET Special:Jobs` and `GET Special:Jobs/{id}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let subpage = title.split_once('/').map(|(_, s)| s.trim());
    if let Some(sub) = subpage.filter(|s| !s.is_empty()) {
        let Ok(id) = sub.parse::<u64>() else {
            return not_found(&cx, headers, sub);
        };
        return match site.client().job(&incoming, id).await {
            Ok(found) => {
                cx.inputs.push(found.cache);
                let model = job_model(&cx.m, &found.value);
                let body = cx.draw("special/job.html", &model);
                pages::respond(&cx, headers, StatusCode::OK, |m| Page {
                    title: m.with("ts-job-title", &[&id.to_string()]),
                    returnto: Some(format!("{TITLE}/{id}")),
                    body,
                    ..Page::default()
                })
            }
            Err(ClientError::Api { code, .. }) if code == "job-not-found" => {
                not_found(&cx, headers, sub)
            }
            Err(e) => pages::api_failed(&cx, headers, &e),
        };
    }
    let status = params
        .map
        .get("status")
        .map(String::as_str)
        .filter(|s| STATUSES.iter().any(|(k, _)| k == s));
    match site.client().jobs(&incoming, status).await {
        Ok(found) => {
            cx.inputs.push(found.cache);
            let model = list_model(&cx.m, &found.value);
            let body = cx.draw("special/jobs.html", &model);
            pages::respond(&cx, headers, StatusCode::OK, |m| Page {
                title: m.get("ts-jobs-title"),
                returnto: Some(TITLE.to_string()),
                body,
                ..Page::default()
            })
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// No such job.
fn not_found(cx: &pages::Context, headers: &HeaderMap, id: &str) -> Response {
    let body = cx.message(MessageKind::Error, &cx.m.with("ts-job-not-found", &[id]));
    pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| Page {
        title: m.get("ts-jobs-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::JobActor;

    fn job(status: &str) -> JobInfo {
        JobInfo {
            id: 12,
            status: status.into(),
            mode: Some("import".into()),
            source: Some("upload".into()),
            version: Some("pages.xml".into()),
            graph: Some("pages".into()),
            actor: JobActor {
                key: "local:1".into(),
                name: Some("Owner".into()),
            },
            started: Some("2026-11-08T10:00:00Z".into()),
            finished: (status == "finished").then(|| "2026-11-08T10:01:00Z".to_string()),
            counts: (status == "finished").then(
                || serde_json::json!({"created": 3, "merged": 1, "import": {"revisions": 9}}),
            ),
            position: (status == "queued").then_some(2),
            progress: (status == "running")
                .then(|| serde_json::json!({"stage": "pages", "pages": 39})),
            ..JobInfo::default()
        }
    }

    #[test]
    fn the_list_links_each_job_and_says_where_it_is() {
        let m = Messages::for_language("en");
        let plain = crate::skin::Skins::load(None);
        let plain = plain.get("plain").expect("plain");
        let html = plain
            .render(
                "special/jobs.html",
                &m,
                list_model(&m, &[job("queued"), job("running"), job("finished")]),
            )
            .unwrap();
        assert!(html.contains("href=\"/wiki/Special:Jobs/12\""), "{html}");
        assert!(html.contains("Queued"), "{html}");
        assert!(html.contains("2 in the queue"), "{html}");
        assert!(html.contains("39 pages"), "{html}");
        assert!(
            plain
                .render("special/jobs.html", &m, list_model(&m, &[]))
                .unwrap()
                .contains("ts-empty")
        );
    }

    #[test]
    fn the_page_shows_the_counts_and_the_origin() {
        let m = Messages::for_language("en");
        let plain = crate::skin::Skins::load(None);
        let plain = plain.get("plain").expect("plain");
        let html = plain
            .render("special/job.html", &m, job_model(&m, &job("finished")))
            .unwrap();
        assert!(html.contains("Job 12"), "{html}");
        assert!(html.contains("Finished"), "{html}");
        assert!(html.contains("Owner"), "{html}");
        assert!(html.contains("<code>pages</code>"), "{html}");
        assert!(html.contains("tag=job%3A12"), "{html}");
        let mut failed = job("failed");
        failed.error = Some("the file is not an export".into());
        let html = plain
            .render("special/job.html", &m, job_model(&m, &failed))
            .unwrap();
        assert!(html.contains("the file is not an export"), "{html}");
    }
}
