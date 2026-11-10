//! `Special:CreateManyEntities` (0104 §11; 0106; 21 §6.6; 05 §3.5): many items from one table,
//! as a spreadsheet holds them: a column per term or property, a row per item, and
//! statements every row shares, with the reference every statement carries. The table is
//! pasted, tab- or comma-separated, or uploaded as a CSV or TSV file. **Its text is the
//! canonical form** (0106 §1; [`table`]): the first row names the columns, and the preview
//! writes back into it what it found, so the table can be corrected and checked again, or
//! passed on, as text.
//!
//! - `label`, `description`, `aliases`, each with an optional language (`label:de`);
//! - a property ID (`P31`), its cells parsed as the property's data type
//!   ([`crate::special::values`]), several values `|`-separated, `row:n` for the item
//!   another row creates or matches;
//! - `id`, the existing item a row adds to (0106 §5): given, or filled in by the preview
//!   for a row whose label and description are an existing item's;
//! - `new:{label}@{datatype}`, a property the run creates first (0106 §4), for holders
//!   of `property-create`;
//! - `{property}/ref:{property}`, a statement's own reference, which replaces the
//!   reference on every statement (0106 §8);
//! - anything else is a plain column, which the reference on every statement may read.
//!
//! **Preview first.** The rows are planned ([`plan`]) before anything is written: cells
//! checked against the data types, matches looked for, each row given a status and its
//! problems. **Then one batch** (0104 §1): the declared properties, then the rows in an
//! order in which every `row:n` names a row already written, each one `wbeditentity`
//! through the same handler as `Special:NewItem`, under the batch value the form names
//! (one is made up when left blank). The run is one job on `Special:Jobs` with Reject and
//! Revert, and the report lists each row's item, or the API's refusal. A row that fails
//! leaves the rest to go on.
//!
//! The form needs a signed-in viewer with the right to create pages, which the API
//! enforces. At most [`MAX_ROWS`] rows a run.
//!
//! **Drafts** (0106 §9; [`draft`]). The table can be saved as a draft, a Table page under
//! the viewer's name whose definition holds the rows not yet written, and opened again
//! as `Special:CreateManyEntities/{table title}`. Importing from a draft saves it first, then
//! records the run in it: each written row becomes a listed row, a refused row stays
//! staged with its reason.
//!
//! **With JavaScript** the table is edited as a grid over the same text, and a chosen file
//! is mapped onto it first (0106 §1, §6, §7; `ui/src/grid`). The page gives the grid a
//! data block to start from, and answers the check as JSON (`wpFormat=json`) so that the
//! grid can show each row's status; everything else is this form.
//!
//! Not here yet: matching by a match-key identifier (0106 §5), which waits on the API
//! serving the lookup; moving a draft to a title of its own (0106 A2); runs past
//! [`MAX_ROWS`].
//!
//! implements 0106 §2 (A4)
//! implements 0106 §8 (A4)

pub mod draft;
pub mod plan;
pub mod table;

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use triplespace_client::{ClientError, Incoming};

use scatter_wikibase_shape::table::{Definition, Limits};

use self::plan::{Declared, Known, Plan, Read, Status, Target};
use self::table::{Column, lines_of, rows_of};
use crate::Site;
use crate::codex::{Action, Button, ButtonModel, Message, MessageKind, MessageModel, Weight};
use crate::entity::{Lookup, entity_href};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::{GateModel, link_to};
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{ControlModel, FieldRow};
use crate::special::new_entity::{datatype_name, offered};

const TITLE: &str = "Special:CreateManyEntities";

/// The most rows one run creates.
pub const MAX_ROWS: usize = 500;

/// The most candidates a label's search is read for when looking for a match.
const MATCH_CANDIDATES: u32 = 50;

/// What the confirmation carries: the rows to write, in order, and the properties the
/// table declares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Carried {
    /// The writable rows, each after the rows it names.
    pub plans: Vec<Plan>,
    /// The declared properties, by position.
    pub new: Vec<Declared>,
    /// The language of their labels.
    pub lang: String,
    /// The draft the table is kept in (0106 §9), where it is one.
    #[serde(default)]
    pub draft: Option<String>,
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
    /// The statements on every item.
    pub shared: FieldRow,
    /// The reference on every statement.
    pub reference: FieldRow,
    /// The batch value.
    pub batch: FieldRow,
    /// The name a new draft gets.
    pub draft_name: Option<FieldRow>,
    /// The draft the table is kept in: its page, its title, and the field that carries it.
    pub draft: Option<DraftModel>,
    /// What the grid starts from (0034 §4; 21 §6.7): a JSON block, HTML.
    pub sheet_data_html: String,
    /// **Check the rows**.
    pub submit: ButtonModel,
    /// **Save as a draft**, or **Save the draft**.
    pub save: ButtonModel,
    /// The preview, after the first post.
    pub preview: Option<PreviewModel>,
    /// The report, after the confirmation.
    pub report: Option<ReportModel>,
}

/// The draft a table is kept in (0106 §9).
#[derive(Debug, Clone, Serialize)]
pub struct DraftModel {
    /// What the page says of it, before the link.
    pub text: String,
    /// The Table page.
    pub href: String,
    /// Its title.
    pub title: String,
}

