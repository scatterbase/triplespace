//! `Special:Preferences` (0085 §5; 19 §2.6) and the Appearance menu's form (0085 §6).
//!
//! The page is the signed-in reader's preferences, one section per group of
//! `preferences.toml`, each a form that saves with one `PATCH /account/preferences`;
//! every key has **Reset**, and **Restore all defaults** resets them all. A reader who is
//! not logged in is offered **Log in** and pointed to the Appearance menu, which keeps
//! their choices in the browser.
//!
//! The Appearance menu's form posts here with `appearance=1`. For a signed-in reader the
//! web tier saves the keys that changed and answers `303` back to the page the reader was
//! on. The form carries no token, since it is drawn on every page, including public
//! ones: the web tier accepts it only from the site's own pages (`Origin` or
//! `Sec-Fetch-Site`), and then asks the API for the reader's token itself.

use std::collections::{BTreeMap, BTreeSet};

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Map, Value, json};
use triplespace_client::settings::{Pref, Preferences, Saved};
use triplespace_client::{CacheInfo, ClientError, Incoming};

use crate::Site;
use crate::codex::{Action, Button, ButtonModel, Message, MessageKind, MessageModel, Weight};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{
    self, Control, ControlModel, Kind, OptionModel, ResetModel, SettingRow,
};
use crate::theme::Theme;

const TITLE: &str = "Special:Preferences";

