//! `Special:CreateEntities` (0104 §11; 21 §6.6; 05 §3.5): many items from one table, as
//! a spreadsheet holds them: a column per term or property, a row per new item, and
//! statements every row shares. The table is pasted, tab- or comma-separated, or uploaded
//! as a CSV or TSV file; its first row names the columns:
//!
//! - `label`, `description`, `aliases` (`|`-separated), each with an optional language
//!   after a colon (`label:de`); without one, the wiki's language;
//! - a property ID (`P31`), whose cells are parsed as the property's data type
//!   ([`crate::special::values`]); an empty cell adds nothing;
//! - anything else is ignored and said so.
//!
//! **Preview first.** The rows are checked against the properties' data types before
//! anything is written, each row's errors listed beside it, and the valid rows shown as
//! they will be created; the confirmation carries them. **Then one batch** (0104 §1): each
//! row is one `wbeditentity` through the same handler as `Special:NewItem`, under the
//! batch value the form names (one is made up when left blank), so the run is one job on
//! `Special:Jobs` with Reject and Revert, and the report lists the new ID of each row,
//! or the API's refusal. A row that fails leaves the rest to go on.
//!
//! The form needs a signed-in viewer with the right to create pages, which the API
//! enforces. At most [`MAX_ROWS`] rows a run.
//!
//! implements 0104 §11 (A3)

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use triplespace_client::{ClientError, Incoming};

use crate::Site;
use crate::codex::{Action, Button, ButtonModel, Message, MessageKind, MessageModel, Weight};
use crate::entity::{Lookup, entity_href};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::{self, Messages};
use crate::moderation::{GateModel, link_to};
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{ControlModel, FieldRow};
use crate::special::values::{datavalue, implied_type};

const TITLE: &str = "Special:CreateEntities";

/// The most rows one run creates.
pub const MAX_ROWS: usize = 500;

/// What a column holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Column {
    /// A label in a language.
    Label(String),
    /// A description in a language.
    Description(String),
    /// Aliases in a language, `|`-separated.
    Aliases(String),
    /// A property's statements.
    Property(String),
    /// A header the page does not understand.
    Ignored,
}

/// Reads a header cell.
#[must_use]
pub fn column(header: &str, default_lang: &str) -> Column {
    let h = header.trim();
    let (name, lang) = match h.split_once(':') {
        Some((n, l)) => (n.trim().to_ascii_lowercase(), l.trim()),
        None => (h.to_ascii_lowercase(), ""),
    };
    let lang = if lang.is_empty() {
        Some(default_lang.to_string())
    } else {
        i18n::valid_code(lang)
    };
    match (name.as_str(), lang) {
        ("label" | "labels", Some(l)) => Column::Label(l),
        ("description" | "descriptions", Some(l)) => Column::Description(l),
        ("alias" | "aliases", Some(l)) => Column::Aliases(l),
        _ if implied_type(h).as_deref() == Some("property") => {
            Column::Property(h.to_ascii_uppercase())
        }
        _ => Column::Ignored,
    }
}

/// Splits a table: tab-separated when the first line holds a tab, else comma-separated
/// with double-quoted cells as a spreadsheet writes them. Empty lines are skipped.
#[must_use]
pub fn rows_of(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.contains('\t') {
        return text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect())
            .collect();
    }
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            ('"', false) if cell.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut cell)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut cell));
                if row.iter().any(|c| !c.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => cell.push(c),
        }
    }
    row.push(cell);
    if row.iter().any(|c| !c.trim().is_empty()) {
        rows.push(row);
    }
    for r in &mut rows {
        for c in r.iter_mut() {
            *c = c.trim().to_string();
        }
    }
    rows
}

/// A statement every row shares, as `P31:Q5` lines give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    /// As written.
    pub raw: String,
    /// The property.
    pub property: String,
    /// The value as written.
    pub value: String,
}