/// The preview: each row as it will be written, and the confirmation.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewModel {
    /// The heading.
    pub title: String,
    /// What the columns were read as, one line each.
    pub columns: Vec<String>,
    /// Each column as the grid needs it.
    pub kinds: Vec<ColumnInfo>,
    /// The labels of the items the cells name, by ID, for the grid's chips.
    pub labels: BTreeMap<String, String>,
    /// How many rows will be created, added to and left out.
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

/// A column as the grid needs it (21 §6.7): what it holds and, for a property, its label
/// and data type, which decide how its cells are edited.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    /// `id`, `label`, `description`, `aliases`, `property`, `new`, `reference`, `plain`.
    pub kind: &'static str,
    /// A property's label.
    pub label: Option<String>,
    /// A property's data type, or a new property's.
    pub datatype: Option<String>,
}

/// One row of the preview.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewRow {
    /// The row's number.
    pub line: String,
    /// The label, or the first cell.
    pub shown: String,
    /// Its status, in words.
    pub status: String,
    /// The status as a class suffix: `new`, `matches`, `ambiguous`, `conflict`,
    /// `invalid`, `skipped`.
    pub kind: &'static str,
    /// The item it adds to.
    pub item: Option<(String, String)>,
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
    /// The properties the run created.
    pub properties: Vec<String>,
    /// The table the run's rows were kept in, and what is left staged in it.
    pub draft: Option<DraftModel>,
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
    /// The row's number.
    pub line: String,
    /// The label.
    pub shown: String,
    /// The item's page, and its ID.
    pub created: Option<(String, String)>,
    /// What was done to it, where that is not creation.
    pub note: Option<String>,
    /// Why it was not written.
    pub error: Option<String>,
}

/// What the form holds, as typed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Typed {
    /// The pasted table.
    pub rows: String,
    /// The statements on every item.
    pub shared: String,
    /// The reference on every statement.
    pub reference: String,
    /// The batch value.
    pub batch: String,
    /// The CSRF token.
    pub token: String,
    /// `wpStep`: `preview` or `create`.
    pub step: String,
    /// Whether **Save as a draft** was pressed.
    pub save: bool,
    /// Whether the check is answered as JSON, for the grid (`wpFormat=json`).
    pub json: bool,
    /// Whether the viewer may create properties, which the grid offers as a column.
    pub can_new: bool,
    /// The title of the draft the table is kept in.
    pub draft: String,
    /// The name a new draft is to get.
    pub draft_name: String,
    /// What the confirmation carries.
    pub carried: Option<Carried>,
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
        let carried = params
            .map
            .get("wpPlans")
            .and_then(|p| base64::engine::general_purpose::STANDARD.decode(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok());
        Self {
            rows,
            shared: get("shared"),
            reference: get("reference"),
            batch: get("batch").trim().to_string(),
            token: get("token"),
            step: get("wpStep"),
            save: params.map.contains_key("wpSave"),
            json: params.map.get("wpFormat").is_some_and(|f| f == "json"),
            can_new: false,
            draft: get("draft").trim().to_string(),
            draft_name: get("draftname").trim().to_string(),
            carried,
        }
    }
}

/// The form's fields.
#[allow(clippy::too_many_lines)]
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
        reference: area(
            "ts-createentities-reference",
            "reference",
            &typed.reference,
            2,
        ),
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
        draft_name: typed.draft.is_empty().then(|| FieldRow {
            id: "ts-createentities-draftname".into(),
            label: m.get("ts-createentities-draftname"),
            class: None,
            control: ControlModel::Input {
                id: Some("ts-createentities-draftname".into()),
                name: "draftname".into(),
                input_type: "text".into(),
                inputmode: None,
                value: typed.draft_name.clone(),
                required: false,
                described_by: Some("ts-createentities-draftname-help".into()),
            },
            help: Some(m.get("ts-createentities-draftname-help")),
            help_id: Some("ts-createentities-draftname-help".into()),
        }),
        draft: (!typed.draft.is_empty()).then(|| DraftModel {
            text: m.get("ts-createentities-draft"),
            href: format!("/wiki/{}", title_url(&typed.draft)),
            title: typed.draft.clone(),
        }),
        sheet_data_html: {
            let data = serde_json::json!({
                "page": "create",
                "lang": cx.site.lang,
                "dir": m.dir(),
                "canCreateProperty": typed.can_new,
                "datatypes": offered(&cx.site),
                "conceptBase": cx.site.concept_base,
                "messages": m.with_prefix("ts-sheet-"),
            });
            format!(
                "<script type=\"application/json\" id=\"ts-sheet-data\">{}</script>",
                data.to_string().replace("</", "<\\/")
            )
        },
        submit: Button {
            label: &m.get("ts-createentities-preview"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        save: Button {
            label: &m.get(if typed.draft.is_empty() {
                "ts-createentities-save-new"
            } else {
                "ts-createentities-save"
            }),
            kind: "submit",
            action: Action::Default,
            weight: Weight::Normal,
        }
        .model(),
        preview: None,
        report: None,
    }
}

/// A row's contents in words: its terms' languages and its statements.
fn row_summary(m: &Messages, p: &Plan) -> String {
    let mut parts = Vec::new();
    for (n, msg) in [
        (p.labels.len(), "ts-createentities-row-labels"),
        (p.descriptions.len(), "ts-createentities-row-descriptions"),
        (p.aliases.len(), "ts-createentities-row-aliases"),
        (p.statements.len(), "ts-createentities-row-statements"),
    ] {
        if n > 0 {
            parts.push(m.with(msg, &[&n.to_string()]));
        }
    }
    parts.join(", ")
}

