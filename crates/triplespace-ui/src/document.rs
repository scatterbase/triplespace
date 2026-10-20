//! Document pages (0008; 19 §3.1–3.3): a page in a page namespace, read, edited and its
//! history listed, at MediaWiki's addresses: `/wiki/{title}`, `?oldid=`, `?redirect=no`,
//! `?action=edit`, `?action=history` and `?action=render`, and the edit form posted to
//! `index.php?title={title}&action=submit`.
//!
//! - **Read** is `action=parse` of the latest revision, redirects followed with
//!   "(Redirected from …)" under the title, or of one revision with a note that it is
//!   an old one. The HTML is the API's, sanitized there (0008 A39); the site adds the
//!   categories at the foot. A title with no page says so, with **Create this page** for
//!   a signed-in viewer; the main page with no page of its own is the generated one
//!   ([`crate::home`]).
//! - **Edit** is a form of the source, a summary and the minor flag, posted with the
//!   viewer's CSRF token and the revision it started from, which is the edit's base
//!   (18 §2.3). **Show preview** renders the text through `action=parse` with the
//!   pre-save transform. A save that conflicts keeps the viewer's text, shows the current
//!   one under it, and takes the current revision as the new base. `index.php?action=edit`
//!   without a title is the **New page** form, which asks for one.
//! - **History** is the page's rows of the activity feed (`target=page:{id}`), drawn as an
//!   entity's history is ([`crate::entity::history`]).
//!
//! Not yet (19 §3.1–3.2): the "About this page" panel, the Page data, Links here and Talk
//! tabs, the side-by-side live preview in WebAssembly, CodeMirror, link autocomplete,
//! **Insert template…**, line-marked conflicts and the Format selector.

use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::{
    CacheInfo, ClientError, EditForm, Incoming, Lookup, Parsed, Saved, Source,
};

use crate::Site;
use crate::codex::{Action as ButtonAction, Button, Field, MessageKind, Weight};
use crate::entity::history;
use crate::frame::{Page, Tab, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params};
use crate::{home, special};

/// What the request asks of the title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Read,
    Edit,
    History,
    Render,
}

impl Ask {
    fn of(params: &Params) -> Self {
        match params.map.get("action").map(String::as_str) {
            Some("edit" | "submit") => Self::Edit,
            Some("history") => Self::History,
            Some("render") => Self::Render,
            _ => Self::Read,
        }
    }
}

/// The tab a page is shown under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabName {
    Read,
    Edit,
    History,
}

fn tabs(m: &Messages, title: &str, exists: bool, current: TabName) -> Vec<Tab> {
    let base = format!("/wiki/{}", title_url(title));
    let tab = |name: TabName, key: &str, href: String| Tab {
        label: m.get(key),
        href,
        current: name == current,
    };
    let mut v = vec![
        tab(TabName::Read, "ts-tab-read", base.clone()),
        tab(
            TabName::Edit,
            if exists {
                "ts-tab-edit"
            } else {
                "ts-tab-create"
            },
            format!("{base}?action=edit"),
        ),
    ];
    if exists {
        v.push(tab(
            TabName::History,
            "ts-tab-history",
            format!("{base}?action=history"),
        ));
    }
    v
}

/// The categories at the foot of a page (0038 §3), hidden ones left out.
fn categories_html(m: &Messages, parsed: &Parsed) -> String {
    let shown: Vec<_> = parsed.categories.iter().filter(|c| !c.hidden).collect();
    if shown.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "<div class=\"ts-catlinks\"><span class=\"ts-catlinks__label\">{}</span><ul class=\"ts-catlinks__list\">",
        esc(&m.get("ts-page-categories"))
    );
    for c in shown {
        let _ = write!(
            s,
            "<li class=\"ts-catlinks__item\">{}</li>",
            link(
                &format!("/wiki/Category:{}", title_url(&c.name)),
                &esc(&c.name),
                ""
            )
        );
    }
    s.push_str("</ul></div>");
    s
}

/// A page's content, as `action=render` and the Read tab show it.
fn content_html(m: &Messages, parsed: &Parsed) -> String {
    format!(
        "<div class=\"ts-document__content\">{}</div>{}",
        parsed.html,
        categories_html(m, parsed)
    )
}

