//! `Special:MovePage/{title}` (0047 §9; 19 §6.3): the Move form, for a holder of `move`.
//! It asks for the new title and a reason, offers to leave no redirect to a holder of
//! `suppressredirect`, and to carry subpages along to a holder of `move-subpages`; the
//! API checks the rest at both ends (10 §2.5), and its refusal is shown over the form
//! with what was typed. A move that succeeds lands on the page at its new title.

use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::Incoming;
use triplespace_client::rights::MoveForm;
use triplespace_client::settings::Saved;

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::moderation::{gate, reason_field};
use crate::pages::{self, Context, Params, Peer};

const TITLE: &str = "Special:MovePage";

/// What the form holds, typed or defaulted.
struct Typed {
    to: String,
    reason: String,
    redirect: bool,
    subpages: bool,
}

/// The page a request names: `Special:MovePage/{title}` or `?target=`.
fn subject(title: &str, params: &Params) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| params.map.get("target").map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty())
}

fn checkbox(m: &Messages, id: &str, name: &str, label: &str, help: &str, checked: bool) -> String {
    format!(
        "<div class=\"ts-protect__talk\"><span class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"{id}\" name=\"{name}\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><label class=\"cdx-checkbox__label\" for=\"{id}\">{}</label></span><div class=\"cdx-field__help-text\">{}</div></div>",
        if checked { " checked" } else { "" },
        esc(&m.get(label)),
        esc(&m.get(help)),
    )
}

/// The form.
async fn form(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    from: &str,
    notice: Option<(MessageKind, String)>,
    typed: Option<Typed>,
) -> Response {
    if !cx.features.has(Feature::Move) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let heading = cx.m.with("ts-move-title", &[from]);
    if let Some(r) = gate(&cx, headers, from, &heading, "move", "move") {
        return r;
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let m = &cx.m;
    let typed = typed.unwrap_or(Typed {
        to: from.to_string(),
        reason: String::new(),
        redirect: true,
        subpages: false,
    });
    let mut body = String::new();
    if let Some((kind, text)) = notice {
        body.push_str(&pages::message(kind, &text));
    }
    body.push_str(&pages::message(
        MessageKind::Notice,
        &m.get("ts-move-intro"),
    ));
    let mut options = String::new();
    if cx.holds("suppressredirect") {
        options.push_str(&checkbox(
            m,
            "ts-move-redirect",
            "redirect",
            "ts-move-redirect",
            "ts-move-redirect-help",
            typed.redirect,
        ));
    }
    if cx.holds("move-subpages") {
        options.push_str(&checkbox(
            m,
            "ts-move-subpages",
            "subpages",
            "ts-move-subpages",
            "ts-move-subpages-help",
            typed.subpages,
        ));
    }
    let _ = write!(
        body,
        "<form class=\"ts-move\" method=\"post\" action=\"/wiki/{}/{}\"><input type=\"hidden\" name=\"token\" value=\"{}\"><div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-move-to\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"ts-move-to\" name=\"to\" value=\"{}\" required></div></div></div>{}{}<div class=\"ts-protect__actions\">{}{}</div></form>",
        TITLE,
        esc(&title_url(from)),
        esc(&token.value.value),
        esc(&m.get("ts-move-to")),
        esc(&typed.to),
        reason_field(m, "ts-move-reason", &typed.reason),
        options,
        Button {
            label: &m.get("ts-move-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default(),
        link(
            &format!("/wiki/{}", title_url(from)),
            &esc(&m.get("ts-protect-cancel")),
            " class=\"ts-protect__cancel\""
        ),
    );
    let r = pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(from.to_string()),
        body: format!("<div class=\"ts-protect-page\">{body}</div>"),
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

/// `GET Special:MovePage/{title}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let Some(from) = subject(title, params) else {
        // No page named: ask for one.
        let m = &cx.m;
        let body = format!(
            "<form class=\"ts-undelete__ask\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"title\" value=\"{TITLE}\"><label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"target\"></label> <button class=\"cdx-button cdx-button--action-progressive\" type=\"submit\">{}</button></form>",
            esc(&m.get("ts-move-which")),
            esc(&m.get("ts-undelete-go")),
        );
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-move-index-title"),
            returnto: Some(TITLE.to_string()),
            body: format!("<div class=\"ts-changes\">{body}</div>"),
            ..Page::default()
        });
    };
    form(site, cx, headers, &incoming, &from, None, None).await
}

fn refusal(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "missingtitle" | "nosuchpageid" => m.get("ts-protect-missing"),
        "articleexists" => m.get("ts-move-exists"),
        "ts-restricted-title" => m.get("ts-move-restricted"),
        "immobilenamespace" | "ts-model-not-allowed" => m.get("ts-move-immobile"),
        "selfmove" => m.get("ts-move-self"),
        "invalidtitle" => m.get("ts-move-invalid"),
        "permissiondenied" | "protectedpage" => m.with("ts-moderation-denied", &["move"]),
        "badtoken" => m.get("ts-login-expired"),
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `POST Special:MovePage/{title}`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Move) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let Some(from) = subject(title, params) else {
        return pages::no_page(&cx, headers, TITLE.to_string());
    };
    let field = |k: &str| params.map.get(k).map_or("", String::as_str);
    let typed = Typed {
        to: field("to").trim().to_string(),
        reason: field("reason").trim().to_string(),
        // The checkbox is offered only to a holder of `suppressredirect`; for anyone else
        // a redirect is always left, so an absent field means one.
        redirect: !cx.holds("suppressredirect") || params.map.contains_key("redirect"),
        subpages: params.map.contains_key("subpages"),
    };
    match site
        .client()
        .move_page(
            &incoming,
            &MoveForm {
                from: &from,
                to: &typed.to,
                reason: &typed.reason,
                noredirect: !typed.redirect,
                subpages: typed.subpages,
            },
            field("token"),
        )
        .await
    {
        Ok(Saved::Ok(v)) => {
            let to = v["to"].as_str().unwrap_or(&typed.to).to_string();
            pages::see_other(&format!("/wiki/{}", title_url(&to)), &[])
        }
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            form(
                site,
                cx,
                headers,
                &incoming,
                &from,
                Some((MessageKind::Error, text)),
                Some(typed),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