/// A row's status in words, with its class suffix.
fn status_words(m: &Messages, status: &Status) -> (String, &'static str) {
    match status {
        Status::New => (m.get("ts-createentities-status-new"), "new"),
        Status::Matches(id) => (m.with("ts-createentities-status-matches", &[id]), "matches"),
        Status::Ambiguous(n) => (
            m.with("ts-createentities-status-ambiguous", &[&n.to_string()]),
            "ambiguous",
        ),
        Status::TermConflict(n) => (
            m.with("ts-createentities-status-conflict", &[&n.to_string()]),
            "conflict",
        ),
        Status::Invalid(n) => (
            m.with("ts-createentities-status-invalid", &[&n.to_string()]),
            "invalid",
        ),
        Status::Skipped => (m.get("ts-createentities-status-skipped"), "skipped"),
    }
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
    query: &BTreeMap<String, String>,
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

/// `GET Special:CreateManyEntities`, or `Special:CreateManyEntities/{table title}` for a draft
/// (0106 §9): the form, holding the draft's staged rows as the table's text.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
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
    let mut typed = Typed {
        token: token.value.value,
        can_new: may_create_properties(site, &incoming).await,
        ..Typed::default()
    };
    let mut notices = Vec::new();
    if let Some(table) = title
        .split_once('/')
        .map(|(_, t)| t.trim())
        .filter(|t| !t.is_empty())
    {
        match stored(site, &cx, &incoming, table).await {
            Err(e) => return pages::api_failed(&cx, headers, &e),
            Ok(Err(why)) => notices.push(error(&why)),
            Ok(Ok(found)) => match draft::text(&found.definition, &cx.site.lang) {
                Ok(text) => {
                    typed.rows = table::text_of(&text.rows, false);
                    typed.shared = text.shared;
                    typed.reference = text.reference;
                    typed.draft = found.title;
                    if params.map.contains_key("saved") {
                        notices.push(
                            Message::block(
                                MessageKind::Success,
                                &cx.m.get("ts-createentities-draft-saved"),
                            )
                            .model(),
                        );
                    }
                    if !found.definition.rows.is_empty() {
                        notices.push(
                            Message::block(
                                MessageKind::Notice,
                                &cx.m.with(
                                    "ts-createentities-draft-listed",
                                    &[&found.definition.rows.len().to_string()],
                                ),
                            )
                            .model(),
                        );
                    }
                }
                Err(columns) => notices.push(error(&cx.m.with(
                    "ts-createentities-draft-columns",
                    &[table, &columns.join(", ")],
                ))),
            },
        }
    }
    let mut model = form_model(&cx, &typed);
    model.notices = notices;
    page(&cx, headers, StatusCode::OK, &model)
}

/// A table as it is stored.
struct Stored {
    /// Its title, as the API writes it.
    title: String,
    /// The revision read.
    revid: u64,
    /// Its definition.
    definition: Definition,
}

/// The table at `title`, or why it cannot be opened here, as a message.
async fn stored(
    site: &Site,
    cx: &Context,
    incoming: &Incoming,
    title: &str,
) -> Result<Result<Stored, String>, ClientError> {
    use triplespace_client::Lookup as Found;
    let found = site.client().page_source(incoming, title).await?;
    let Found::Found(source) = found.value else {
        return Ok(Err(cx.m.with("ts-createentities-draft-missing", &[title])));
    };
    if source.model != scatter_wikibase_shape::table::MODEL {
        return Ok(Err(cx
            .m
            .with("ts-createentities-draft-not-table", &[title])));
    }
    // The page was held to the limits when it was saved.
    let limits = Limits {
        max_rows: usize::MAX,
        max_columns: usize::MAX,
    };
    Ok(match Definition::parse(&source.text, limits) {
        Ok(definition) => Ok(Stored {
            title: source.title,
            revid: source.revid,
            definition,
        }),
        Err(_) => Err(cx.m.with("ts-createentities-draft-not-table", &[title])),
    })
}

/// The properties a table names, with their labels and data types, in one call.
async fn lookup_of(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    read: &Read,
) -> Result<Lookup, ClientError> {
    let ids = read.properties();
    if ids.is_empty() {
        return Ok(Lookup::default());
    }
    let languages = cx.m.term_chain().join("|");
    let f = site
        .client()
        .entities(incoming, &ids, "labels|datatype", &languages)
        .await?;
    cx.inputs.push(f.cache);
    Ok(Lookup::from_entities(&f.value.found, &cx.m.term_chain()))
}

