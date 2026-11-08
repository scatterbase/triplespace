//! `Special:Import` (0047 §8.3; 21 §4.3; 10 §6): the form for the general import job of
//! 0008 §9, restricted to `import`. An uploaded MediaWiki XML export (`importupload`), or
//! a page of another wiki named by site alias (0026 §2); the template census first, shown
//! as a report with a confirmation; on confirmation the job, run through `action=import`
//! on the request path, and its report. `Special:Jobs/{id}` is not built yet, so the job
//! is named, not linked. The site reaches the instance through the client only (22 §1.2
//! rule 6); the file rides along the confirmation as a hidden field when it is small
//! enough, and is asked for again otherwise, since the site keeps nothing between requests.
//! implements 0047 §8.3 (A38)
//! implements 0008 §9.7 (A50)

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use serde_json::Value;
use triplespace_client::{ClientError, ImportAsk};

use crate::Site;
use crate::codex::{Action, Button, ButtonModel, Message, MessageKind, MessageModel, Weight};
use crate::features::Feature;
use crate::frame::Page;
use crate::i18n::Messages;
use crate::moderation::GateModel;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{CheckRow, ControlModel, FieldRow};

const TITLE: &str = "Special:Import";

/// The most a file may be to ride along the confirmation as a hidden field; a larger one,
/// up to the wiki's `import.max_upload`, is asked for again on confirmation (0047 A36).
const MAX_CARRIED: usize = 8 * 1024 * 1024;

/// `Special:Import` as Plain's `special/import.html` draws it: the form, or the census
/// with its confirmation, or the report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportPage {
    /// A refusal or a notice.
    pub notices: Vec<MessageModel>,
    /// What the page does.
    pub intro: String,
    /// Where the forms post.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The upload section's legend.
    pub upload_legend: String,
    /// The file (`xml`).
    pub upload: FieldRow,
    /// The imported authors' prefix (`interwikiprefix`).
    pub prefix: FieldRow,
    /// The other-wiki section's legend.
    pub interwiki_legend: String,
    /// The site alias (`interwikisource`).
    pub interwiki: FieldRow,
    /// The page there (`interwikipage`).
    pub frompage: FieldRow,
    /// `fullhistory`, `templates`.
    pub options: Vec<CheckRow>,
    /// The reason (`summary`).
    pub summary: FieldRow,
    /// **Check the file**.
    pub submit: ButtonModel,
    /// The census, after the first post.
    pub census: Option<CensusModel>,
    /// The report, after the confirmation.
    pub done: Option<DoneModel>,
}

/// The census as shown, with the confirmation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CensusModel {
    /// The heading.
    pub title: String,
    /// What the file holds, one line each.
    pub lines: Vec<String>,
    /// Titles that already exist here.
    pub existing: Vec<String>,
    /// The line over them.
    pub existing_note: Option<String>,
    /// Templates the pages use that the file lacks.
    pub missing: Vec<String>,
    /// The line over them.
    pub missing_note: Option<String>,
    /// Functions and tags outside the registry.
    pub unregistered: Vec<String>,
    /// The line over them.
    pub unregistered_note: Option<String>,
    /// What confirming does.
    pub confirm_note: String,
    /// The hidden fields the confirmation carries.
    pub hidden: Vec<(String, String)>,
    /// The file field, where the file could not be carried.
    pub reselect: Option<FieldRow>,
    /// **Import**.
    pub submit: ButtonModel,
    /// Where **Cancel** goes.
    pub cancel: String,
    /// Its label.
    pub cancel_label: String,
}

/// The report of a finished import.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DoneModel {
    /// The heading.
    pub title: String,
    /// What the job did, one line each.
    pub lines: Vec<String>,
    /// The pages' heading.
    pub pages_title: String,
    /// Each page: its title and what it got.
    pub pages: Vec<(String, String)>,
    /// Rejects, one line each.
    pub rejects: Vec<String>,
    /// The heading over them.
    pub rejects_note: Option<String>,
}