/// What goes under the title: where a redirect was followed from, or that the revision
/// is an old one.
fn subtitle(m: &Messages, parsed: &Parsed, oldid: Option<u64>) -> Option<String> {
    if let Some(from) = &parsed.redirected_from {
        return Some(format!(
            "<span class=\"ts-document__redirected\">{}</span>",
            esc(&m.with("ts-page-redirected-from", &["\u{1}"])).replacen(
                '\u{1}',
                &link(
                    &format!("/wiki/{}?redirect=no", title_url(from)),
                    &esc(from),
                    ""
                ),
                1
            )
        ));
    }
    oldid.map(|_| {
        format!(
            "<span class=\"ts-document__old\">{} {}</span>",
            esc(&m.get("ts-page-old-revision")),
            link(
                &format!("/wiki/{}", title_url(&parsed.title)),
                &esc(&m.get("ts-page-current-revision")),
                ""
            )
        )
    })
}

/// The Read tab, or `action=render`.
async fn read(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
    params: &Params,
    render: bool,
) -> Response {
    let oldid = params.map.get("oldid").and_then(|o| o.parse::<u64>().ok());
    let follow = params.map.get("redirect").is_none_or(|r| r != "no");
    let parsed = match site
        .client()
        .parse_page(incoming, &title, oldid, follow)
        .await
    {
        Ok(p) => p,
        Err(ClientError::Api { code, .. }) => {
            return match code.as_str() {
                "missingtitle" => missing(&cx, headers, title, render),
                "nosuchrevid" | "nosuchpageid" | "permissiondenied" if oldid.is_some() => {
                    no_revision(&cx, headers, title, oldid.unwrap_or(0))
                }
                // A title outside the page namespaces: the site's own `404`, which also
                // sends `Main Page` on to the main page.
                _ if render => pages::fragment(&cx, headers, StatusCode::NOT_FOUND, String::new),
                _ => pages::no_page(&cx, headers, title),
            };
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(parsed.cache);
    let parsed = parsed.value;
    if render {
        return pages::fragment(&cx, headers, StatusCode::OK, || {
            content_html(&cx.m, &parsed)
        });
    }
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: parsed.title.clone(),
        returnto: Some(parsed.title.clone()),
        subtitle_html: subtitle(m, &parsed, oldid),
        tabs: tabs(m, &parsed.title, true, TabName::Read),
        body: format!(
            "<div class=\"ts-document\">{}</div>",
            content_html(m, &parsed)
        ),
        ..Page::default()
    })
}

/// A title with no page: the generated main page for the main page's title, the `404`
/// of a missing page otherwise.
fn missing(cx: &Context, headers: &HeaderMap, title: String, render: bool) -> Response {
    if pages::is_mainpage(&title, &cx.site) {
        if render {
            return pages::fragment(cx, headers, StatusCode::OK, || home::body(cx));
        }
        return pages::respond(cx, headers, StatusCode::OK, |_| home::page(cx));
    }
    if cx.site.mainpage != "Main Page" && title.eq_ignore_ascii_case("Main Page") {
        return pages::redirect_to(cx, &cx.site.mainpage.clone());
    }
    if render {
        return pages::fragment(cx, headers, StatusCode::NOT_FOUND, String::new);
    }
    pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| {
        let mut body = pages::message(MessageKind::Notice, &m.get("ts-page-missing"));
        if cx.user.is_some() {
            let _ = write!(
                body,
                "<p>{}</p>",
                link(
                    &format!("/wiki/{}?action=edit", title_url(&title)),
                    &esc(&m.get("ts-page-missing-create")),
                    ""
                )
            );
        }
        Page {
            returnto: Some(title.clone()),
            tabs: tabs(m, &title, false, TabName::Read),
            title,
            body,
            ..Page::default()
        }
    })
}

/// A revision that does not exist, or is not the viewer's to see.
fn no_revision(cx: &Context, headers: &HeaderMap, title: String, oldid: u64) -> Response {
    pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| Page {
        body: pages::message(
            MessageKind::Error,
            &m.with("ts-page-no-revision", &[&oldid.to_string()]),
        ),
        returnto: Some(title.clone()),
        title,
        ..Page::default()
    })
}

/// A title that cannot name a page.
fn bad_title(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    pages::respond(cx, headers, StatusCode::BAD_REQUEST, |m| Page {
        title: m.get("ts-page-bad-title-heading"),
        body: pages::message(MessageKind::Error, &m.with("ts-page-bad-title", &[title])),
        ..Page::default()
    })
}

// ---- editing ---------------------------------------------------------------------------

