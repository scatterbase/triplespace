//! `Special:CreateEntitiesFromFile` (0106 §7, A9; 21 §6.8): a file read as the table of
//! `Special:CreateManyEntities`, or as the commands of `Special:QuickStatements`, by what
//! it holds. The page writes nothing: its answer is one of those pages' forms.
//!
//! **Without JavaScript** the file is uploaded here. QuickStatements commands go to
//! [`super::quickstatements`]; anything else is read as CSV or TSV text whose first row
//! names the columns in the table's own conventions, with no mapping step. **With it**
//! the file is read in the browser and never uploaded (0106 A6): its columns are mapped
//! (`ui/src/grid`), and the result is posted to `Special:CreateManyEntities` as text. A
//! QuickStatements file is sent here as any form's file is.
//!
//! `Special:CreateEntitiesFromFile/{table title}` fills a draft: a draft holding columns
//! and no rows is a saved mapping, which a file of its shape goes under without the step.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use super::table::rows_of;
use super::{HandoffPage, Typed, draft, error, handoff, open, quickstatements, viewer};
use crate::Site;
use crate::codex::{Action, Button, Weight};
use crate::frame::title_url;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{ControlModel, FieldRow};
use crate::special::new_entity::offered;

/// The page's title.
pub const TITLE: &str = "Special:CreateEntitiesFromFile";

/// The table title after the page's name, where the page was asked for with one.
fn draft_of(title: &str) -> String {
    title
        .split_once('/')
        .map(|(_, t)| t.trim().to_string())
        .unwrap_or_default()
}

/// The page: one file, for the draft `draft` where one is named. `columns` are that
/// draft's headers when it holds columns and no rows (a saved mapping).
fn model(cx: &Context, draft: &str, columns: Option<&[String]>, can_new: bool) -> HandoffPage {
    let m = &cx.m;
    let data = serde_json::json!({
        "page": "file",
        "lang": cx.site.lang,
        "uiLang": m.lang(),
        "dir": m.dir(),
        "canCreateProperty": can_new,
        "datatypes": offered(&cx.site),
        "conceptBase": cx.site.concept_base,
        "open": format!("/wiki/{}", super::TITLE),
        "draft": draft,
        "columns": columns,
        "messages": m.with_prefix("ts-sheet-"),
    });
    HandoffPage {
        gate: None,
        class: "ts-createfromfile".into(),
        intro: m.get("ts-createfromfile-intro"),
        notices: Vec::new(),
        action: if draft.is_empty() {
            format!("/wiki/{TITLE}")
        } else {
            format!("/wiki/{TITLE}/{}", title_url(draft))
        },
        multipart: true,
        hidden: Vec::new(),
        fields: vec![FieldRow {
            id: "ts-createfromfile-file".into(),
            label: m.get("ts-createfromfile-file"),
            class: None,
            control: ControlModel::File {
                id: Some("ts-createfromfile-file".into()),
                name: "file".into(),
                accept: Some(
                    ".csv,.tsv,.txt,.qs,.xlsx,.jsonl,.ndjson,text/csv,text/tab-separated-values"
                        .into(),
                ),
                described_by: Some("ts-createfromfile-file-help".into()),
            },
            help: Some(m.get("ts-createfromfile-file-help")),
            help_id: Some("ts-createfromfile-file-help".into()),
        }],
        data_html: format!(
            "<script type=\"application/json\" id=\"ts-sheet-data\">{}</script>",
            data.to_string().replace("</", "<\\/")
        ),
        submit: Button {
            label: &m.get("ts-createfromfile-read"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        links: vec![
            (
                if draft.is_empty() {
                    format!("/wiki/{}", super::TITLE)
                } else {
                    format!("/wiki/{}/{}", super::TITLE, title_url(draft))
                },
                m.get("ts-createentities-title"),
            ),
            (
                format!("/wiki/{}", quickstatements::TITLE),
                m.get("ts-quickstatements-title"),
            ),
        ],
    }
}

/// `GET Special:CreateEntitiesFromFile`, or `…/{table title}` for a draft.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let (cx, gate) = match viewer(site, headers, &incoming, &params.map, TITLE).await {
        Ok(found) => found,
        Err(r) => return r,
    };
    let mut named = draft_of(title);
    let mut notices = Vec::new();
    let mut columns: Option<Vec<String>> = None;
    if gate.is_none() && !named.is_empty() {
        match super::stored(site, &cx, &incoming, &named).await {
            Err(e) => return pages::api_failed(&cx, headers, &e),
            Ok(Err(why)) => {
                notices.push(error(&why));
                named.clear();
            }
            Ok(Ok(found)) => {
                named = found.title;
                // A saved mapping: columns, and no rows left to write.
                if let Ok(text) = draft::text(&found.definition, &cx.site.lang)
                    && text.rows.len() == 1
                {
                    columns = text.rows.into_iter().next();
                }
            }
        }
    }
    let can_new = gate.is_none() && super::may_create_properties(site, &incoming).await;
    let mut page = model(&cx, &named, columns.as_deref(), can_new);
    page.gate = gate;
    page.notices = notices;
    handoff(
        &cx,
        headers,
        StatusCode::OK,
        TITLE,
        "ts-createfromfile-title",
        &page,
    )
}

/// `POST Special:CreateEntitiesFromFile`: the uploaded file, read as commands or as the
/// table's text, and the form that takes it from there.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let (mut cx, gate) = match viewer(site, headers, &incoming, query, TITLE).await {
        Ok(found) => found,
        Err(r) => return r,
    };
    let named = draft_of(title);
    let again = |cx: &Context, gate, problem: Option<String>| {
        let mut page = model(cx, &named, None, false);
        page.gate = gate;
        page.notices = problem.iter().map(|p| error(p)).collect();
        handoff(
            cx,
            headers,
            StatusCode::OK,
            TITLE,
            "ts-createfromfile-title",
            &page,
        )
    };
    if gate.is_some() {
        return again(&cx, gate, None);
    }
    let Some((name, bytes)) = params.files.get("file").filter(|(_, b)| !b.is_empty()) else {
        let problem = cx.m.get("ts-createfromfile-none");
        return again(&cx, None, Some(problem));
    };
    let extension = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    // A ZIP's signature, whatever the file is called: a spreadsheet is read in the browser.
    if bytes.starts_with(b"PK") || matches!(extension.as_str(), "xlsx" | "jsonl" | "ndjson") {
        let problem = cx.m.with("ts-createfromfile-needs-script", &[name]);
        return again(&cx, None, Some(problem));
    }
    let text = String::from_utf8_lossy(bytes).into_owned();
    if extension == "qs" || quickstatements::looks_like(&text) {
        return quickstatements::answer(site, cx, headers, &incoming, &text).await;
    }
    if rows_of(&text).len() < 2 {
        let problem = cx.m.with("ts-createfromfile-empty", &[name]);
        return again(&cx, None, Some(problem));
    }
    let typed = Typed {
        rows: text,
        draft: named.clone(),
        ..Typed::default()
    };
    open(site, &mut cx, headers, &incoming, typed, Vec::new()).await
}