/// What the form holds, typed or defaulted.
#[derive(Debug, Clone, Default)]
pub struct Typed {
    /// The file, with its name.
    pub xml: Option<(String, Vec<u8>)>,
    /// `interwikisource`.
    pub interwiki: String,
    /// `interwikipage`.
    pub frompage: String,
    /// `fullhistory`.
    pub history: bool,
    /// `templates`.
    pub templates: bool,
    /// `summary`.
    pub summary: String,
    /// `interwikiprefix`.
    pub prefix: String,
    /// `wpStep`: `census` or `import`.
    pub step: String,
    /// `token`.
    pub token: String,
}

impl Typed {
    /// The form as it comes up.
    fn fresh() -> Self {
        Self {
            history: true,
            step: "census".into(),
            ..Self::default()
        }
    }

    /// From a posted form: the file from the upload, or from the hidden field of a
    /// confirmation.
    fn posted(params: &Params) -> Self {
        let field = |k: &str| params.map.get(k).map_or("", String::as_str);
        let xml = params.files.get("xml").cloned().or_else(|| {
            let blob = field("wpXml");
            if blob.is_empty() {
                return None;
            }
            base64::engine::general_purpose::STANDARD
                .decode(blob)
                .ok()
                .map(|bytes| {
                    (
                        params
                            .map
                            .get("wpFilename")
                            .cloned()
                            .unwrap_or_else(|| "import.xml".into()),
                        bytes,
                    )
                })
        });
        Self {
            xml,
            interwiki: field("interwikisource").trim().to_string(),
            frompage: field("interwikipage").trim().replace('_', " "),
            history: params.map.contains_key("fullhistory"),
            templates: params.map.contains_key("templates"),
            summary: field("summary").trim().to_string(),
            prefix: field("interwikiprefix").trim().to_string(),
            step: match field("wpStep") {
                "import" => "import".into(),
                _ => "census".into(),
            },
            token: field("token").to_string(),
        }
    }

    fn has_source(&self) -> bool {
        self.xml.is_some() || (!self.interwiki.is_empty() && !self.frompage.is_empty())
    }

    fn ask(&self, census: bool) -> ImportAsk {
        ImportAsk {
            xml: self.xml.clone(),
            from: (self.xml.is_none() && !self.interwiki.is_empty())
                .then(|| (self.interwiki.clone(), self.frompage.clone())),
            history: self.history,
            templates: self.templates,
            summary: self.summary.clone(),
            prefix: self.prefix.clone(),
            census,
            token: self.token.clone(),
        }
    }
}

/// The page's gate: the feature, then the right.
fn gate(cx: &Context, headers: &HeaderMap) -> Option<Response> {
    if !cx.features.has(Feature::Import) {
        return Some(pages::no_page(cx, headers, TITLE.to_string()));
    }
    if !cx.holds("import") {
        let text = cx.m.get("ts-import-denied");
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(MessageKind::Error, &text),
        );
        let title = cx.m.get("ts-import-title");
        return Some(pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| {
            Page {
                title: title.clone(),
                returnto: Some(TITLE.to_string()),
                body,
                ..Page::default()
            }
        }));
    }
    None
}