/// The edit form's state.
#[derive(Debug, Clone, Default)]
struct Draft {
    /// The title, as the API writes it.
    title: String,
    /// Whether the page existed when the edit began.
    exists: bool,
    /// The text.
    text: String,
    /// The summary.
    summary: String,
    /// A minor edit.
    minor: bool,
    /// The base revision.
    base: Option<u64>,
    /// The base revision's time.
    base_time: Option<String>,
}

impl Draft {
    fn of_source(s: &Source) -> Self {
        Self {
            title: s.title.clone(),
            exists: true,
            text: s.text.clone(),
            base: Some(s.revid),
            base_time: Some(s.timestamp.clone()),
            ..Self::default()
        }
    }

    fn of_form(title: &str, form: &Params) -> Self {
        let get = |k: &str| form.map.get(k).cloned().unwrap_or_default();
        // A textarea's value comes back with CRLF line ends; the page's are LF.
        let text = get("wpTextbox1").replace("\r\n", "\n");
        Self {
            title: title.to_string(),
            exists: !form.map.contains_key("wpCreateonly"),
            text,
            summary: get("wpSummary"),
            minor: form.map.contains_key("wpMinoredit"),
            base: form.map.get("baseRevId").and_then(|b| b.parse().ok()),
            base_time: form
                .map
                .get("wpEdittime")
                .filter(|t| !t.is_empty())
                .cloned(),
        }
    }
}

/// The edit form, with a preview, notices and the current text of a conflict.
fn edit_form(m: &Messages, token: &str, d: &Draft) -> String {
    let mut s = format!(
        "<form class=\"ts-form ts-editform\" method=\"post\" action=\"/w/index.php?title={}&amp;action=submit\"><input type=\"hidden\" name=\"wpEditToken\" value=\"{}\">",
        title_url(&d.title),
        esc(token)
    );
    if let Some(b) = d.base {
        let _ = write!(
            s,
            "<input type=\"hidden\" name=\"baseRevId\" value=\"{b}\">"
        );
    }
    if let Some(t) = &d.base_time {
        let _ = write!(
            s,
            "<input type=\"hidden\" name=\"wpEdittime\" value=\"{}\">",
            esc(t)
        );
    }
    if !d.exists {
        s.push_str("<input type=\"hidden\" name=\"wpCreateonly\" value=\"1\">");
    }
    let _ = write!(
        s,
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"wpTextbox1\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-area\"><textarea class=\"cdx-text-area__textarea ts-editform__text\" id=\"wpTextbox1\" name=\"wpTextbox1\" rows=\"25\" lang=\"\" dir=\"auto\" spellcheck=\"true\">{}</textarea></div></div></div>",
        esc(&m.get("ts-pageedit-text")),
        // A leading newline would be eaten by the parser of the textarea's content.
        esc(&format!("\n{}", d.text))
    );
    s.push_str(
        &Field {
            id: "wpSummary",
            name: "wpSummary",
            kind: "text",
            value: &d.summary,
            label: &m.get("ts-pageedit-summary"),
            description: None,
            help: None,
            required: false,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
    );
    let _ = write!(
        s,
        "<div class=\"cdx-checkbox\"><div class=\"cdx-checkbox__wrapper\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"wpMinoredit\" name=\"wpMinoredit\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><div class=\"cdx-checkbox__label cdx-label\"><label class=\"cdx-label__label\" for=\"wpMinoredit\"><span class=\"cdx-label__label__text\">{}</span></label></div></div></div>",
        if d.minor { " checked" } else { "" },
        esc(&m.get("ts-pageedit-minor"))
    );
    let save = Button {
        label: &m.get(if d.exists {
            "ts-pageedit-save"
        } else {
            "ts-pageedit-save-new"
        }),
        kind: "submit",
        action: ButtonAction::Progressive,
        weight: Weight::Primary,
    }
    .render()
    .unwrap_or_default()
    .replacen("<button ", "<button name=\"wpSave\" value=\"1\" ", 1);
    let preview = Button::submit(&m.get("ts-pageedit-preview"))
        .render()
        .unwrap_or_default()
        .replacen("<button ", "<button name=\"wpPreview\" value=\"1\" ", 1);
    let cancel = link(
        &format!("/wiki/{}", title_url(&d.title)),
        &esc(&m.get("ts-pageedit-cancel")),
        " class=\"ts-editform__cancel\"",
    );
    let _ = write!(
        s,
        "<div class=\"ts-form__actions\">{save} {preview} {cancel}</div></form>"
    );
    s
}

/// The edit page.
#[allow(clippy::too_many_arguments)]
fn edit_page(
    cx: &Context,
    headers: &HeaderMap,
    token: &str,
    d: &Draft,
    notices: &[(MessageKind, String)],
    preview: Option<&Parsed>,
    current: Option<&str>,
) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| {
        let mut body = String::from("<div class=\"ts-edit-document\">");
        for (k, t) in notices {
            body.push_str(&pages::message(*k, t));
        }
        if let Some(p) = preview {
            let _ = write!(
                body,
                "<section class=\"ts-editform__preview\" aria-labelledby=\"ts-preview-heading\"><h2 id=\"ts-preview-heading\">{}</h2>{}<div class=\"ts-document\">{}</div></section>",
                esc(&m.get("ts-pageedit-preview-heading")),
                pages::message(MessageKind::Warning, &m.get("ts-pageedit-preview-note")),
                content_html(m, p)
            );
        }
        body.push_str(&edit_form(m, token, d));
        if let Some(c) = current {
            let _ = write!(
                body,
                "<section class=\"ts-editform__current\"><h2>{}</h2><div class=\"cdx-text-area\"><textarea class=\"cdx-text-area__textarea ts-editform__text\" rows=\"15\" readonly dir=\"auto\" aria-label=\"{}\">{}</textarea></div></section>",
                esc(&m.get("ts-pageedit-conflict-current")),
                esc(&m.get("ts-pageedit-conflict-current")),
                esc(&format!("\n{c}"))
            );
        }
        body.push_str("</div>");
        Page {
            title: m.with(
                if d.exists {
                    "ts-pageedit-title"
                } else {
                    "ts-pageedit-create-title"
                },
                &[&d.title],
            ),
            doc_title: None,
            returnto: Some(d.title.clone()),
            tabs: tabs(m, &d.title, d.exists, TabName::Edit),
            body,
            ..Page::default()
        }
    })
}