/// Saves the form's table as a draft (0106 §9; 15 §2.7): the Table page the form names,
/// or a new one under the viewer's name, `Table:Drafts/{user}/{name}` (0106 A1). The
/// title saved and the definition, or why not, as notices.
async fn store(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    typed: &Typed,
    summary: &str,
) -> Result<Result<(String, Definition), Vec<MessageModel>>, ClientError> {
    let rows = rows_of(&typed.rows);
    if rows.is_empty() {
        return Ok(Err(vec![error(&cx.m.get("ts-createentities-empty"))]));
    }
    let mut notices = Vec::new();
    let mut lines = |text: &str| {
        let mut out = Vec::new();
        for l in lines_of(text) {
            match l {
                Ok(l) => out.push(l),
                Err(raw) => {
                    notices.push(error(&cx.m.with("ts-new-statement-malformed", &[&raw])));
                }
            }
        }
        out
    };
    let shared = lines(&typed.shared);
    let reference = lines(&typed.reference);
    if !notices.is_empty() {
        return Ok(Err(notices));
    }
    let lang = cx.site.lang.clone();
    let mut read = Read::of(&rows[0], &rows[1..], shared, reference, &lang);
    read.concept_base.clone_from(&cx.site.concept_base);
    let lookup = lookup_of(site, cx, incoming, &read).await?;
    let (title, base) = if typed.draft.is_empty() {
        let name = if typed.draft_name.is_empty() {
            made_up_batch().replace(TITLE, "").trim().to_string()
        } else {
            typed.draft_name.clone()
        };
        (
            draft::title_for(cx.user.as_deref().unwrap_or_default(), &name),
            None,
        )
    } else {
        match stored(site, cx, incoming, &typed.draft).await? {
            Ok(found) => (found.title.clone(), Some(found)),
            Err(why) => return Ok(Err(vec![error(&why)])),
        }
    };
    let definition =
        match draft::definition(&cx.m, &read, &lookup, base.as_ref().map(|b| &b.definition)) {
            Ok(d) => d,
            Err(problems) => return Ok(Err(problems.iter().map(|p| error(p)).collect())),
        };
    match save(
        site,
        incoming,
        typed,
        &title,
        &definition,
        base.as_ref(),
        summary,
    )
    .await
    {
        Ok(title) => Ok(Ok((title, definition))),
        Err(ClientError::Api { code, info }) => {
            let text = match code.as_str() {
                "badtoken" => cx.m.get("ts-new-expired"),
                "permissiondenied" | "protectedpage" => cx.m.get("ts-permission-denied"),
                "articleexists" => cx.m.with("ts-createentities-draft-exists", &[&title]),
                _ if info.is_empty() => code,
                _ => info,
            };
            Ok(Err(vec![error(&text)]))
        }
        Err(e) => Err(e),
    }
}

/// Writes a definition to its Table page: a new page when `base` is `None`.
async fn save(
    site: &Site,
    incoming: &Incoming,
    typed: &Typed,
    title: &str,
    definition: &Definition,
    base: Option<&Stored>,
    summary: &str,
) -> Result<String, ClientError> {
    use triplespace_client::pages::{EditForm, Saved};
    let text = definition.canonical();
    let saved = site
        .client()
        .save_page(
            incoming,
            &EditForm {
                title,
                text: &text,
                summary,
                minor: false,
                baserevid: base.map(|b| b.revid),
                basetimestamp: None,
                createonly: base.is_none(),
                content_model: base
                    .is_none()
                    .then_some(scatter_wikibase_shape::table::MODEL),
                token: &typed.token,
            },
        )
        .await?;
    Ok(match saved {
        Saved::Saved { title, .. } | Saved::NoChange { title } => title,
    })
}

/// `POST Special:CreateManyEntities`: the preview, or the creation.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let mut typed = Typed::of(params);
    typed.can_new = may_create_properties(site, &incoming).await;
    if typed.save {
        let summary = cx.m.get("ts-createentities-draft-summary");
        return match store(site, &mut cx, &incoming, &typed, &summary).await {
            Ok(Ok((title, _))) => {
                pages::see_other(&format!("/wiki/{TITLE}/{}?saved=1", title_url(&title)), &[])
            }
            Ok(Err(notices)) => {
                let mut model = form_model(&cx, &typed);
                model.notices = notices;
                page(&cx, headers, StatusCode::OK, &model)
            }
            Err(e) => pages::api_failed(&cx, headers, &e),
        };
    }
    if typed.step == "create" {
        return create(site, &mut cx, headers, &incoming, typed).await;
    }
    preview(site, &mut cx, headers, &incoming, typed).await
}

/// Whether the viewer holds `property-create` (0106 §4), which decides whether the grid
/// offers a new property as a column. The check and the API enforce it either way.
async fn may_create_properties(site: &Site, incoming: &Incoming) -> bool {
    site.client()
        .rights(incoming)
        .await
        .is_ok_and(|r| r.iter().any(|x| x == "property-create"))
}

fn error(text: &str) -> MessageModel {
    Message::block(MessageKind::Error, text).model()
}

/// The items whose label and description in one language are the row's (0106 §5): the
/// pair Wikibase would refuse to create a second time.
async fn matches_of(
    site: &Site,
    incoming: &Incoming,
    plan: &Plan,
) -> Result<Vec<String>, ClientError> {
    let mut ids: Vec<String> = Vec::new();
    for label in &plan.labels {
        let Some(description) = plan
            .descriptions
            .iter()
            .find(|d| d.language == label.language)
        else {
            continue;
        };
        let found = site
            .client()
            .search(
                incoming,
                &label.value,
                &label.language,
                "item",
                MATCH_CANDIDATES,
                0,
            )
            .await?;
        let is = |pair: &Option<(String, String)>, t: &plan::Term| {
            pair.as_ref()
                .is_some_and(|(v, l)| *v == t.value && *l == t.language)
        };
        for h in found.value.hits {
            if is(&h.label, label) && is(&h.description, description) && !ids.contains(&h.id) {
                ids.push(h.id);
            }
        }
    }
    Ok(ids)
}