/// The key of the reader's skin.
const SKIN_KEY: &str = "ui.skin";

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/preferences.html` draws.
// ---------------------------------------------------------------------------------------

/// One section's form: a group of `preferences.toml`, saved together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrefSection {
    /// The group's ID, the section's anchor.
    pub id: String,
    /// Its heading.
    pub title: String,
    /// That it was saved.
    pub saved: Option<MessageModel>,
    /// The Appearance section's note.
    pub note: Option<String>,
    /// What the form refused about the section as a whole.
    pub errors: Vec<MessageModel>,
    /// The CSRF token.
    pub token: String,
    /// The keys it holds, space-separated.
    pub keys: String,
    /// The rows.
    pub rows: Vec<SettingRow>,
    /// The notification matrix, on the tab that has its cells (19 §2.6).
    pub matrix: Option<MatrixModel>,
    /// **Save**.
    pub save: ButtonModel,
}

/// The notification matrix (16 §5.3; 19 §2.6): a table of reasons by channels, each
/// cell a checkbox that is a preference of its own, saved with the tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixModel {
    /// The table's caption.
    pub caption: String,
    /// What the table is for.
    pub help: String,
    /// The first column's heading.
    pub reason_heading: String,
    /// The channels' headings.
    pub channels: Vec<String>,
    /// A row per reason.
    pub rows: Vec<MatrixRow>,
}

/// One reason's row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixRow {
    /// The reason, in words.
    pub label: String,
    /// A cell per channel; `None` where the reason has no cell for it.
    pub cells: Vec<Option<controls::ControlModel>>,
}

/// The matrix of a tab's cells: the reasons in the inbox menu's order, the channels as
/// the API lists them.
/// `None` where the tab has none.
fn matrix_model(
    m: &Messages,
    cells: &[(&String, &Pref)],
    group: &str,
    back: &Returned<'_>,
) -> Option<MatrixModel> {
    let mut reasons: Vec<&str> = Vec::new();
    let mut channels: Vec<&str> = Vec::new();
    for (_, p) in cells {
        let c = p.cell.as_ref()?;
        if !reasons.contains(&c.reason.as_str()) {
            reasons.push(&c.reason);
        }
        if !channels.contains(&c.channel.as_str()) {
            channels.push(&c.channel);
        }
    }
    if reasons.is_empty() {
        return None;
    }
    // The kinds in the order the inbox's menu lists them; one it does not know, last.
    let known = crate::special::notifications::REASONS;
    reasons.sort_by_key(|r| known.iter().position(|k| k == r).unwrap_or(known.len()));
    let word = |prefix: &str, id: &str| {
        let key = format!("{prefix}{id}");
        if m.has(&key) {
            m.get(&key)
        } else {
            id.to_string()
        }
    };
    let rows = reasons
        .iter()
        .map(|reason| {
            let label = word("ts-notif-reason-", reason);
            MatrixRow {
                cells: channels
                    .iter()
                    .map(|channel| {
                        let (key, p) = cells.iter().find(|(_, p)| {
                            p.cell
                                .as_ref()
                                .is_some_and(|c| c.reason == *reason && c.channel == *channel)
                        })?;
                        let name = format!("v.{key}");
                        // What the reader sent, where the form came back refused.
                        let checked = match (back.section == group, back.form) {
                            (true, Some(f)) => f.map.contains_key(&name),
                            _ => p.value == Some(Value::Bool(true)),
                        };
                        Some(controls::ControlModel::Checkbox {
                            id: Some(format!("ts-p-{key}")),
                            name,
                            value: "1".into(),
                            checked,
                            // Named for a screen reader by its row and its column.
                            label: Some(m.with(
                                "ts-preferences-matrix-cell",
                                &[&label, &word("ts-notif-channel-", channel)],
                            )),
                            note: None,
                            disabled: false,
                            described_by: None,
                        })
                    })
                    .collect(),
                label,
            }
        })
        .collect();
    Some(MatrixModel {
        caption: m.get("ts-preferences-matrix"),
        help: m.get("ts-preferences-matrix-help"),
        reason_heading: m.get("ts-preferences-matrix-reason"),
        channels: channels
            .iter()
            .map(|c| word("ts-notif-channel-", c))
            .collect(),
        rows,
    })
}

/// **Restore all defaults**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreForm {
    /// The CSRF token.
    pub token: String,
    /// What it does.
    pub intro: String,
    /// The button.
    pub button: ButtonModel,
}

/// The page for a reader who is not logged in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnonymousPrefs {
    /// That preferences need an account.
    pub notice: MessageModel,
    /// **Log in**, where the wiki offers it.
    pub login: Option<Choice>,
    /// That the Appearance menu keeps choices in the browser.
    pub appearance: String,
}

/// `Special:Preferences`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreferencesPage {
    /// The page for a reader who is not logged in, in place of the rest.
    pub anonymous: Option<AnonymousPrefs>,
    /// That every default was restored.
    pub restored: Option<MessageModel>,
    /// The sections.
    pub sections: Vec<PrefSection>,
    /// **Restore all defaults**.
    pub restore: Option<RestoreForm>,
}

/// The palette and type set are chosen from what the wiki lets readers choose (0086 §4),
/// as the Appearance menu offers them: a select whose empty choice is the wiki's own.
fn sheet_pref(
    m: &Messages,
    theme: &Theme,
    key: &str,
    p: &Pref,
) -> Option<(Pref, Vec<(String, String)>)> {
    let (_, sheet) = crate::appearance::SHEET_KEYS
        .iter()
        .find(|(k, _)| *k == key)?;
    let options = crate::appearance::sheet_options(m, theme, *sheet);
    let mut p = p.clone();
    p.ty = "choice".into();
    p.choices = options.iter().skip(1).map(|(v, _)| v.clone()).collect();
    if p.value
        .as_ref()
        .and_then(Value::as_str)
        .is_some_and(|v| !p.choices.iter().any(|c| c == v))
    {
        p.value = None;
    }
    Some((p, options))
}

/// The skin is chosen from the skins the wiki lets readers choose (0097 §8), where there
/// is more than one: a select whose empty choice is the wiki's own.
fn skin_pref(
    m: &Messages,
    skins: &[String],
    key: &str,
    p: &Pref,
) -> Option<(Pref, Vec<(String, String)>)> {
    if key != SKIN_KEY || skins.len() < 2 {
        return None;
    }
    let mut options = vec![(String::new(), m.get("ts-preferences-skin-default"))];
    options.extend(skins.iter().map(|s| (s.clone(), s.clone())));
    let mut p = p.clone();
    p.ty = "choice".into();
    p.choices = skins.to_vec();
    if p.value
        .as_ref()
        .and_then(Value::as_str)
        .is_some_and(|v| !skins.iter().any(|c| c == v))
    {
        p.value = None;
    }
    Some((p, options))
}

/// The skins a reader of this wiki may choose, by name: the loaded ones, less those the
/// wiki's `skins.available` leaves out.
fn offered_skins(site: &Site, cx: &Context) -> Vec<String> {
    site.skins()
        .loaded()
        .map(|s| s.manifest().name.clone())
        .filter(|n| {
            cx.site
                .skins
                .available
                .as_ref()
                .is_none_or(|a| a.iter().any(|x| x == n))
        })
        .collect()
}

/// How a preference type is edited.
fn kind_of(ty: &str) -> Kind {
    match ty {
        "boolean" => Kind::Boolean,
        "choice" => Kind::Choice,
        "integer" => Kind::Integer,
        "language-list" => Kind::Lines,
        "json" => Kind::Json,
        _ => Kind::Line,
    }
}

fn label(m: &Messages, key: &str, p: &Pref) -> String {
    if m.has(&p.label) {
        m.get(&p.label)
    } else {
        key.to_string()
    }
}

/// What came back with a form.
#[derive(Debug, Default)]
struct Returned<'a> {
    section: String,
    form: Option<&'a Params>,
    errors: BTreeMap<String, Vec<String>>,
}

/// The periods `feeds.period` offers, as ISO 8601 durations, and the rows a page
/// `rc.limit` offers: what the feeds' own forms offer.
const PERIODS: [(&str, &str); 6] = [
    ("P1D", "1"),
    ("P3D", "3"),
    ("P7D", "7"),
    ("P14D", "14"),
    ("P30D", "30"),
    ("P90D", "90"),
];
const LIMITS: [&str; 4] = ["50", "100", "250", "500"];

/// How many choices are few enough to show at once, as radio buttons.
const FEW: usize = 5;

/// A control in the form that suits its key best (19 §2.6): the Appearance menu's icon
/// buttons for the keys it draws so; radio buttons for a choice of a few that always
/// has a value; a select of the usual values for a period or a page length, with the
/// reader's own value kept where it is none of them. Each posts what the plain control
/// would, so the form reads back the same.
fn shaped(m: &Messages, key: &str, p: &Pref, control: ControlModel, label: &str) -> ControlModel {
    match control {
        ControlModel::Select {
            id,
            name,
            options,
            described_by,
        } => {
            let id = id.unwrap_or_default();
            let current = options
                .iter()
                .find(|o| o.selected)
                .map_or("", |o| o.value.as_str());
            if let Some(options) = crate::appearance::icon_options(m, key, current) {
                return ControlModel::Icons {
                    id,
                    name,
                    label: label.to_string(),
                    options,
                    described_by,
                };
            }
            // A choice that may have no value keeps its select, whose first option says so.
            if options.len() <= FEW && options.iter().all(|o| !o.value.is_empty()) {
                return ControlModel::Radios {
                    id,
                    name,
                    label: label.to_string(),
                    options,
                    described_by,
                };
            }
            ControlModel::Select {
                id: Some(id),
                name,
                options,
                described_by,
            }
        }
        ControlModel::Input {
            id,
            name,
            value,
            described_by,
            ..
        } if key == "feeds.period" || key == "rc.limit" => {
            let mut options: Vec<OptionModel> = if p.ty == "duration" {
                PERIODS
                    .iter()
                    .map(|(v, days)| OptionModel {
                        value: (*v).to_string(),
                        label: m.with("ts-changes-days-n", &[days]),
                        selected: *v == value,
                    })
                    .collect()
            } else {
                LIMITS
                    .iter()
                    .map(|v| OptionModel {
                        value: (*v).to_string(),
                        label: (*v).to_string(),
                        selected: *v == value,
                    })
                    .collect()
            };
            if !value.is_empty() && !options.iter().any(|o| o.selected) {
                options.push(OptionModel {
                    value: value.clone(),
                    label: value,
                    selected: true,
                });
            }
            ControlModel::Select {
                id,
                name,
                options,
                described_by,
            }
        }
        other => other,
    }
}

/// One preference's row, as a model.
fn row_model(
    m: &Messages,
    theme: &Theme,
    skins: &[String],
    key: &str,
    p: &Pref,
    back: &Returned<'_>,
) -> SettingRow {
    let sheet = sheet_pref(m, theme, key, p).or_else(|| skin_pref(m, skins, key, p));
    let p = sheet.as_ref().map_or(p, |(p, _)| p);
    let kind = kind_of(&p.ty);
    let id = format!("ts-p-{key}");
    let help_key = format!("{}-help", p.label);
    let help_id = format!("{id}-help");
    let name = format!("v.{key}");
    let label_text = label(m, key, p);
    let has_help = m.has(&help_key);
    let typed;
    let value = match (back.section == p.group, back.form) {
        (true, Some(f)) => match (kind, f.map.get(&name)) {
            (Kind::Boolean, raw) => {
                typed = Value::Bool(raw.is_some());
                Some(&typed)
            }
            (_, Some(raw)) => {
                typed = Value::String(raw.clone());
                Some(&typed)
            }
            (_, None) => p.value.as_ref(),
        },
        _ => p.value.as_ref(),
    };
    let choice_label = |c: &str| {
        if let Some((_, options)) = &sheet {
            return options
                .iter()
                .find(|(v, _)| v == c)
                .map_or_else(|| c.to_string(), |(_, l)| l.clone());
        }
        let k = format!("{}-{}", p.label, c.replace(' ', "-").to_lowercase());
        if m.has(&k) { m.get(&k) } else { c.to_string() }
    };
    let empty = sheet.as_ref().map(|(_, options)| options[0].1.clone());
    let control = Control {
        id: &id,
        name: &name,
        kind,
        choices: &p.choices,
        value,
        empty_choice: empty
            .as_deref()
            .or_else(|| p.value.is_none().then_some("—")),
        described_by: has_help.then_some(help_id.as_str()),
        choice_label: Some(&choice_label),
    }
    .model();
    let control = shaped(m, key, p, control, &label_text);
    let grouped = matches!(
        control,
        ControlModel::Radios { .. } | ControlModel::Icons { .. }
    );
    SettingRow {
        row_id: format!("pref-{key}"),
        key: key.to_string(),
        label: label_text.clone(),
        // A group of buttons is named by the row's label itself.
        label_for: (!grouped).then(|| id.clone()),
        chips: Vec::new(),
        help_id: Some(help_id.clone()),
        help: has_help.then(|| m.get(&help_key)),
        errors: back
            .errors
            .get(key)
            .map(|e| {
                e.iter()
                    .map(|t| Message::block(MessageKind::Error, t).model())
                    .collect()
            })
            .unwrap_or_default(),
        control: Some(control),
        shown: None,
        reset: p.own.then(|| ResetModel {
            value: key.to_string(),
            aria_label: m.with("ts-settings-reset-label", &[&label_text]),
            label: m.get("ts-settings-reset"),
        }),
        lock: None,
        shown_chip: None,
        note: None,
    }
}

/// The page's model, signed in.
fn body_model(
    m: &Messages,
    theme: &Theme,
    skins: &[String],
    prefs: &Preferences,
    token: &str,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> PreferencesPage {
    let errors = |e: Option<&Vec<String>>| -> Vec<MessageModel> {
        e.map(|e| {
            e.iter()
                .map(|t| Message::block(MessageKind::Error, t).model())
                .collect()
        })
        .unwrap_or_default()
    };
    let mut sections = Vec::new();
    for g in &prefs.groups {
        let keys: Vec<(&String, &Pref)> = prefs
            .preferences
            .iter()
            .filter(|(_, p)| p.group == g.id)
            .collect();
        if keys.is_empty() {
            continue;
        }
        let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        // The matrix's cells are drawn as a table, not as rows of their own.
        let (cells, keys): (Vec<_>, Vec<_>) = keys.into_iter().partition(|(_, p)| p.cell.is_some());
        sections.push(PrefSection {
            id: g.id.clone(),
            title: if m.has(&g.label) {
                m.get(&g.label)
            } else {
                g.id.clone()
            },
            saved: saved.is_some_and(|s| *s == g.id).then(|| {
                Message::block(MessageKind::Success, &m.get("ts-preferences-saved")).model()
            }),
            note: (g.id == "appearance").then(|| m.get("ts-preferences-appearance-note")),
            errors: if back.section == g.id {
                errors(back.errors.get(""))
            } else {
                Vec::new()
            },
            token: token.to_string(),
            keys: names.join(" "),
            rows: keys
                .iter()
                .map(|(k, p)| row_model(m, theme, skins, k, p, back))
                .collect(),
            matrix: matrix_model(m, &cells, &g.id, back),
            save: Button {
                label: &m.get("ts-preferences-save"),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .model(),
        });
    }
    PreferencesPage {
        anonymous: None,
        restored: saved.is_some_and(|s| s == "restore").then(|| {
            Message::block(MessageKind::Success, &m.get("ts-preferences-restored")).model()
        }),
        sections,
        restore: Some(RestoreForm {
            token: token.to_string(),
            intro: m.get("ts-preferences-restore-intro"),
            button: Button {
                label: &m.get("ts-preferences-restore"),
                kind: "submit",
                action: Action::Destructive,
                weight: Weight::Normal,
            }
            .model(),
        }),
    }
}

/// The page for a reader who is not logged in.
fn anonymous(cx: &Context, headers: &HeaderMap) -> Response {
    let m = &cx.m;
    let login_href = format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE));
    let offered = cx.features.has(Feature::Login);
    let model = PreferencesPage {
        anonymous: Some(AnonymousPrefs {
            notice: Message::block(MessageKind::Notice, &m.get("ts-preferences-anon")).model(),
            login: offered.then(|| Choice {
                href: login_href.clone(),
                label: m.get("ts-login"),
                current: false,
            }),
            appearance: m.get("ts-preferences-anon-appearance"),
        }),
        restored: None,
        sections: Vec::new(),
        restore: None,
    };
    let body = cx.draw("special/preferences.html", model);
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-preferences-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let mut cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Preferences) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    cx.inputs.push(CacheInfo::default());
    Ok(cx)
}

async fn render(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &Returned<'_>,
    saved: Option<&String>,
    status: StatusCode,
) -> Response {
    let client = site.client();
    let (prefs, token) = tokio::join!(client.preferences(incoming), client.token(incoming, "csrf"));
    let (prefs, token) = match (prefs, token) {
        (Ok(p), Ok(t)) => (p, t),
        (Err(ClientError::Api { code, .. }), _) if code == "notloggedin" => {
            return anonymous(cx, headers);
        }
        (Err(e), _) | (_, Err(e)) => return pages::api_failed(cx, headers, &e),
    };
    let model = body_model(
        &cx.m,
        &cx.theme,
        &offered_skins(site, cx),
        &prefs.value,
        &token.value.value,
        back,
        saved,
    );
    // The page's tabs are the frame's: one per section.
    let tabs = model
        .sections
        .iter()
        .map(|s| Tab {
            label: s.title.clone(),
            href: format!("#{}", s.id),
            current: false,
        })
        .collect();
    let html = cx.draw("special/preferences.html", model);
    let page = pages::respond(cx, headers, status, |m| Page {
        title: m.get("ts-preferences-title"),
        returnto: Some(TITLE.to_string()),
        tabs,
        body: html,
        ..Page::default()
    });
    pages::with_cookies(page, &token.set_cookies)
}

/// `GET Special:Preferences`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if cx.user.is_none() {
        return anonymous(&cx, headers);
    }
    render(
        site,
        &mut cx,
        headers,
        &incoming,
        &Returned::default(),
        params.map.get("saved"),
        StatusCode::OK,
    )
    .await
}

/// The body of a section's save.
fn patch_of(
    form: &Params,
    prefs: &Preferences,
) -> (Map<String, Value>, BTreeMap<String, Vec<String>>) {
    let f = &form.map;
    let mut body = Map::new();
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(t) = f.get("token") {
        body.insert("token".into(), json!(t));
    }
    if f.contains_key("reset_all") {
        body.insert("reset_all".into(), json!(true));
        return (body, errors);
    }
    if let Some(k) = f.get("reset") {
        body.insert("reset".into(), json!([k]));
        return (body, errors);
    }
    let keys: BTreeSet<&str> = f
        .get("keys")
        .map_or_else(BTreeSet::new, |k| k.split_whitespace().collect());
    let mut set = Map::new();
    let mut reset = Vec::new();
    for key in keys {
        let Some(p) = prefs.preferences.get(key) else {
            continue;
        };
        // The palette's, type set's and skin's empty choice is the wiki's own: it resets.
        if (key == SKIN_KEY || crate::appearance::SHEET_KEYS.iter().any(|(k, _)| *k == key))
            && f.get(&format!("v.{key}")).is_some_and(String::is_empty)
        {
            if p.own {
                reset.push(key.to_string());
            }
            continue;
        }
        let kind = kind_of(&p.ty);
        match controls::parse(kind, f.get(&format!("v.{key}")).map(String::as_str)) {
            Err(e) => errors.entry(key.to_string()).or_default().push(e),
            Ok(None) => {}
            Ok(Some(v)) => {
                if p.value.as_ref() != Some(&v) {
                    set.insert(key.to_string(), v);
                }
            }
        }
    }
    if !set.is_empty() {
        body.insert("set".into(), Value::Object(set));
    }
    if !reset.is_empty() {
        body.insert("reset".into(), json!(reset));
    }
    (body, errors)
}

/// Whether a form came from the site's own pages: the browser's `Origin` names this host,
/// or `Sec-Fetch-Site` says the request is same-origin.
fn same_origin(headers: &HeaderMap, host: Option<&str>) -> bool {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return site == "same-origin";
    }
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    match (origin, host) {
        (Some(o), Some(h)) => {
            let o = o.split_once("://").map_or(o, |(_, rest)| rest);
            o.eq_ignore_ascii_case(h)
        }
        _ => false,
    }
}

/// The page to return to from the Appearance menu: a title, never an address elsewhere.
fn returnto(form: &BTreeMap<String, String>, mainpage: &str) -> String {
    let t = form.get("returnto").map_or("", |v| v.trim()).to_string();
    if t.is_empty() || t.contains("://") || t.starts_with('/') {
        mainpage.to_string()
    } else {
        t
    }
}

/// `POST Special:Preferences&appearance=1`: the Appearance menu without JavaScript.
async fn post_appearance(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: &Context,
    form: &Params,
) -> Response {
    let back = format!(
        "/wiki/{}",
        title_url(&returnto(&form.map, &cx.site.mainpage))
    );
    if cx.user.is_none() {
        // Nothing can be kept for a reader who is not logged in and has no script.
        return pages::see_other(&back, &[]);
    }
    if !same_origin(headers, incoming.host()) {
        return (
            StatusCode::FORBIDDEN,
            [(header::CACHE_CONTROL, "no-store")],
            "Forbidden\n",
        )
            .into_response();
    }
    let current = cx.prefs.clone().unwrap_or_default();
    let mut set = Map::new();
    let mut reset = Vec::new();
    for (key, choices, _) in crate::appearance::KEYS {
        if let Some(v) = form.map.get(*key)
            && choices.contains(&v.as_str())
            && current.get(*key).and_then(Value::as_str) != Some(v.as_str())
        {
            set.insert((*key).to_string(), json!(v));
        }
    }
    for (key, sheet) in crate::appearance::SHEET_KEYS {
        let Some(v) = form.map.get(key) else { continue };
        let now = current.get(key).and_then(Value::as_str).unwrap_or_default();
        if v.is_empty() {
            if !now.is_empty() {
                reset.push(key);
            }
        } else if v != now && cx.theme.choices(sheet).iter().any(|o| o.name == *v) {
            set.insert(key.to_string(), json!(v));
        }
    }
    if set.is_empty() && reset.is_empty() {
        return pages::see_other(&back, &[]);
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let body = json!({"set": set, "reset": reset, "token": token.value.value});
    match site.client().save_preferences(incoming, &body).await {
        Ok(Saved::Ok(_)) => pages::see_other(&back, &token.set_cookies),
        Ok(Saved::Refused(r)) => {
            let e = ClientError::Api {
                code: r.first().map(|x| x.code.clone()).unwrap_or_default(),
                info: r.first().map(|x| x.message.clone()).unwrap_or_default(),
            };
            pages::api_failed(cx, headers, &e)
        }
        Err(e) => pages::api_failed(cx, headers, &e),
    }
}

/// `POST Special:Preferences`: saves a section, resets a key or all of them, or takes the
/// Appearance menu's form.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if query.contains_key("appearance") || form.map.contains_key("appearance") {
        return post_appearance(site, headers, &incoming, &cx, form).await;
    }
    if cx.user.is_none() {
        return anonymous(&cx, headers);
    }
    let section = form.map.get("section").cloned().unwrap_or_default();
    let prefs = match site.client().preferences(&incoming).await {
        Ok(p) => p.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let (body, errors) = patch_of(form, &prefs);
    let back_to = |saved: bool| {
        let anchor = if section == "restore" {
            String::new()
        } else {
            format!("#{section}")
        };
        format!(
            "/wiki/{}{}{anchor}",
            title_url(TITLE),
            if saved {
                format!("?saved={section}")
            } else {
                String::new()
            }
        )
    };
    if !errors.is_empty() {
        let back = Returned {
            section: section.clone(),
            form: Some(form),
            errors,
        };
        return render(
            site,
            &mut cx,
            headers,
            &incoming,
            &back,
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    if !["set", "reset", "reset_all"]
        .iter()
        .any(|k| body.contains_key(*k))
    {
        return pages::see_other(&back_to(false), &[]);
    }
    match site
        .client()
        .save_preferences(&incoming, &Value::Object(body))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&back_to(true), &[]),
        Ok(Saved::Refused(refusals)) => {
            let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for r in refusals {
                let text = match r.code.as_str() {
                    "badtoken" => cx.m.get("ts-login-expired"),
                    "permissiondenied" => cx.m.get("ts-preferences-denied"),
                    _ => r.message,
                };
                // The API names the key in its message; the form shows it at the top.
                errors.entry(String::new()).or_default().push(text);
            }
            let back = Returned {
                section: section.clone(),
                form: Some(form),
                errors,
            };
            render(
                site,
                &mut cx,
                headers,
                &incoming,
                &back,
                None,
                StatusCode::BAD_REQUEST,
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A preference's row (19 §5.10): its control from its type, help where the label has
    /// some, **Reset** only for the reader's own value, and the reader's text back after a
    /// refusal, with the row's errors.
    #[test]
    fn a_row_is_a_model() {
        let m = Messages::for_language("en");
        let theme = Theme::shipped();
        let p = Pref {
            value: Some(json!(50)),
            own: false,
            ty: "integer".into(),
            group: "history".into(),
            label: "ts-pref-rc-limit".into(),
            ..Pref::default()
        };
        let r = row_model(&m, &theme, &[], "rc.limit", &p, &Returned::default());
        assert_eq!(
            (r.row_id.as_str(), r.label_for.as_deref()),
            ("pref-rc.limit", Some("ts-p-rc.limit"))
        );
        assert!(r.reset.is_none() && r.errors.is_empty());
        // A page length is a select of the usual ones.
        let selected = |r: &SettingRow| match &r.control {
            Some(ControlModel::Select { options, .. }) => options
                .iter()
                .filter(|o| o.selected)
                .map(|o| o.value.clone())
                .collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(selected(&r), ["50"]);
        let own = Pref {
            own: true,
            ..p.clone()
        };
        assert_eq!(
            row_model(&m, &theme, &[], "rc.limit", &own, &Returned::default())
                .reset
                .unwrap()
                .value,
            "rc.limit"
        );
        let mut sent = Params::default();
        sent.map.insert("v.rc.limit".into(), "twenty".into());
        let back = Returned {
            section: "history".into(),
            form: Some(&sent),
            errors: BTreeMap::from([("rc.limit".to_string(), vec!["not a number".to_string()])]),
        };
        let r = row_model(&m, &theme, &[], "rc.limit", &p, &back);
        assert_eq!(r.errors.len(), 1);
        // What the reader sent comes back chosen, though it is none of the usual ones.
        assert_eq!(selected(&r), ["twenty"]);
    }

    /// 19 §2.6: a control takes the form that suits its key. The Appearance menu's icon
    /// keys are icon buttons; a choice of a few that always has a value is radio
    /// buttons, named by the row's label; one that may have none keeps its select; a
    /// period is a select of the usual periods.
    #[test]
    fn a_control_suits_its_key() {
        let m = Messages::for_language("en");
        let theme = Theme::shipped();
        let choice = |key: &str, choices: &[&str], value: Option<&str>| {
            let p = Pref {
                value: value.map(|v| json!(v)),
                ty: "choice".into(),
                choices: choices.iter().map(|c| (*c).to_string()).collect(),
                group: "appearance".into(),
                label: format!("ts-pref-{}", key.replace('.', "-")),
                ..Pref::default()
            };
            row_model(&m, &theme, &[], key, &p, &Returned::default())
        };
        let r = choice("ui.mode", &["auto", "light", "dark"], Some("dark"));
        assert!(r.label_for.is_none());
        match r.control {
            Some(ControlModel::Icons {
                name,
                label,
                options,
                ..
            }) => {
                assert_eq!(
                    (name.as_str(), label.as_str()),
                    ("v.ui.mode", "Colour mode")
                );
                assert_eq!(options.len(), 3);
                assert!(options[2].checked && options[2].icon_html.starts_with("<svg"));
            }
            other => panic!("{other:?}"),
        }
        let r = choice(
            "notifications.email-digest",
            &["immediate", "daily", "weekly"],
            Some("daily"),
        );
        match r.control {
            Some(ControlModel::Radios { options, .. }) => {
                assert_eq!(options.len(), 3);
                assert!(options[1].selected && !options[0].selected);
            }
            other => panic!("{other:?}"),
        }
        let r = choice("threads.destination", &["here", "upstream"], None);
        assert!(matches!(r.control, Some(ControlModel::Select { .. })));
        let period = Pref {
            value: Some(json!("P7D")),
            ty: "duration".into(),
            group: "history".into(),
            label: "ts-pref-feeds-period".into(),
            ..Pref::default()
        };
        match row_model(
            &m,
            &theme,
            &[],
            "feeds.period",
            &period,
            &Returned::default(),
        )
        .control
        {
            Some(ControlModel::Select { options, .. }) => {
                assert_eq!(options.len(), 6);
                assert_eq!(
                    options
                        .iter()
                        .find(|o| o.selected)
                        .map(|o| o.label.as_str()),
                    Some("7 days")
                );
            }
            other => panic!("{other:?}"),
        }
    }

    /// 19 §2.6; 16 §5.3: the notification matrix is a table of the tab's cells, a row a
    /// reason and a column a channel, each a checkbox named as its preference; the cells
    /// are saved with the tab and are not rows of their own.
    #[test]
    fn the_matrix_is_a_table_of_cells() {
        use triplespace_client::settings::{Cell, Group};
        let m = Messages::for_language("en");
        let cell = |reason: &str, on: bool| {
            (
                format!("notifications.{reason}.web"),
                Pref {
                    value: Some(json!(on)),
                    ty: "boolean".into(),
                    group: "notifications".into(),
                    cell: Some(Cell {
                        reason: reason.into(),
                        channel: "web".into(),
                    }),
                    ..Pref::default()
                },
            )
        };
        let digest = (
            "notifications.email-digest".to_string(),
            Pref {
                value: Some(json!("daily")),
                ty: "choice".into(),
                choices: vec!["immediate".into(), "daily".into(), "weekly".into()],
                group: "notifications".into(),
                label: "ts-pref-notifications-email-digest".into(),
                ..Pref::default()
            },
        );
        let prefs = Preferences {
            groups: vec![Group {
                id: "notifications".into(),
                label: "ts-prefs-group-notifications".into(),
            }],
            preferences: [cell("talk", true), cell("mention", false), digest]
                .into_iter()
                .collect(),
        };
        let page = body_model(
            &m,
            &Theme::shipped(),
            &[],
            &prefs,
            "tok",
            &Returned::default(),
            None,
        );
        let sec = &page.sections[0];
        // The cells are in the form's keys, and only the digest is a row.
        assert!(sec.keys.contains("notifications.mention.web"));
        assert_eq!(sec.rows.len(), 1);
        let matrix = sec.matrix.as_ref().expect("a matrix");
        assert_eq!(matrix.channels, ["On this site"]);
        let rows: Vec<(&str, bool)> = matrix
            .rows
            .iter()
            .map(|r| match &r.cells[0] {
                Some(ControlModel::Checkbox { checked, name, .. }) => {
                    assert!(name.starts_with("v.notifications."));
                    (r.label.as_str(), *checked)
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("Messages on my talk page", true),
                ("Mentions of me", false)
            ],
            "in the order the inbox's own menu lists the kinds"
        );
    }

    #[test]
    fn same_origin_checks() {
        let mut h = HeaderMap::new();
        assert!(!same_origin(&h, Some("librarybase.org")));
        h.insert(header::ORIGIN, "https://librarybase.org".parse().unwrap());
        assert!(same_origin(&h, Some("librarybase.org")));
        h.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!same_origin(&h, Some("librarybase.org")));
        h.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(!same_origin(&h, Some("librarybase.org")));
        h.insert("sec-fetch-site", "same-origin".parse().unwrap());
        assert!(same_origin(&h, Some("librarybase.org")));
    }

    #[test]
    fn returnto_stays_on_the_site() {
        let f = |v: &str| BTreeMap::from([("returnto".to_string(), v.to_string())]);
        assert_eq!(returnto(&f("Item:Q6"), "Project:Home"), "Item:Q6");
        assert_eq!(
            returnto(&f("https://evil.example/"), "Project:Home"),
            "Project:Home"
        );
        assert_eq!(
            returnto(&f("//evil.example"), "Project:Home"),
            "Project:Home"
        );
    }
}