/// A page asking a viewer who is not signed in to log in first.
fn login_needed(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| {
        let mut body = pages::message(MessageKind::Notice, &m.get("ts-pageedit-login"));
        if cx.features.has(crate::features::Feature::Login) {
            let _ = write!(
                body,
                "<p>{}</p>",
                link(
                    &format!(
                        "/w/index.php?title=Special:UserLogin&returnto={}",
                        title_url(title)
                    ),
                    &esc(&m.get("ts-login-title")),
                    ""
                )
            );
        }
        Page {
            title: title.to_string(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        }
    })
}

/// A fresh CSRF token, the page made the viewer's alone.
async fn token(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
) -> Result<(String, Vec<axum::http::HeaderValue>), ClientError> {
    let t = site.client().token(incoming, "csrf").await?;
    cx.inputs.push(t.value.cache);
    cx.inputs.push(CacheInfo::default());
    Ok((t.value.value, t.set_cookies))
}

/// The New page form: a title to create.
fn new_page(cx: &Context, headers: &HeaderMap) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| {
        let mut body = format!(
            "<p>{}</p><form class=\"ts-form\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"action\" value=\"edit\">",
            esc(&m.get("ts-newpage-intro"))
        );
        body.push_str(
            &Field {
                id: "ts-newpage-title",
                name: "title",
                kind: "text",
                value: "",
                label: &m.get("ts-newpage-label"),
                description: None,
                help: None,
                required: true,
                autocomplete: Some("off"),
            }
            .render()
            .unwrap_or_default(),
        );
        let _ = write!(
            body,
            "<div class=\"ts-form__actions\">{}</div></form>",
            Button {
                label: &m.get("ts-newpage-go"),
                kind: "submit",
                action: ButtonAction::Progressive,
                weight: Weight::Primary,
            }
            .render()
            .unwrap_or_default()
        );
        Page {
            title: m.get("ts-newpage-title"),
            body,
            ..Page::default()
        }
    })
}

/// The Edit tab.
async fn edit(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
) -> Response {
    if cx.user.is_none() {
        return login_needed(&cx, headers, &title);
    }
    let source = match site.client().page_source(incoming, &title).await {
        Ok(s) => s.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let draft = match source {
        Lookup::Found(s) => {
            if s.model != "wikitext" {
                return unsupported(&cx, headers, &s);
            }
            Draft::of_source(&s)
        }
        Lookup::Missing { title, .. } => Draft {
            title,
            ..Draft::default()
        },
        Lookup::Invalid => return bad_title(&cx, headers, &title),
    };
    let (token, cookies) = match token(site, &mut cx, incoming).await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    pages::with_cookies(
        edit_page(&cx, headers, &token, &draft, &[], None, None),
        &cookies,
    )
}

/// A page whose content model the form does not edit.
fn unsupported(cx: &Context, headers: &HeaderMap, s: &Source) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: s.title.clone(),
        returnto: Some(s.title.clone()),
        tabs: tabs(m, &s.title, true, TabName::Edit),
        body: pages::message(
            MessageKind::Notice,
            &m.with("ts-pageedit-unsupported", &[&s.model]),
        ),
        ..Page::default()
    })
}