/// The preview: the table read, the properties looked up, each row planned, matched and
/// ordered; the table's text written back with what was found.
#[allow(clippy::too_many_lines)]
async fn preview(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut typed: Typed,
) -> Response {
    let mut rows = rows_of(&typed.rows);
    let tabbed = table::is_tabbed(&typed.rows);
    let mut model = form_model(cx, &typed);
    let m = &cx.m;
    if rows.len() < 2 {
        model.notices.push(error(&m.get("ts-createentities-empty")));
        return answer(cx, headers, &typed, &model);
    }
    if rows.len() - 1 > MAX_ROWS {
        model.notices.push(error(&m.with(
            "ts-createentities-too-many",
            &[&(rows.len() - 1).to_string(), &MAX_ROWS.to_string()],
        )));
        return answer(cx, headers, &typed, &model);
    }
    let lang = cx.site.lang.clone();
    let mut notices = Vec::new();
    // Whether something other than a row's own problems keeps the run from starting.
    let mut blocked = false;
    let mut lines = |text: &str| {
        let mut out = Vec::new();
        for l in lines_of(text) {
            match l {
                Ok(l) => out.push(l),
                Err(raw) => notices.push(error(&m.with("ts-new-statement-malformed", &[&raw]))),
            }
        }
        out
    };
    let shared = lines(&typed.shared);
    let reference = lines(&typed.reference);
    let mut read = Read::of(&rows[0], &rows[1..], shared, reference, &lang);
    read.concept_base.clone_from(&cx.site.concept_base);

    // New properties (0106 §4): the right to create them, and a confirmed data type each.
    if !read.new.is_empty() {
        match site.client().rights(incoming).await {
            Ok(r) if r.iter().any(|x| x == "property-create") => {}
            Ok(_) => {
                blocked = true;
                notices.push(error(&m.get("ts-createentities-new-denied")));
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
        let known = offered(&cx.site);
        for i in 0..read.new.len() {
            let label = read.new[i].label.clone();
            match read.new[i].datatype.clone() {
                Some(d) if known.contains(&d) => {}
                Some(d) => {
                    blocked = true;
                    read.new[i].datatype = None;
                    notices.push(error(&m.with(
                        "ts-createentities-new-bad-type",
                        &[&label, &d, &known.join(", ")],
                    )));
                }
                None => {
                    blocked = true;
                    if let Some(d) = plan::propose(m, &read.values_of(&label)) {
                        // Written into the header, so that checking again confirms it.
                        read.new[i].datatype = Some(d.to_string());
                        for (at, c) in read.columns.iter().enumerate() {
                            if matches!(c, Column::New { label: l, .. } if *l == label) {
                                rows[0][at] = table::header(
                                    &Column::New {
                                        label: label.clone(),
                                        datatype: Some(d.to_string()),
                                    },
                                    &lang,
                                );
                            }
                        }
                        notices.push(
                            Message::block(
                                MessageKind::Warning,
                                &m.with(
                                    "ts-createentities-new-proposed",
                                    &[&label, &datatype_name(m, d), d],
                                ),
                            )
                            .model(),
                        );
                    } else {
                        notices.push(error(
                            &m.with("ts-createentities-new-choose", &[&label, &known.join(", ")]),
                        ));
                    }
                }
            }
        }
    }

    let lookup = match lookup_of(site, cx, incoming, &read).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let m = &cx.m;
    let reads: Vec<&str> = read.reference.iter().filter_map(|l| l.column()).collect();
    let mut column_notes = Vec::new();
    for (h, c) in read.headers.iter().zip(&read.columns) {
        let property = |p: &str| match lookup.label(p) {
            Some((label, _)) => format!("{label} ({p})"),
            None => p.to_string(),
        };
        column_notes.push(match c {
            Column::Id => m.with("ts-createentities-col-id", &[h]),
            Column::Label(l) => m.with("ts-createentities-col-label", &[h, l]),
            Column::Description(l) => m.with("ts-createentities-col-description", &[h, l]),
            Column::Aliases(l) => m.with("ts-createentities-col-aliases", &[h, l]),
            Column::Property(p) => match lookup.label(p) {
                Some((label, _)) => m.with("ts-createentities-col-property", &[h, label, p]),
                None if lookup.datatype(p).is_some() => {
                    m.with("ts-createentities-col-property", &[h, p, p])
                }
                None => m.with("ts-createentities-col-unknown", &[h]),
            },
            Column::New { label, .. } => {
                let dt = read
                    .declared(label)
                    .and_then(|i| read.new[i].datatype.clone());
                match dt {
                    Some(d) => m.with(
                        "ts-createentities-col-new",
                        &[h, label, &datatype_name(m, &d)],
                    ),
                    None => m.with("ts-createentities-col-new-untyped", &[h, label]),
                }
            }
            Column::Reference { of, property: p } => m.with(
                "ts-createentities-col-reference",
                &[h, &property(of), &property(p)],
            ),
            Column::Plain(_) if reads.iter().any(|r| r.eq_ignore_ascii_case(h.trim())) => {
                m.with("ts-createentities-col-read", &[h])
            }
            Column::Plain(_) => m.with("ts-createentities-col-ignored", &[h]),
        });
    }

    let kinds: Vec<ColumnInfo> = read
        .columns
        .iter()
        .map(|c| {
            let (kind, property) = match c {
                Column::Id => ("id", None),
                Column::Label(_) => ("label", None),
                Column::Description(_) => ("description", None),
                Column::Aliases(_) => ("aliases", None),
                Column::Property(p) => ("property", Some(p.as_str())),
                Column::New { .. } => ("new", None),
                Column::Reference { property, .. } => ("reference", Some(property.as_str())),
                Column::Plain(_) => ("plain", None),
            };
            ColumnInfo {
                kind,
                label: property
                    .and_then(|p| lookup.label(p))
                    .map(|(l, _)| l.to_string()),
                datatype: match c {
                    Column::New { label, .. } => read
                        .declared(label)
                        .and_then(|i| read.new[i].datatype.clone()),
                    _ => property
                        .and_then(|p| lookup.datatype(p))
                        .map(str::to_string),
                },
            }
        })
        .collect();
    // The labels of the items the cells of item columns name, for the grid's chips.
    let mut named: Vec<String> = Vec::new();
    for (at, k) in kinds.iter().enumerate() {
        let items = k.datatype.as_deref() == Some("wikibase-item") || k.kind == "id";
        let units = k.datatype.as_deref() == Some("quantity");
        if items || units {
            for r in &read.rows {
                for v in table::split_values(r.get(at).map_or("", String::as_str)) {
                    // An item cell's value, or the unit after a quantity's amount.
                    let id = if units {
                        v.split_whitespace().nth(1).unwrap_or_default()
                    } else {
                        v.as_str()
                    };
                    if crate::special::values::implied_type(id).as_deref() == Some("item") {
                        named.push(id.to_ascii_uppercase());
                    }
                }
            }
        }
    }
    named.sort();
    named.dedup();
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    if typed.json && !named.is_empty() {
        let languages = cx.m.term_chain().join("|");
        if let Ok(f) = site
            .client()
            .entities(incoming, &named, "labels", &languages)
            .await
        {
            let found = Lookup::from_entities(&f.value.found, &cx.m.term_chain());
            for id in &named {
                if let Some((l, _)) = found.label(id) {
                    labels.insert(id.clone(), l.to_string());
                }
            }
        }
    }

    let mut plans = plan::plan(m, &read, &lookup);

    // A row whose label and description are an existing item's adds to it (0106 §5).
    let mut filled: Vec<(usize, String)> = Vec::new();
    for p in &mut plans {
        if !p.auto || p.target != Target::New {
            continue;
        }
        match matches_of(site, incoming, p).await {
            Ok(ids) if ids.len() == 1 => {
                filled.push((p.n, ids[0].clone()));
                p.target = Target::Existing(ids[0].clone());
            }
            Ok(ids) if ids.len() > 1 => {
                p.errors.push(m.with(
                    "ts-createentities-ambiguous",
                    &[&ids.len().to_string(), &ids.join(", ")],
                ));
                p.target = Target::Ambiguous(ids);
            }
            Ok(_) => {}
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
    }
    // The items added to, as they stand: for the terms that differ.
    let mut targets: Vec<String> = plans
        .iter()
        .filter_map(|p| match &p.target {
            Target::Existing(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    targets.sort();
    targets.dedup();
    if !targets.is_empty() {
        match site
            .client()
            .entities(incoming, &targets, "labels|descriptions|aliases", "")
            .await
        {
            Ok(f) => {
                for p in &mut plans {
                    if let Target::Existing(id) = &p.target {
                        let entity = f.value.found.get(id).cloned();
                        plan::against(m, p, entity.as_ref());
                    }
                }
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
    }
    let order = plan::order(m, &mut plans);

    // The text, with the matches the preview found written into `id` (0106 §2).
    if !filled.is_empty() {
        let at = read
            .columns
            .iter()
            .position(|c| *c == Column::Id)
            .unwrap_or_else(|| {
                for (i, r) in rows.iter_mut().enumerate() {
                    r.insert(0, if i == 0 { "id".into() } else { String::new() });
                }
                0
            });
        for (n, id) in &filled {
            if let Some(r) = rows.get_mut(*n) {
                if r.len() <= at {
                    r.resize(at + 1, String::new());
                }
                r[at].clone_from(id);
            }
        }
    }
    typed.rows = table::text_of(&rows, tabbed);

    let count = |f: &dyn Fn(&Plan) -> bool| plans.iter().filter(|p| f(p)).count();
    let creates = count(&|p| p.writable() && p.target == Target::New);
    let adds = count(&|p| p.writable() && matches!(p.target, Target::Existing(_)));
    let left = plans.len() - creates - adds;
    let batch = if typed.batch.is_empty() {
        made_up_batch()
    } else {
        typed.batch.clone()
    };
    typed.batch.clone_from(&batch);
    let carried = Carried {
        plans: order.iter().map(|i| plans[*i].clone()).collect(),
        new: read.new.clone(),
        lang: lang.clone(),
        draft: (!typed.draft.is_empty()).then(|| typed.draft.clone()),
    };
    let mut hidden = vec![
        ("token".to_string(), typed.token.clone()),
        ("wpStep".to_string(), "create".to_string()),
        ("batch".to_string(), batch.clone()),
        (
            "wpPlans".to_string(),
            base64::engine::general_purpose::STANDARD
                .encode(serde_json::to_vec(&carried).unwrap_or_default()),
        ),
    ];
    if !typed.draft.is_empty() {
        // Importing saves the draft first (21 §6.7), so the confirmation carries the table.
        for (k, v) in [
            ("draft", &typed.draft),
            ("rows", &typed.rows),
            ("shared", &typed.shared),
            ("reference", &typed.reference),
        ] {
            hidden.push((k.to_string(), v.clone()));
        }
    }
    let mut model = form_model(cx, &typed);
    model.notices = notices;
    model.preview = Some(PreviewModel {
        title: m.get("ts-createentities-preview-title"),
        columns: column_notes,
        kinds,
        labels,
        summary: m.with(
            "ts-createentities-preview-summary",
            &[
                &creates.to_string(),
                &adds.to_string(),
                &left.to_string(),
                &batch,
            ],
        ),
        headings: [
            "ts-createentities-col-line",
            "ts-createentities-col-item",
            "ts-createentities-col-status",
            "ts-createentities-col-holds",
            "ts-createentities-col-problems",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: plans
            .iter()
            .map(|p| {
                let (status, kind) = status_words(m, &p.status());
                PreviewRow {
                    line: p.n.to_string(),
                    shown: p.shown.clone(),
                    status,
                    kind,
                    item: match &p.target {
                        Target::Existing(id) => Some((entity_href(id, "item"), id.clone())),
                        _ => None,
                    },
                    summary: row_summary(m, p),
                    errors: p.errors.clone(),
                }
            })
            .collect(),
        confirm_note: m.get("ts-createentities-confirm-note"),
        hidden,
        submit: (!blocked && creates + adds > 0).then(|| {
            Button {
                label: &m.with(
                    "ts-createentities-confirm",
                    &[&creates.to_string(), &adds.to_string(), &left.to_string()],
                ),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .model()
        }),
        cancel: format!("/wiki/{TITLE}"),
        cancel_label: m.get("ts-import-cancel"),
    });
    answer(cx, headers, &typed, &model)
}

/// The check's answer: the page, or for the grid (`wpFormat=json`; 21 §6.7) the same
/// findings as JSON: the notices, each row's status and problems, what the columns
/// hold, and the table's text as the check wrote it back.
fn answer(
    cx: &Context,
    headers: &HeaderMap,
    typed: &Typed,
    model: &CreateEntitiesPage,
) -> Response {
    if !typed.json {
        return page(cx, headers, StatusCode::OK, model);
    }
    let p = model.preview.as_ref();
    let body = serde_json::json!({
        "notices": model
            .notices
            .iter()
            .map(|n| serde_json::json!({
                "error": n.classes.contains("--error"),
                "text": n.text,
            }))
            .collect::<Vec<_>>(),
        "text": typed.rows,
        "summary": p.map(|p| p.summary.clone()),
        "confirm": p.and_then(|p| p.submit.as_ref()).map(|b| b.label.clone()),
        "columns": p.map(|p| p.kinds.clone()),
        "labels": p.map(|p| p.labels.clone()),
        "rows": p.map(|p| p.rows.clone()),
    });
    let mut r = Response::new(axum::body::Body::from(body.to_string()));
    r.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json; charset=utf-8"),
    );
    r.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    r
}

/// What the API said, in the page's words, and whether the run stops there.
fn refusal(m: &Messages, code: &str, info: String) -> (String, bool) {
    let text = match code {
        "badtoken" => m.get("ts-new-expired"),
        "permissiondenied" => m.get("ts-permission-denied"),
        "ts-batch-rejected" => m.get("ts-createentities-rejected"),
        _ if info.is_empty() => code.to_string(),
        _ => info,
    };
    (text, matches!(code, "badtoken" | "ts-batch-rejected"))
}

/// The run: the declared properties, then one `wbeditentity` per planned row, under the
/// batch.
#[allow(clippy::too_many_lines)]
async fn create(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    typed: Typed,
) -> Response {
    let Some(carried) = typed.carried.clone().filter(|c| !c.plans.is_empty()) else {
        let mut model = form_model(cx, &typed);
        model
            .notices
            .push(error(&cx.m.get("ts-createentities-empty")));
        return page(cx, headers, StatusCode::OK, &model);
    };
    let batch = if typed.batch.is_empty() {
        made_up_batch()
    } else {
        typed.batch.clone()
    };
    // A draft is saved as the run reads it, before anything is written (21 §6.7).
    let mut kept: Option<(String, Definition)> = None;
    if carried.draft.is_some() {
        let summary =
            cx.m.with("ts-createentities-draft-import-summary", &[&batch]);
        match store(site, cx, incoming, &typed, &summary).await {
            Ok(Ok(saved)) => kept = Some(saved),
            Ok(Err(notices)) => {
                let mut model = form_model(cx, &typed);
                model.notices = notices;
                return page(cx, headers, StatusCode::OK, &model);
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
    }
    let mut job = None;
    let mut known = Known::default();
    let mut notices = Vec::new();
    let mut properties = Vec::new();
    let mut stopped = false;
    // The properties first (0106 §4), so that the rows can name them.
    for d in &carried.new {
        let Some(datatype) = &d.datatype else {
            known.properties.push(None);
            continue;
        };
        if stopped {
            known.properties.push(None);
            continue;
        }
        let data = serde_json::json!({
            "labels": {&carried.lang: {"language": carried.lang, "value": d.label}},
            "datatype": datatype,
        });
        match site
            .client()
            .create_entity_in(incoming, "property", &data, &typed.token, Some(&batch))
            .await
        {
            Ok((c, b)) => {
                job = job.or(b);
                properties.push(cx.m.with(
                    "ts-createentities-report-property",
                    &[&d.label, &c.id, &datatype_name(&cx.m, datatype)],
                ));
                known.properties.push(Some(c.id));
            }
            Err(ClientError::Api { code, info }) => {
                let (text, stop) = refusal(&cx.m, &code, info);
                stopped = stopped || stop;
                notices.push(error(
                    &cx.m
                        .with("ts-createentities-property-failed", &[&d.label, &text]),
                ));
                known.properties.push(None);
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
    }
    let mut rows = Vec::new();
    let (mut created_n, mut added_n) = (0usize, 0usize);
    for p in &carried.plans {
        if stopped {
            break;
        }
        let mut row = ReportRow {
            line: p.n.to_string(),
            shown: p.shown.clone(),
            created: None,
            note: None,
            error: None,
        };
        let waits = |on: String| cx.m.with("ts-createentities-waits", &[&on]);
        let result = match &p.target {
            Target::New => match plan::create_data(p, &known) {
                Err(on) => {
                    row.error = Some(waits(on));
                    rows.push(row);
                    continue;
                }
                Ok(data) => site
                    .client()
                    .create_entity_in(incoming, "item", &data, &typed.token, Some(&batch))
                    .await
                    .map(|(c, b)| {
                        created_n += 1;
                        (c.id, b, None)
                    }),
            },
            Target::Existing(id) => {
                let entity = match site
                    .client()
                    .entities(
                        incoming,
                        std::slice::from_ref(id),
                        "labels|descriptions|aliases|claims",
                        "",
                    )
                    .await
                {
                    Ok(f) => f.value.found.get(id).cloned(),
                    Err(e) => return pages::api_failed(cx, headers, &e),
                };
                let Some(entity) = entity else {
                    row.error = Some(cx.m.with("ts-createentities-no-item", &[id]));
                    rows.push(row);
                    continue;
                };
                match plan::add_data(p, &entity, &known) {
                    Err(on) => {
                        row.error = Some(waits(on));
                        rows.push(row);
                        continue;
                    }
                    Ok(None) => Ok((
                        id.clone(),
                        None,
                        Some(cx.m.get("ts-createentities-report-nothing")),
                    )),
                    Ok(Some(data)) => site
                        .client()
                        .edit_entity_in(incoming, id, &data, &typed.token, Some(&batch))
                        .await
                        .map(|b| {
                            added_n += 1;
                            (
                                id.clone(),
                                b,
                                Some(cx.m.get("ts-createentities-report-added")),
                            )
                        }),
                }
            }
            Target::Ambiguous(_) | Target::Skip => continue,
        };
        match result {
            Ok((id, b, note)) => {
                job = job.or(b);
                known.rows.insert(p.n, id.clone());
                row.created = Some((entity_href(&id, "item"), id));
                row.note = note;
            }
            Err(ClientError::Api { code, info }) => {
                let (text, stop) = refusal(&cx.m, &code, info);
                stopped = stop;
                row.error = Some(text);
            }
            Err(e) => return pages::api_failed(cx, headers, &e),
        }
        rows.push(row);
    }
    // The draft records the run (15 §2.6, §2.7): written rows become listed rows, refused
    // rows stay with their reason. The entities are written by now; if this save fails,
    // the report says so and names them.
    let mut draft_model = None;
    if let Some((title, mut definition)) = kept {
        let n = |r: &ReportRow| r.line.parse::<usize>().ok();
        let written: Vec<(usize, String)> = rows
            .iter()
            .filter_map(|r| Some((n(r)?, r.created.as_ref()?.1.clone())))
            .collect();
        let refused: Vec<(usize, String)> = rows
            .iter()
            .filter_map(|r| Some((n(r)?, r.error.clone()?)))
            .collect();
        draft::record(&mut definition, &written, &refused);
        let left = definition.staged.as_ref().map_or(0, |s| s.rows.len());
        let summary =
            cx.m.with("ts-createentities-draft-import-summary", &[&batch]);
        let recorded = match stored(site, cx, incoming, &title).await {
            Ok(Ok(base)) => save(
                site,
                incoming,
                &typed,
                &title,
                &definition,
                Some(&base),
                &summary,
            )
            .await
            .map_err(|e| e.to_string()),
            Ok(Err(why)) => Err(why),
            Err(e) => Err(e.to_string()),
        };
        match recorded {
            Ok(title) => {
                draft_model = Some(DraftModel {
                    text: cx
                        .m
                        .with("ts-createentities-report-draft", &[&left.to_string()]),
                    href: format!("/wiki/{}", title_url(&title)),
                    title,
                });
            }
            Err(why) => notices.push(error(
                &cx.m
                    .with("ts-createentities-draft-not-recorded", &[&title, &why]),
            )),
        }
    }
    let m = &cx.m;
    let mut model = form_model(cx, &Typed::default());
    model.notices = notices;
    model.report = Some(ReportModel {
        title: m.get("ts-createentities-report-title"),
        summary: m.with(
            "ts-createentities-report-summary",
            &[
                &created_n.to_string(),
                &added_n.to_string(),
                &(carried.plans.len() - created_n - added_n).to_string(),
            ],
        ),
        job: job.map(|j| {
            (
                crate::special::jobs::href(j),
                m.with("ts-createentities-report-job", &[&j.to_string()]),
            )
        }),
        properties,
        draft: draft_model,
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