/// The page, in one of its three states.
#[allow(clippy::too_many_lines)]
fn draw(
    cx: &Context,
    headers: &HeaderMap,
    token: &str,
    typed: &Typed,
    notice: Option<(MessageKind, String)>,
    census: Option<CensusModel>,
    done: Option<DoneModel>,
) -> Response {
    let m = &cx.m;
    let notices: Vec<MessageModel> = notice
        .into_iter()
        .map(|(k, t)| Message::block(k, &t).model())
        .collect();
    let option = |id: &str, name: &str, on: bool| CheckRow {
        class: "ts-import__option".into(),
        control: ControlModel::check(id, name, on, m.get(id)),
        help: m.get(&format!("{id}-help")),
    };
    let text = |id: &str, name: &str, value: &str| FieldRow {
        id: id.into(),
        label: m.get(id),
        class: None,
        control: ControlModel::Input {
            id: Some(id.into()),
            name: name.into(),
            input_type: "text".into(),
            inputmode: None,
            value: value.into(),
            required: false,
            described_by: Some(format!("{id}-help")),
        },
        help: Some(m.get(&format!("{id}-help"))),
        help_id: Some(format!("{id}-help")),
    };
    let model = ImportPage {
        notices,
        intro: m.get("ts-import-intro"),
        action: format!("/wiki/{TITLE}"),
        token: token.to_string(),
        upload_legend: m.get("ts-import-upload-legend"),
        upload: FieldRow {
            id: "ts-import-file".into(),
            label: m.get("ts-import-file"),
            class: None,
            control: ControlModel::File {
                id: Some("ts-import-file".into()),
                name: "xml".into(),
                accept: Some(".xml,application/xml,text/xml".into()),
                described_by: Some("ts-import-file-help".into()),
            },
            help: Some(m.with(
                "ts-import-file-help",
                &[&(cx.site.import.max_upload / (1024 * 1024)).to_string()],
            )),
            help_id: Some("ts-import-file-help".into()),
        },
        prefix: text("ts-import-prefix", "interwikiprefix", &typed.prefix),
        interwiki_legend: m.get("ts-import-interwiki-legend"),
        interwiki: text("ts-import-interwiki", "interwikisource", &typed.interwiki),
        frompage: text("ts-import-frompage", "interwikipage", &typed.frompage),
        options: vec![
            option("ts-import-fullhistory", "fullhistory", typed.history),
            option("ts-import-templates", "templates", typed.templates),
        ],
        summary: text("ts-import-summary", "summary", &typed.summary),
        submit: Button {
            label: &m.get("ts-import-preview"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        census,
        done,
    };
    let body = cx.draw("special/import.html", &model);
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-import-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

/// What the page says of a failed call.
fn refusal(m: &Messages, e: &ClientError) -> String {
    match e {
        ClientError::Api { code, info } => match code.as_str() {
            "permissiondenied" => m.get("ts-import-denied"),
            "import-nosource" => m.get("ts-import-none"),
            "import-unreadable" => m.get("ts-import-unreadable"),
            "import-too-large" => m.get("ts-import-too-large"),
            "import-nothing" => m.get("ts-import-nothing"),
            "import-unknownsource" => m.get("ts-import-unknownsource"),
            "badtoken" => m.get("ts-login-expired"),
            _ if info.is_empty() => code.clone(),
            _ => info.clone(),
        },
        other => other.to_string(),
    }
}

/// A list of names as one line.
fn join(names: &[Value]) -> String {
    names
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The census the API answered, with the confirmation that carries the file.
#[allow(clippy::too_many_lines)]
fn census_model(cx: &Context, m: &Messages, typed: &Typed, c: &Value) -> CensusModel {
    let n = |k: &str| c[k].as_u64().unwrap_or(0).to_string();
    let mut lines = vec![m.with("ts-import-census-pages", &[&n("pages"), &n("revisions")])];
    if c["current_only"].as_bool().unwrap_or(false) {
        lines.push(m.get("ts-import-census-current-only"));
    }
    lines.push(m.with(
        "ts-import-census-plan",
        &[&n("imported"), &n("talk"), &n("entities")],
    ));
    let left_out: Vec<String> = ["deferred", "ignored"]
        .iter()
        .flat_map(|k| c[*k].as_object().into_iter().flatten())
        .map(|(why, count)| format!("{why} ({})", count.as_u64().unwrap_or(0)))
        .collect();
    if !left_out.is_empty() {
        lines.push(m.with("ts-import-census-left-out", &[&left_out.join("; ")]));
    }
    let existing: Vec<String> = c["existing"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let used = c["templates_used"]
        .as_object()
        .map_or(0, serde_json::Map::len);
    let missing: Vec<String> = c["templates_missing"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if missing.is_empty() {
        lines.push(m.with("ts-import-census-templates-ok", &[&used.to_string()]));
    }
    let unregistered: Vec<String> = ["functions", "tags"]
        .iter()
        .flat_map(|k| c[*k].as_object().into_iter().flatten())
        .filter(|(_, s)| s.as_str() == Some("unregistered"))
        .map(|(name, _)| name.clone())
        .collect();
    let _ = join;

    // The confirmation: the file as a hidden field when it fits, else asked for again.
    let mut hidden: Vec<(String, String)> = vec![
        ("token".into(), typed.token.clone()),
        ("wpStep".into(), "import".into()),
        ("summary".into(), typed.summary.clone()),
        ("interwikiprefix".into(), typed.prefix.clone()),
    ];
    if typed.history {
        hidden.push(("fullhistory".into(), "1".into()));
    }
    if typed.templates {
        hidden.push(("templates".into(), "1".into()));
    }
    let mut reselect = None;
    let mut confirm_note = m.get("ts-import-census-confirm");
    match &typed.xml {
        Some((name, bytes)) if bytes.len() <= MAX_CARRIED => {
            hidden.push((
                "wpXml".into(),
                base64::engine::general_purpose::STANDARD.encode(bytes),
            ));
            hidden.push(("wpFilename".into(), name.clone()));
        }
        Some(_) => {
            confirm_note = m.get("ts-import-census-reselect");
            reselect = Some(FieldRow {
                id: "ts-import-file".into(),
                label: m.get("ts-import-file"),
                class: None,
                control: ControlModel::File {
                    id: Some("ts-import-file".into()),
                    name: "xml".into(),
                    accept: Some(".xml,application/xml,text/xml".into()),
                    described_by: None,
                },
                help: None,
                help_id: None,
            });
        }
        None => {
            hidden.push(("interwikisource".into(), typed.interwiki.clone()));
            hidden.push(("interwikipage".into(), typed.frompage.clone()));
        }
    }
    let _ = cx;
    CensusModel {
        title: m.get("ts-import-census-title"),
        lines,
        existing_note: (!existing.is_empty())
            .then(|| m.with("ts-import-census-existing", &[&existing.len().to_string()])),
        existing,
        missing_note: (!missing.is_empty()).then(|| {
            m.with(
                "ts-import-census-templates-missing",
                &[&missing.len().to_string()],
            )
        }),
        missing,
        unregistered_note: (!unregistered.is_empty())
            .then(|| m.get("ts-import-census-unregistered")),
        unregistered,
        confirm_note,
        hidden,
        reselect,
        submit: Button {
            label: &m.get("ts-import-confirm"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        cancel: format!("/wiki/{TITLE}"),
        cancel_label: m.get("ts-import-cancel"),
    }
}

/// The job's report.
fn done_model(m: &Messages, v: &Value) -> DoneModel {
    let job = &v["job"];
    let counts = &job["counts"];
    let n = |o: &Value, k: &str| o[k].as_u64().unwrap_or(0).to_string();
    let mut lines = vec![m.with(
        "ts-import-done",
        &[
            &n(job, "id"),
            &n(counts, "created"),
            &n(counts, "merged"),
            &n(counts, "revisions"),
            &n(counts, "present"),
            &n(counts, "restored"),
        ],
    )];
    let talk = &job["talk"];
    if talk["pages"].as_u64().unwrap_or(0) + talk["present"].as_u64().unwrap_or(0) > 0 {
        lines.push(m.with(
            "ts-import-done-talk",
            &[
                &n(talk, "pages"),
                &n(talk, "threads"),
                &n(talk, "posts"),
                &n(talk, "present"),
            ],
        ));
    }
    let mut left_out: Vec<String> = job["left_out"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(why, count)| format!("{why} ({})", count.as_u64().unwrap_or(0)))
        .collect();
    if let Some(e) = job["entities"].as_u64().filter(|e| *e > 0) {
        left_out.push(m.with("ts-import-done-entities", &[&e.to_string()]));
    }
    if !left_out.is_empty() {
        lines.push(m.with("ts-import-done-left-out", &[&left_out.join("; ")]));
    }
    lines.push(m.with("ts-import-done-jobs-note", &[&n(job, "id")]));
    let pages: Vec<(String, String)> = v["import"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            (
                p["title"].as_str().unwrap_or_default().to_string(),
                m.with(
                    "ts-import-done-page",
                    &[&n(p, "revisions"), &n(p, "present")],
                ),
            )
        })
        .collect();
    let rejects: Vec<String> = job["rejects"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| r["reason"].as_str().unwrap_or_default().to_string())
        .collect();
    DoneModel {
        title: m.get("ts-import-done-title"),
        lines,
        pages_title: m.get("ts-import-done-pages"),
        pages,
        rejects_note: (!rejects.is_empty()).then(|| m.get("ts-import-done-rejects")),
        rejects,
    }
}

/// `GET Special:Import`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if let Some(r) = gate(&cx, headers) {
        return r;
    }
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let r = draw(
        &cx,
        headers,
        &token.value.value,
        &Typed::fresh(),
        None,
        None,
        None,
    );
    pages::with_cookies(r, &token.set_cookies)
}

/// `POST Special:Import`: the census of the file or the other wiki's page, or, on
/// confirmation, the import.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &std::collections::BTreeMap<String, String>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if let Some(r) = gate(&cx, headers) {
        return r;
    }
    let typed = Typed::posted(params);
    let token = typed.token.clone();
    if !typed.has_source() {
        let text = cx.m.get("ts-import-none");
        return draw(
            &cx,
            headers,
            &token,
            &typed,
            Some((MessageKind::Warning, text)),
            None,
            None,
        );
    }
    let census_only = typed.step != "import";
    match site
        .client()
        .import(&incoming, &typed.ask(census_only))
        .await
    {
        Ok(v) if census_only => {
            let census = census_model(&cx, &cx.m, &typed, &v["census"]);
            draw(&cx, headers, &token, &typed, None, Some(census), None)
        }
        Ok(v) => {
            let done = done_model(&cx.m, &v);
            draw(&cx, headers, &token, &typed, None, None, Some(done))
        }
        Err(e) => {
            let text = refusal(&cx.m, &e);
            draw(
                &cx,
                headers,
                &token,
                &typed,
                Some((MessageKind::Error, text)),
                None,
                None,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_rides_along_the_confirmation() {
        let mut p = Params::new(vec![
            ("wpStep".into(), "import".into()),
            (
                "wpXml".into(),
                base64::engine::general_purpose::STANDARD.encode(b"<mediawiki/>"),
            ),
            ("wpFilename".into(), "x.xml".into()),
            ("fullhistory".into(), "1".into()),
            ("token".into(), "t".into()),
        ]);
        let t = Typed::posted(&p);
        assert_eq!(t.xml, Some(("x.xml".to_string(), b"<mediawiki/>".to_vec())));
        assert!(t.history && t.step == "import" && t.has_source());
        let ask = t.ask(false);
        assert!(ask.xml.is_some() && ask.from.is_none() && !ask.census);
        // An upload takes precedence over the hidden field; a wiki and a page are a source.
        p.files
            .insert("xml".into(), ("y.xml".into(), b"<y/>".to_vec()));
        assert_eq!(Typed::posted(&p).xml.unwrap().0, "y.xml");
        let p = Params::new(vec![
            ("interwikisource".into(), "enwiki".into()),
            ("interwikipage".into(), "Douglas_Adams".into()),
        ]);
        let t = Typed::posted(&p);
        assert!(t.has_source());
        assert_eq!(
            t.ask(true).from,
            Some(("enwiki".to_string(), "Douglas Adams".to_string()))
        );
        assert!(!Typed::posted(&Params::default()).has_source());
    }
}