/// Reads the shared statements, one per line.
#[must_use]
pub fn shared_of(text: &str) -> Vec<Result<Shared, String>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|raw| {
            raw.split_once(':')
                .map(|(p, v)| (p.trim().to_ascii_uppercase(), v.trim().to_string()))
                .filter(|(p, v)| implied_type(p).as_deref() == Some("property") && !v.is_empty())
                .map(|(property, value)| Shared {
                    raw: raw.to_string(),
                    property,
                    value,
                })
                .ok_or_else(|| raw.to_string())
        })
        .collect()
}

/// One row as it will be created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The row's line in the table, 1-based, the header being line 1.
    pub line: usize,
    /// The label shown for it: the first label column's, else the first cell.
    pub shown: String,
    /// `wbeditentity`'s `data`.
    pub data: Value,
    /// Why the row cannot be created, each a message.
    pub errors: Vec<String>,
}

/// Plans the rows: each cell parsed as its column takes it, the shared statements added,
/// and a row with nothing to say or a bad cell marked.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn plan(
    m: &Messages,
    columns: &[Column],
    rows: &[Vec<String>],
    shared: &[Shared],
    lookup: &Lookup,
) -> Vec<Plan> {
    let statement = |property: &str, value: &str, raw: &str| -> Result<Value, String> {
        let dt = lookup
            .datatype(property)
            .ok_or_else(|| m.with("ts-new-statement-no-property", &[property, raw]))?;
        datavalue(m, dt, value, raw).map(|dv| {
            json!({
                "mainsnak": {"snaktype": "value", "property": property, "datavalue": dv},
                "type": "statement",
                "rank": "normal",
            })
        })
    };
    rows.iter()
        .enumerate()
        .map(|(i, cells)| {
            let line = i + 2;
            let mut labels = Map::new();
            let mut descriptions = Map::new();
            let mut aliases = Map::new();
            let mut claims: Map<String, Value> = Map::new();
            let mut errors = Vec::new();
            let mut add_claim = |property: &str, s: Value| {
                if let Some(a) = claims
                    .entry(property.to_string())
                    .or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()
                {
                    a.push(s);
                }
            };
            for (col, cell) in columns.iter().zip(cells.iter()) {
                if cell.is_empty() {
                    continue;
                }
                match col {
                    Column::Label(l) => {
                        labels.insert(l.clone(), json!({"language": l, "value": cell}));
                    }
                    Column::Description(l) => {
                        descriptions.insert(l.clone(), json!({"language": l, "value": cell}));
                    }
                    Column::Aliases(l) => {
                        let list: Vec<Value> = cell
                            .split('|')
                            .map(str::trim)
                            .filter(|a| !a.is_empty())
                            .map(|a| json!({"language": l, "value": a}))
                            .collect();
                        if !list.is_empty() {
                            aliases.insert(l.clone(), Value::Array(list));
                        }
                    }
                    Column::Property(p) => {
                        let raw = format!("{p}:{cell}");
                        match statement(p, cell, &raw) {
                            Ok(s) => add_claim(p, s),
                            Err(e) => errors.push(e),
                        }
                    }
                    Column::Ignored => {}
                }
            }
            for s in shared {
                match statement(&s.property, &s.value, &s.raw) {
                    Ok(v) => add_claim(&s.property, v),
                    Err(e) => errors.push(e),
                }
            }
            let shown = columns
                .iter()
                .zip(cells.iter())
                .find(|(c, cell)| matches!(c, Column::Label(_)) && !cell.is_empty())
                .map(|(_, cell)| cell.clone())
                .or_else(|| cells.iter().find(|c| !c.is_empty()).cloned())
                .unwrap_or_default();
            if labels.is_empty() && descriptions.is_empty() && aliases.is_empty() {
                errors.push(m.get("ts-new-insufficient"));
            }
            let mut data = Map::new();
            if !labels.is_empty() {
                data.insert("labels".into(), Value::Object(labels));
            }
            if !descriptions.is_empty() {
                data.insert("descriptions".into(), Value::Object(descriptions));
            }
            if !aliases.is_empty() {
                data.insert("aliases".into(), Value::Object(aliases));
            }
            if !claims.is_empty() {
                data.insert("claims".into(), Value::Object(claims));
            }
            Plan {
                line,
                shown,
                data: Value::Object(data),
                errors,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `special/createentities.html` draws.
// ---------------------------------------------------------------------------------------

/// The page: the form, the preview with its confirmation, or the report.
#[derive(Debug, Clone, Serialize)]
pub struct CreateEntitiesPage {
    /// A reader is asked to log in.
    pub gate: Option<GateModel>,
    /// What the page says above the form.
    pub intro: String,
    /// Notices above the form.
    pub notices: Vec<MessageModel>,
    /// Where the form posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The table, pasted.
    pub rows: FieldRow,
    /// The table, uploaded.
    pub file: FieldRow,
    /// The shared statements.
    pub shared: FieldRow,
    /// The batch value.
    pub batch: FieldRow,
    /// **Preview**.
    pub submit: ButtonModel,
    /// The preview, after the first post.
    pub preview: Option<PreviewModel>,
    /// The report, after the confirmation.
    pub report: Option<ReportModel>,
}

/// The preview: each row as it will be created, and the confirmation.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewModel {
    /// The heading.
    pub title: String,
    /// What the columns were read as, one line each.
    pub columns: Vec<String>,
    /// How many rows will be created, and how many cannot be.
    pub summary: String,
    /// The column headings of the preview table.
    pub headings: Vec<String>,
    /// The rows.
    pub rows: Vec<PreviewRow>,
    /// What confirming does.
    pub confirm_note: String,
    /// The hidden fields the confirmation carries.
    pub hidden: Vec<(String, String)>,
    /// **Create**.
    pub submit: Option<ButtonModel>,
    /// Back to the form.
    pub cancel: String,
    /// Its label.
    pub cancel_label: String,
}

/// One row of the preview.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewRow {
    /// The line.
    pub line: String,
    /// The label, or the first cell.
    pub shown: String,
    /// What the row holds, in words.
    pub summary: String,
    /// Its errors.
    pub errors: Vec<String>,
}

/// The report: what each row became.
#[derive(Debug, Clone, Serialize)]
pub struct ReportModel {
    /// The heading.
    pub title: String,
    /// How it went, one line.
    pub summary: String,
    /// The batch's job page.
    pub job: Option<(String, String)>,
    /// The column headings.
    pub headings: Vec<String>,
    /// The rows.
    pub rows: Vec<ReportRow>,
    /// Back to the form.
    pub again: (String, String),
}

/// One row of the report.
#[derive(Debug, Clone, Serialize)]
pub struct ReportRow {
    /// The line.
    pub line: String,
    /// The label.
    pub shown: String,
    /// The new entity's page, and its ID.
    pub created: Option<(String, String)>,
    /// Why it was not created.
    pub error: Option<String>,
}

/// What the form holds, as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Typed {
    /// The pasted table.
    pub rows: String,
    /// The shared statements.
    pub shared: String,
    /// The batch value.
    pub batch: String,
    /// The CSRF token.
    pub token: String,
    /// `wpStep`: `preview` or `create`.
    pub step: String,
    /// The plans the confirmation carries.
    pub plans: Option<Vec<Plan>>,
}

impl Typed {
    fn of(params: &Params) -> Self {
        let get = |k: &str| params.map.get(k).cloned().unwrap_or_default();
        let mut rows = get("rows");
        if rows.trim().is_empty()
            && let Some((_, bytes)) = params.files.get("file")
        {
            rows = String::from_utf8_lossy(bytes).into_owned();
        }
        let plans = params
            .map
            .get("wpPlans")
            .and_then(|p| base64::engine::general_purpose::STANDARD.decode(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok());
        Self {
            rows,
            shared: get("shared"),
            batch: get("batch").trim().to_string(),
            token: get("token"),
            step: get("wpStep"),
            plans,
        }
    }
}

/// The form's fields.
fn form_model(cx: &Context, typed: &Typed) -> CreateEntitiesPage {
    let m = &cx.m;
    let area = |id: &str, name: &str, value: &str, rows: usize| FieldRow {
        id: id.into(),
        label: m.get(id),
        class: None,
        control: ControlModel::TextArea {
            id: Some(id.into()),
            name: name.into(),
            value: value.into(),
            rows,
            no_spellcheck: true,
            described_by: Some(format!("{id}-help")),
        },
        help: Some(m.get(&format!("{id}-help"))),
        help_id: Some(format!("{id}-help")),
    };
    CreateEntitiesPage {
        gate: None,
        intro: m.get("ts-createentities-intro"),
        notices: Vec::new(),
        action: format!("/wiki/{TITLE}"),
        token: typed.token.clone(),
        rows: area("ts-createentities-rows", "rows", &typed.rows, 8),
        file: FieldRow {
            id: "ts-createentities-file".into(),
            label: m.get("ts-createentities-file"),
            class: None,
            control: ControlModel::File {
                id: Some("ts-createentities-file".into()),
                name: "file".into(),
                accept: Some(".csv,.tsv,.txt,text/csv,text/tab-separated-values".into()),
                described_by: Some("ts-createentities-file-help".into()),
            },
            help: Some(m.get("ts-createentities-file-help")),
            help_id: Some("ts-createentities-file-help".into()),
        },
        shared: area("ts-createentities-shared", "shared", &typed.shared, 3),
        batch: FieldRow {
            id: "ts-createentities-batch".into(),
            label: m.get("ts-createentities-batch"),
            class: None,
            control: ControlModel::Input {
                id: Some("ts-createentities-batch".into()),
                name: "batch".into(),
                input_type: "text".into(),
                inputmode: None,
                value: typed.batch.clone(),
                required: false,
                described_by: Some("ts-createentities-batch-help".into()),
            },
            help: Some(m.get("ts-createentities-batch-help")),
            help_id: Some("ts-createentities-batch-help".into()),
        },
        submit: Button {
            label: &m.get("ts-createentities-preview"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        preview: None,
        report: None,
    }
}

/// A row's contents in words: the languages of its terms and its statements' properties.
fn row_summary(m: &Messages, data: &Value) -> String {
    let mut parts = Vec::new();
    let count = |key: &str| data.get(key).and_then(Value::as_object).map_or(0, Map::len);
    for (key, msg) in [
        ("labels", "ts-createentities-row-labels"),
        ("descriptions", "ts-createentities-row-descriptions"),
        ("aliases", "ts-createentities-row-aliases"),
    ] {
        let n = count(key);
        if n > 0 {
            parts.push(m.with(msg, &[&n.to_string()]));
        }
    }
    let statements: usize = data
        .get("claims")
        .and_then(Value::as_object)
        .map_or(0, |c| {
            c.values().filter_map(Value::as_array).map(Vec::len).sum()
        });
    if statements > 0 {
        parts.push(m.with(
            "ts-createentities-row-statements",
            &[&statements.to_string()],
        ));
    }
    parts.join(", ")
}

/// The batch value a run gets when the form names none.
fn made_up_batch() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{TITLE} {now}")
}

/// The context, or the response that ends the request: creating not offered (`404`), or
/// a viewer not signed in (asked to).
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &std::collections::BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Create) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    if cx.user.is_none() {
        let login_href = format!(
            "/w/index.php?title=Special:UserLogin&returnto={}",
            title_url(TITLE)
        );
        let mut model = form_model(&cx, &Typed::default());
        model.gate = Some(GateModel {
            notice: Message::block(MessageKind::Notice, &cx.m.get("ts-new-login")).model(),
            login: cx
                .features
                .has(Feature::Login)
                .then(|| link_to(login_href, cx.m.get("ts-login-title"))),
        });
        return Err(page(&cx, headers, StatusCode::OK, &model));
    }
    Ok(cx)
}

fn page(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    model: &CreateEntitiesPage,
) -> Response {
    let body = cx.draw("special/createentities.html", model);
    pages::respond(cx, headers, status, |m| Page {
        title: m.get("ts-createentities-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

/// `GET Special:CreateEntities`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache.clone());
    let typed = Typed {
        token: token.value.value,
        ..Typed::default()
    };
    let model = form_model(&cx, &typed);
    page(&cx, headers, StatusCode::OK, &model)
}

/// `POST Special:CreateEntities`: the preview, or the creation.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &std::collections::BTreeMap<String, String>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let typed = Typed::of(params);
    if typed.step == "create" {
        return create(site, &mut cx, headers, &incoming, typed).await;
    }
    preview(site, &mut cx, headers, &incoming, typed).await
}

/// The preview: the table read, the properties looked up, each row planned.
#[allow(clippy::too_many_lines)]
async fn preview(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    typed: Typed,
) -> Response {
    let rows = rows_of(&typed.rows);
    let mut model = form_model(cx, &typed);
    let m = &cx.m;
    let Some((header, body)) = rows.split_first() else {
        model
            .notices
            .push(Message::block(MessageKind::Error, &m.get("ts-createentities-empty")).model());
        return page(cx, headers, StatusCode::OK, &model);
    };
    if body.is_empty() {
        model
            .notices
            .push(Message::block(MessageKind::Error, &m.get("ts-createentities-empty")).model());
        return page(cx, headers, StatusCode::OK, &model);
    }
    if body.len() > MAX_ROWS {
        model.notices.push(
            Message::block(
                MessageKind::Error,
                &m.with(
                    "ts-createentities-too-many",
                    &[&body.len().to_string(), &MAX_ROWS.to_string()],
                ),
            )
            .model(),
        );
        return page(cx, headers, StatusCode::OK, &model);
    }
    let lang = cx.site.lang.clone();
    let columns: Vec<Column> = header.iter().map(|h| column(h, &lang)).collect();
    let shared_lines = shared_of(&typed.shared);
    let mut shared = Vec::new();
    let mut column_notes = Vec::new();
    for s in shared_lines {
        match s {
            Ok(s) => shared.push(s),
            Err(raw) => model.notices.push(
                Message::block(
                    MessageKind::Error,
                    &m.with("ts-new-statement-malformed", &[&raw]),
                )
                .model(),
            ),
        }
    }
    // The properties named, with their labels and data types, in one call.
    let mut ids: Vec<String> = columns
        .iter()
        .filter_map(|c| match c {
            Column::Property(p) => Some(p.clone()),
            _ => None,
        })
        .chain(shared.iter().map(|s| s.property.clone()))
        .collect();
    ids.sort();
    ids.dedup();
    let lookup = if ids.is_empty() {
        Lookup::default()
    } else {
        let languages = cx.m.term_chain().join("|");
        match site
            .client()
            .entities(incoming, &ids, "labels|datatype", &languages)
            .await
        {
            Ok(f) => {
                cx.inputs.push(f.cache);
                Lookup::from_entities(&f.value.found, &cx.m.term_chain())
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
    };
    let m = &cx.m;
    for (h, c) in header.iter().zip(&columns) {
        column_notes.push(match c {
            Column::Label(l) => m.with("ts-createentities-col-label", &[h, l]),
            Column::Description(l) => m.with("ts-createentities-col-description", &[h, l]),
            Column::Aliases(l) => m.with("ts-createentities-col-aliases", &[h, l]),
            Column::Property(p) => match lookup.label(p) {
                Some((label, _)) => m.with("ts-createentities-col-property", &[h, label, p]),
                None => m.with("ts-createentities-col-unknown", &[h]),
            },
            Column::Ignored => m.with("ts-createentities-col-ignored", &[h]),
        });
    }
    let plans = plan(m, &columns, body, &shared, &lookup);
    let valid: Vec<&Plan> = plans.iter().filter(|p| p.errors.is_empty()).collect();
    let batch = if typed.batch.is_empty() {
        made_up_batch()
    } else {
        typed.batch.clone()
    };
    let carried: Vec<Plan> = valid.iter().map(|p| (*p).clone()).collect();
    let hidden = vec![
        ("token".to_string(), typed.token.clone()),
        ("wpStep".to_string(), "create".to_string()),
        ("batch".to_string(), batch.clone()),
        (
            "wpPlans".to_string(),
            base64::engine::general_purpose::STANDARD
                .encode(serde_json::to_vec(&carried).unwrap_or_default()),
        ),
    ];
    model.batch.control = ControlModel::Input {
        id: Some("ts-createentities-batch".into()),
        name: "batch".into(),
        input_type: "text".into(),
        inputmode: None,
        value: batch.clone(),
        required: false,
        described_by: Some("ts-createentities-batch-help".into()),
    };
    model.preview = Some(PreviewModel {
        title: m.get("ts-createentities-preview-title"),
        columns: column_notes,
        summary: m.with(
            "ts-createentities-preview-summary",
            &[
                &valid.len().to_string(),
                &(plans.len() - valid.len()).to_string(),
                &batch,
            ],
        ),
        headings: [
            "ts-createentities-col-line",
            "ts-createentities-col-item",
            "ts-createentities-col-holds",
            "ts-createentities-col-problems",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: plans
            .iter()
            .map(|p| PreviewRow {
                line: p.line.to_string(),
                shown: p.shown.clone(),
                summary: row_summary(m, &p.data),
                errors: p.errors.clone(),
            })
            .collect(),
        confirm_note: m.get("ts-createentities-confirm-note"),
        hidden,
        submit: (!valid.is_empty()).then(|| {
            Button {
                label: &m.with("ts-createentities-confirm", &[&valid.len().to_string()]),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .model()
        }),
        cancel: format!("/wiki/{TITLE}"),
        cancel_label: m.get("ts-import-cancel"),
    });
    page(cx, headers, StatusCode::OK, &model)
}

/// The creation: one `wbeditentity` per planned row, under the batch.
async fn create(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    typed: Typed,
) -> Response {
    let Some(plans) = typed.plans.clone().filter(|p| !p.is_empty()) else {
        let mut model = form_model(cx, &typed);
        model
            .notices
            .push(Message::block(MessageKind::Error, &cx.m.get("ts-createentities-empty")).model());
        return page(cx, headers, StatusCode::OK, &model);
    };
    let batch = if typed.batch.is_empty() {
        made_up_batch()
    } else {
        typed.batch.clone()
    };
    let mut rows = Vec::new();
    let mut job = None;
    let mut created_n = 0usize;
    for p in &plans {
        match site
            .client()
            .create_entity_in(incoming, "item", &p.data, &typed.token, Some(&batch))
            .await
        {
            Ok((c, b)) => {
                job = job.or(b);
                created_n += 1;
                rows.push(ReportRow {
                    line: p.line.to_string(),
                    shown: p.shown.clone(),
                    created: Some((entity_href(&c.id, &c.entity_type), c.id)),
                    error: None,
                });
            }
            Err(ClientError::Api { code, info }) => {
                let text = match code.as_str() {
                    "badtoken" => cx.m.get("ts-new-expired"),
                    "permissiondenied" => cx.m.get("ts-permission-denied"),
                    "ts-batch-rejected" => cx.m.get("ts-createentities-rejected"),
                    _ if info.is_empty() => code.clone(),
                    _ => info,
                };
                let stop = matches!(code.as_str(), "badtoken" | "ts-batch-rejected");
                rows.push(ReportRow {
                    line: p.line.to_string(),
                    shown: p.shown.clone(),
                    created: None,
                    error: Some(text),
                });
                if stop {
                    break;
                }
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
    }
    let m = &cx.m;
    let mut model = form_model(cx, &Typed::default());
    model.report = Some(ReportModel {
        title: m.get("ts-createentities-report-title"),
        summary: m.with(
            "ts-createentities-report-summary",
            &[
                &created_n.to_string(),
                &(plans.len() - created_n).to_string(),
            ],
        ),
        job: job.map(|j| {
            (
                crate::special::jobs::href(j),
                m.with("ts-createentities-report-job", &[&j.to_string()]),
            )
        }),
        headings: [
            "ts-createentities-col-line",
            "ts-createentities-col-item",
            "ts-createentities-col-created",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows,
        again: (format!("/wiki/{TITLE}"), m.get("ts-createentities-again")),
    });
    page(cx, headers, StatusCode::OK, &model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_split_as_tabs_or_commas() {
        let tsv = "label\tP31\nSix\tQ5\n\nSeven\tQ6\n";
        assert_eq!(
            rows_of(tsv),
            vec![vec!["label", "P31"], vec!["Six", "Q5"], vec!["Seven", "Q6"]]
        );
        let csv = "\u{feff}label,description:de\r\n\"Six, the number\",\"eine \"\"Zahl\"\"\"\r\nSeven,\r\n";
        assert_eq!(
            rows_of(csv),
            vec![
                vec!["label", "description:de"],
                vec!["Six, the number", "eine \"Zahl\""],
                vec!["Seven", ""]
            ]
        );
    }

    #[test]
    fn headers_name_terms_and_properties() {
        assert_eq!(column("label", "en"), Column::Label("en".into()));
        assert_eq!(
            column("Description:de", "en"),
            Column::Description("de".into())
        );
        assert_eq!(column("aliases:fr", "en"), Column::Aliases("fr".into()));
        assert_eq!(column("p31", "en"), Column::Property("P31".into()));
        assert_eq!(column("label:not a language", "en"), Column::Ignored);
        assert_eq!(column("notes", "en"), Column::Ignored);
    }

    #[test]
    fn rows_are_planned_with_their_errors() {
        let m = Messages::for_language("en");
        let mut found = std::collections::BTreeMap::new();
        found.insert(
            "P31".to_string(),
            json!({"id": "P31", "datatype": "wikibase-item", "labels": {"en": {"language": "en", "value": "instance of"}}}),
        );
        found.insert(
            "P12".to_string(),
            json!({"id": "P12", "datatype": "quantity", "labels": {}}),
        );
        let lookup = Lookup::from_entities(&found, &["en".to_string()]);
        let columns = vec![
            Column::Label("en".into()),
            Column::Property("P31".into()),
            Column::Property("P12".into()),
            Column::Ignored,
        ];
        let rows = vec![
            vec!["Six".into(), "Q5".into(), "6".into(), "x".into()],
            vec![String::new(), "Q5".into(), String::new(), String::new()],
            vec![
                "Eight".into(),
                "nonsense".into(),
                "eight".into(),
                String::new(),
            ],
        ];
        let shared = vec![Shared {
            raw: "P31:Q1".into(),
            property: "P31".into(),
            value: "Q1".into(),
        }];
        let plans = plan(&m, &columns, &rows, &shared, &lookup);
        assert_eq!(plans.len(), 3);
        assert!(plans[0].errors.is_empty(), "{:?}", plans[0].errors);
        assert_eq!(plans[0].line, 2);
        assert_eq!(plans[0].shown, "Six");
        assert_eq!(plans[0].data["labels"]["en"]["value"], "Six");
        assert_eq!(plans[0].data["claims"]["P31"].as_array().unwrap().len(), 2);
        assert_eq!(
            plans[0].data["claims"]["P12"][0]["mainsnak"]["datavalue"]["value"]["amount"],
            "+6"
        );
        assert_eq!(plans[1].errors.len(), 1, "no terms: {:?}", plans[1].errors);
        assert_eq!(plans[2].errors.len(), 2, "{:?}", plans[2].errors);
        assert_eq!(row_summary(&m, &plans[0].data), "1 label, 3 statements");
    }
}