/// `POST …&action=submit`: a preview, or a save.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: &str,
    form: &Params,
) -> Response {
    if cx.user.is_none() {
        return login_needed(&cx, headers, title);
    }
    let mut draft = Draft::of_form(title, form);
    let posted = form.map.get("wpEditToken").cloned().unwrap_or_default();
    // The page is the viewer's alone: it holds their token and their text.
    cx.inputs.push(CacheInfo::default());
    if form.map.contains_key("wpPreview") {
        return match site.client().preview(incoming, title, &draft.text).await {
            Ok(p) => edit_page(&cx, headers, &posted, &draft, &[], Some(&p), None),
            Err(ClientError::Api { info, .. }) => {
                let notices = [(MessageKind::Error, info)];
                edit_page(&cx, headers, &posted, &draft, &notices, None, None)
            }
            Err(e) => pages::api_failed(&cx, headers, &e),
        };
    }
    let saved = site
        .client()
        .save_page(
            incoming,
            &EditForm {
                title,
                text: &draft.text,
                summary: &draft.summary,
                minor: draft.minor,
                baserevid: draft.base,
                basetimestamp: draft.base_time.as_deref(),
                createonly: !draft.exists,
                token: &posted,
            },
        )
        .await;
    match saved {
        Ok(Saved::Saved { title, .. } | Saved::NoChange { title }) => {
            pages::see_other(&format!("/wiki/{}", title_url(&title)), &[])
        }
        Err(ClientError::Api { code, info }) => match code.as_str() {
            "editconflict" | "articleexists" => {
                // The current text, and the base moved to it: saving again replaces it
                // with what the viewer merged.
                let current = match site.client().page_source(incoming, title).await {
                    Ok(f) => f.value,
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                };
                let current_text = match current {
                    Lookup::Found(s) => {
                        draft.exists = true;
                        draft.base = Some(s.revid);
                        draft.base_time = Some(s.timestamp.clone());
                        draft.title = s.title;
                        Some(s.text)
                    }
                    _ => None,
                };
                let notices = [(MessageKind::Error, cx.m.get("ts-pageedit-conflict"))];
                edit_page(
                    &cx,
                    headers,
                    &posted,
                    &draft,
                    &notices,
                    None,
                    current_text.as_deref(),
                )
            }
            "badtoken" => {
                let (token, cookies) = match token(site, &mut cx, incoming).await {
                    Ok(t) => t,
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                };
                let notices = [(MessageKind::Warning, cx.m.get("ts-new-expired"))];
                pages::with_cookies(
                    edit_page(&cx, headers, &token, &draft, &notices, None, None),
                    &cookies,
                )
            }
            "permissiondenied" => {
                let notices = [(MessageKind::Error, cx.m.get("ts-permission-denied"))];
                edit_page(&cx, headers, &posted, &draft, &notices, None, None)
            }
            _ => {
                let notices = [(
                    MessageKind::Error,
                    cx.m.with("ts-pageedit-failed", &[&info]),
                )];
                edit_page(&cx, headers, &posted, &draft, &notices, None, None)
            }
        },
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

// ---- history ---------------------------------------------------------------------------

/// The History tab: the page's rows of the activity feed.
async fn page_history(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
    params: &Params,
) -> Response {
    let info = match site.client().page_info(incoming, &title).await {
        Ok(i) => i,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(info.cache);
    let info = match info.value {
        Lookup::Found(i) => i,
        Lookup::Missing { .. } => return missing(&cx, headers, title, false),
        Lookup::Invalid => return bad_title(&cx, headers, &title),
    };
    let ask = history::Ask::of(&params.map);
    let mut query = ask.query(cx.m.lang());
    query.target = Some(format!("page:{}", info.page_id));
    let rows = match site.client().activity(incoming, &query).await {
        Ok(r) => r,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(rows.cache);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: info.title.clone(),
        returnto: Some(info.title.clone()),
        tabs: tabs(m, &info.title, true, TabName::History),
        body: history::body(m, &cx.site, &info.title, &ask, None, &rows.value),
        ..Page::default()
    })
}

// ---- dispatch --------------------------------------------------------------------------

/// Serves a title that is neither an entity's nor a special page's, where the API serves
/// pages.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    title: String,
    params: &Params,
) -> Response {
    let ask = Ask::of(params);
    if title.is_empty() {
        return if ask == Ask::Edit {
            new_page(&cx, headers)
        } else {
            pages::redirect_to(&cx, &cx.site.mainpage.clone())
        };
    }
    if special::name_of(&title).is_some() {
        return pages::no_page(&cx, headers, title);
    }
    match ask {
        Ask::Read => read(site, cx, headers, incoming, title, params, false).await,
        Ask::Render => read(site, cx, headers, incoming, title, params, true).await,
        Ask::Edit => edit(site, cx, headers, incoming, title).await,
        Ask::History => page_history(site, cx, headers, incoming, title, params).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::PageCategory;

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn what_is_asked() {
        assert_eq!(Ask::of(&params(&[])), Ask::Read);
        assert_eq!(Ask::of(&params(&[("action", "edit")])), Ask::Edit);
        assert_eq!(Ask::of(&params(&[("action", "submit")])), Ask::Edit);
        assert_eq!(Ask::of(&params(&[("action", "history")])), Ask::History);
        assert_eq!(Ask::of(&params(&[("action", "render")])), Ask::Render);
    }

    #[test]
    fn tabs_of_a_page_and_a_missing_one() {
        let m = Messages::for_language("en");
        let t = tabs(&m, "Project:Home", true, TabName::History);
        assert_eq!(t.len(), 3);
        assert_eq!(t[1].href, "/wiki/Project:Home?action=edit");
        assert_eq!(t[1].label, m.get("ts-tab-edit"));
        assert!(t[2].current);
        let t = tabs(&m, "Project:New one", false, TabName::Read);
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].label, m.get("ts-tab-create"));
        assert_eq!(t[1].href, "/wiki/Project:New_one?action=edit");
    }

    #[test]
    fn categories_are_listed_but_not_hidden_ones() {
        let m = Messages::for_language("en");
        let p = Parsed {
            categories: vec![
                PageCategory {
                    name: "Policy pages".into(),
                    hidden: false,
                },
                PageCategory {
                    name: "Tracking".into(),
                    hidden: true,
                },
            ],
            ..Parsed::default()
        };
        let html = categories_html(&m, &p);
        assert!(html.contains("href=\"/wiki/Category:Policy_pages\">Policy pages</a>"));
        assert!(!html.contains("Tracking"));
        assert_eq!(categories_html(&m, &Parsed::default()), "");
    }

    #[test]
    fn the_form_round_trips_a_draft() {
        let m = Messages::for_language("en");
        let d = Draft {
            title: "Project:Sandbox".into(),
            exists: true,
            text: "<b>a</b> & b".into(),
            summary: "x\"y".into(),
            minor: true,
            base: Some(7),
            base_time: Some("2026-10-19T00:00:00Z".into()),
        };
        let html = edit_form(&m, "tok+\\", &d);
        assert!(html.contains("action=\"/w/index.php?title=Project:Sandbox&amp;action=submit\""));
        assert!(html.contains(">\n&lt;b&gt;a&lt;/b&gt; &amp; b</textarea>"));
        assert!(html.contains("name=\"baseRevId\" value=\"7\""));
        assert!(html.contains("value=\"x&#34;y\""));
        assert!(html.contains("name=\"wpMinoredit\" value=\"1\" checked"));
        assert!(html.contains("name=\"wpSave\""));
        assert!(html.contains("name=\"wpPreview\""));
        assert!(!html.contains("wpCreateonly"));
        let back = Draft::of_form(
            "Project:Sandbox",
            &params(&[
                ("wpTextbox1", "line\r\nline"),
                ("wpSummary", "s"),
                ("baseRevId", "7"),
                ("wpEdittime", "2026-10-19T00:00:00Z"),
            ]),
        );
        assert_eq!(back.text, "line\nline");
        assert_eq!(back.base, Some(7));
        assert!(back.exists);
        assert!(!back.minor);
        let new = Draft::of_form("Project:X", &params(&[("wpCreateonly", "1")]));
        assert!(!new.exists);
        assert_eq!(new.base, None);
    }
}
