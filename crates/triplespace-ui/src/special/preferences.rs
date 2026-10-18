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
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use triplespace_client::settings::{Pref, Preferences, Saved};
use triplespace_client::{CacheInfo, ClientError, Incoming};

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{self, Control, Kind};
use crate::theme::Theme;

const TITLE: &str = "Special:Preferences";

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

fn errors_html(e: Option<&Vec<String>>) -> String {
    e.map_or_else(String::new, |e| {
        e.iter()
            .map(|t| pages::message(MessageKind::Error, t))
            .collect()
    })
}

fn hidden(name: &str, value: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
        esc(name),
        esc(value)
    )
}

fn row(m: &Messages, theme: &Theme, key: &str, p: &Pref, back: &Returned<'_>) -> String {
    let sheet = sheet_pref(m, theme, key, p);
    let p = sheet.as_ref().map_or(p, |(p, _)| p);
    let kind = kind_of(&p.ty);
    let id = format!("ts-p-{key}");
    let help_key = format!("{}-help", p.label);
    let help_id = format!("{id}-help");
    let name = format!("v.{key}");
    let label_text = label(m, key, p);
    let mut s = format!(
        "<div class=\"ts-setting\" id=\"pref-{}\"><div class=\"ts-setting__head\"><label class=\"ts-setting__label\" for=\"{}\">{}</label> <code class=\"ts-setting__key\">{}</code></div>",
        esc(key),
        esc(&id),
        esc(&label_text),
        esc(key)
    );
    let has_help = m.has(&help_key);
    if has_help {
        let _ = write!(
            s,
            "<p class=\"ts-setting__help\" id=\"{}\">{}</p>",
            esc(&help_id),
            esc(&m.get(&help_key))
        );
    }
    s.push_str(&errors_html(back.errors.get(key)));
    s.push_str("<div class=\"ts-setting__value\">");
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
    s.push_str(&controls::render(&Control {
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
    }));
    if p.own {
        let _ = write!(
            s,
            "<button class=\"cdx-button cdx-button--weight-quiet ts-setting__reset\" type=\"submit\" name=\"reset\" value=\"{}\" aria-label=\"{}\">{}</button>",
            esc(key),
            esc(&m.with("ts-settings-reset-label", &[&label_text])),
            esc(&m.get("ts-settings-reset"))
        );
    }
    s.push_str("</div></div>");
    s
}

fn body(
    m: &Messages,
    theme: &Theme,
    prefs: &Preferences,
    token: &str,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> (String, Vec<Tab>) {
    let mut tabs = Vec::new();
    let mut out = String::new();
    for g in &prefs.groups {
        let keys: Vec<(&String, &Pref)> = prefs
            .preferences
            .iter()
            .filter(|(_, p)| p.group == g.id)
            .collect();
        if keys.is_empty() {
            continue;
        }
        let title = if m.has(&g.label) {
            m.get(&g.label)
        } else {
            g.id.clone()
        };
        tabs.push(Tab {
            label: title.clone(),
            href: format!("#{}", g.id),
            current: false,
        });
        let _ = write!(
            out,
            "<section class=\"ts-settings__section\" id=\"{id}\" aria-labelledby=\"{id}-title\"><h2 id=\"{id}-title\">{}</h2>",
            esc(&title),
            id = esc(&g.id)
        );
        if saved.is_some_and(|s| *s == g.id) {
            out.push_str(&pages::message(
                MessageKind::Success,
                &m.get("ts-preferences-saved"),
            ));
        }
        if g.id == "appearance" {
            let _ = write!(
                out,
                "<p>{}</p>",
                esc(&m.get("ts-preferences-appearance-note"))
            );
        }
        if back.section == g.id {
            out.push_str(&errors_html(back.errors.get("")));
        }
        let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        let _ = write!(
            out,
            "<form class=\"ts-settings__form\" method=\"post\" action=\"/w/index.php?title={}\">{}{}{}",
            title_url(TITLE),
            hidden("section", &g.id),
            hidden("token", token),
            hidden("keys", &names.join(" "))
        );
        for (k, p) in keys {
            out.push_str(&row(m, theme, k, p, back));
        }
        let _ = write!(
            out,
            "<div class=\"ts-settings__save\">{}</div></form></section>",
            Button {
                label: &m.get("ts-preferences-save"),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .render()
            .unwrap_or_default()
        );
    }
    let _ = write!(
        out,
        "<section class=\"ts-settings__section\" id=\"restore\"><form class=\"ts-settings__form\" method=\"post\" action=\"/w/index.php?title={}\">{}{}{}<p>{}</p>{}</form></section>",
        title_url(TITLE),
        hidden("section", "restore"),
        hidden("token", token),
        hidden("reset_all", "1"),
        esc(&m.get("ts-preferences-restore-intro")),
        Button {
            label: &m.get("ts-preferences-restore"),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Normal,
        }
        .render()
        .unwrap_or_default()
    );
    if saved.is_some_and(|s| s == "restore") {
        out.insert_str(
            0,
            &pages::message(MessageKind::Success, &m.get("ts-preferences-restored")),
        );
    }
    (
        format!("<div class=\"ts-settings ts-preferences\">{out}</div>"),
        tabs,
    )
}

/// The page for a reader who is not logged in.
fn anonymous(cx: &Context, headers: &HeaderMap) -> Response {
    let login = cx.features.has(Feature::Login).then(|| {
        link(
            &format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE)),
            &esc(&cx.m.get("ts-login")),
            "",
        )
    });
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-preferences-title"),
        returnto: Some(TITLE.to_string()),
        body: format!(
            "{}{}<p>{}</p>",
            pages::message(MessageKind::Notice, &m.get("ts-preferences-anon")),
            login.map(|l| format!("<p>{l}</p>")).unwrap_or_default(),
            esc(&m.get("ts-preferences-anon-appearance"))
        ),
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
    let (html, tabs) = body(
        &cx.m,
        &cx.theme,
        &prefs.value,
        &token.value.value,
        back,
        saved,
    );
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
        // The palette's and type set's empty choice is the wiki's own: it resets.
        if crate::appearance::SHEET_KEYS.iter().any(|(k, _)| *k == key)
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
