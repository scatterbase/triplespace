//! `Special:UserRights` (09 §3.3, §4.5; 0016 §7; 19 §2.2): an account's groups, changed
//! by a holder of `userrights` or of a delegation (`adds`/`removes`, 09 §3.2).
//!
//! - **`Special:UserRights`** asks for a name and goes to the account's page.
//! - **`Special:UserRights/{name}`** lists every group the wiki has as a checkbox: on for
//!   a membership in force (its expiry beside it), enabled where the viewer may make the
//!   change `meta=userinfo`'s `changeablegroups` allows (adding where not a member,
//!   removing where one; the self lists for the viewer's own page), disabled otherwise,
//!   so the page shows what the viewer cannot do rather than hiding it. An expiry applies
//!   to every group added; a reason goes to the public `rights/rights` log event, which
//!   is the accountability of the model.
//! - **Adding oneself** is the accountable way into a confidential group (09 §4.5), so
//!   the viewer's own page proposes an expiry of a day, since a look at one page needs no
//!   standing membership; it may be cleared.
//!
//! The form posts the whole set; the page works out which memberships changed and sends
//! one `action=userrights` with the `userrights` token. Its refusal comes back on the
//! form (`permissiondenied` naming the group, `ts-last-owner`, `badvalue`, `badtoken`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{GroupInfo, Incoming, RightsForm, UserGroups};

use crate::Site;
use crate::codex::{Action, Button, Field, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};

const TITLE: &str = "Special:UserRights";

/// The name a title carries: `Special:UserRights/Alice`.
fn subject_of(title: &str) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The page asking for a name.
fn ask_page(cx: &Context, headers: &HeaderMap, notice: Option<(MessageKind, String)>) -> Response {
    let m = &cx.m;
    let mut body = String::new();
    if let Some((kind, text)) = notice {
        body.push_str(&pages::message(kind, &text));
    }
    let _ = write!(
        body,
        "<form class=\"ts-userrights__ask\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"title\" value=\"{TITLE}\">{}{}</form>",
        Field {
            id: "ts-userrights-user",
            name: "user",
            kind: "text",
            value: "",
            label: &m.get("ts-userrights-user"),
            description: None,
            help: Some(&m.get("ts-userrights-user-help")),
            required: true,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
        Button {
            label: &m.get("ts-userrights-go"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default()
    );
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-userrights-title"),
        returnto: Some(TITLE.to_string()),
        body: format!("<div class=\"ts-userrights\">{body}</div>"),
        ..Page::default()
    })
}

/// What the form shows when it comes back.
#[derive(Debug, Default)]
struct Returned {
    notice: Option<(MessageKind, String)>,
    expiry: Option<String>,
    reason: String,
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "permissiondenied" => m.with("ts-userrights-denied-group", &[info]),
        "ts-last-owner" => m.with("ts-userrights-last-owner", &[info]),
        "invalidexpiry" => m.get("ts-protect-invalid-expiry"),
        "badtoken" => m.get("ts-login-expired"),
        "nosuchuser" | "nosuchuserid" => m.get("ts-userrights-nosuchuser"),
        "badvalue" | "ts-unknown-group" => m.get("ts-protect-unknown-group"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// The group checkboxes and the fields under them.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one form, read top to bottom
fn form_html(
    m: &Messages,
    user: &UserGroups,
    groups: &[GroupInfo],
    changeable: &[Vec<String>; 4],
    own: bool,
    token: &str,
    back: &Returned,
) -> String {
    let [add, remove, add_self, remove_self] = changeable;
    let (may_add, may_remove) = if own {
        (add_self, remove_self)
    } else {
        (add, remove)
    };
    let current: BTreeMap<&str, &str> = user
        .memberships
        .iter()
        .map(|(g, e)| (g.as_str(), e.as_str()))
        .collect();
    let mut s = format!(
        "<form class=\"ts-userrights__groups\" method=\"post\" action=\"/w/index.php?title={}\"><input type=\"hidden\" name=\"token\" value=\"{}\"><fieldset class=\"ts-userrights__set\"><legend>{}</legend>",
        esc(&title_url(&format!("{TITLE}/{}", user.name))),
        esc(token),
        esc(&m.with("ts-userrights-groups-of", &[&user.name])),
    );
    let mut any_enabled = false;
    for g in groups.iter().filter(|g| !g.implicit && g.name != "*") {
        let member = current.contains_key(g.name.as_str());
        let enabled = if member {
            may_remove.contains(&g.name)
        } else {
            may_add.contains(&g.name)
        };
        any_enabled |= enabled;
        let id = format!("ts-ur-{}", g.name);
        let expiry = current
            .get(g.name.as_str())
            .filter(|e| **e != "infinity")
            .map(|e| {
                format!(
                    " <span class=\"ts-userrights__expiry\">{}</span>",
                    esc(&m.with("ts-userrights-until", &[e]))
                )
            })
            .unwrap_or_default();
        let _ = write!(
            s,
            "<div class=\"ts-userrights__group\"><input type=\"hidden\" name=\"was-{name}\" value=\"{was}\"><span class=\"cdx-checkbox\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"{id}\" name=\"group-{name}\" value=\"1\"{checked}{disabled}><span class=\"cdx-checkbox__icon\"></span><label class=\"cdx-checkbox__label\" for=\"{id}\">{label}{expiry}</label></span></div>",
            name = esc(&g.name),
            was = u8::from(member),
            checked = if member { " checked" } else { "" },
            disabled = if enabled { "" } else { " disabled" },
            label = esc(&g.name),
        );
    }
    s.push_str("</fieldset>");
    if !any_enabled {
        s.push_str(&pages::message(
            MessageKind::Notice,
            &m.get("ts-userrights-nothing-changeable"),
        ));
    }
    let expiry = back.expiry.clone().unwrap_or_else(|| {
        if own {
            m.get("ts-userrights-self-expiry")
        } else {
            "infinite".to_string()
        }
    });
    let _ = write!(
        s,
        "{}{}<div class=\"ts-userrights__actions\">{}</div></form>",
        Field {
            id: "ts-userrights-expiry",
            name: "expiry",
            kind: "text",
            value: &expiry,
            label: &m.get("ts-userrights-expiry"),
            description: None,
            help: Some(&m.get(if own {
                "ts-userrights-expiry-self-help"
            } else {
                "ts-userrights-expiry-help"
            })),
            required: false,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
        Field {
            id: "ts-userrights-reason",
            name: "reason",
            kind: "text",
            value: &back.reason,
            label: &m.get("ts-userrights-reason"),
            description: None,
            help: Some(&m.get("ts-userrights-reason-help")),
            required: false,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
        Button {
            label: &m.get("ts-userrights-save"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default(),
    );
    s
}

/// The account's page.
async fn user_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    name: &str,
    back: Returned,
) -> Response {
    let (user, groups, changeable, token) = tokio::join!(
        site.client().user_groups(incoming, name),
        site.client().groups(incoming),
        site.client().changeable_groups(incoming),
        site.client().token(incoming, "userrights")
    );
    let (user, groups, changeable, token) = match (user, groups, changeable, token) {
        (Ok(u), Ok(g), Ok(c), Ok(t)) => (u, g, c, t),
        (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
            return pages::api_failed(cx, headers, &e);
        }
    };
    cx.inputs.push(user.cache);
    cx.inputs.push(groups.cache);
    cx.inputs.push(changeable.cache);
    let Some(user) = user.value else {
        return ask_page(
            cx,
            headers,
            Some((MessageKind::Error, cx.m.get("ts-userrights-nosuchuser"))),
        );
    };
    let own = cx.user.as_deref() == Some(user.name.as_str());
    let m = &cx.m;
    let mut body = String::new();
    if let Some((kind, text)) = &back.notice {
        body.push_str(&pages::message(*kind, text));
    }
    if let Some(e) = &user.block_expiry {
        body.push_str(&pages::message(
            MessageKind::Warning,
            &m.with("ts-userrights-blocked", &[&user.name, e]),
        ));
    }
    body.push_str(&form_html(
        m,
        &user,
        &groups.value,
        &changeable.value,
        own,
        &token.value.value,
        &back,
    ));
    let _ = write!(
        body,
        "<p class=\"ts-userrights__links\">{}</p>",
        link(
            &format!("/wiki/Special:Contributions/{}", title_url(&user.name)),
            &esc(&m.get("ts-userrights-contributions")),
            ""
        ),
    );
    let heading = m.with("ts-userrights-title-of", &[&user.name]);
    let r = pages::respond(cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(format!("{TITLE}/{}", user.name)),
        body: format!("<div class=\"ts-userrights\">{body}</div>"),
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

/// The context, or the response that ends the request: a 404 where the API does not
/// serve the module, a login prompt for a reader.
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::UserRights) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    if cx.user.is_none() {
        let body = format!(
            "{}<p>{}</p>",
            pages::message(MessageKind::Notice, &cx.m.get("ts-userrights-login")),
            link(
                &format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE)),
                &esc(&cx.m.get("ts-login")),
                ""
            )
        );
        return Err(pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-userrights-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        }));
    }
    Ok(cx)
}

/// `GET Special:UserRights[/{name}]`.
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
    // The name form sends `user=`; go to the account's own address.
    if let Some(user) = params
        .map
        .get("user")
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
    {
        return pages::see_other(
            &format!("/wiki/{}", title_url(&format!("{TITLE}/{user}"))),
            &[],
        );
    }
    let Some(name) = subject_of(title) else {
        return ask_page(&cx, headers, None);
    };
    let notice = params
        .map
        .get("done")
        .map(|_| (MessageKind::Success, cx.m.get("ts-userrights-done")));
    let back = Returned {
        notice,
        ..Returned::default()
    };
    user_page(site, &mut cx, headers, &incoming, &name, back).await
}

/// `POST Special:UserRights/{name}`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let Some(name) = subject_of(title) else {
        return ask_page(&cx, headers, None);
    };
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for (k, v) in &form.map {
        if let Some(group) = k.strip_prefix("was-") {
            let was = v == "1";
            let now = form.map.contains_key(&format!("group-{group}"));
            if now && !was {
                add.push(group.to_string());
            } else if was && !now {
                remove.push(group.to_string());
            }
        }
    }
    let expiry = field("expiry").trim();
    let reason = field("reason").trim();
    if add.is_empty() && remove.is_empty() {
        let back = Returned {
            notice: Some((MessageKind::Notice, cx.m.get("ts-userrights-no-change"))),
            expiry: Some(expiry.to_string()),
            reason: reason.to_string(),
        };
        return user_page(site, &mut cx, headers, &incoming, &name, back).await;
    }
    let sent = RightsForm {
        user: &name,
        add,
        remove,
        expiry: if expiry.is_empty() {
            "infinite"
        } else {
            expiry
        },
        reason,
        token: field("token"),
    };
    match site.client().userrights(&incoming, &sent).await {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!("/wiki/{}?done=1", title_url(&format!("{TITLE}/{name}"))),
            &[],
        ),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            let back = Returned {
                notice: Some((MessageKind::Error, text)),
                expiry: Some(expiry.to_string()),
                reason: reason.to_string(),
            };
            user_page(site, &mut cx, headers, &incoming, &name, back).await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subject_is_the_subpage() {
        assert_eq!(
            subject_of("Special:UserRights/Alice_B").as_deref(),
            Some("Alice B")
        );
        assert_eq!(subject_of("Special:UserRights"), None);
        assert_eq!(subject_of("Special:UserRights/"), None);
    }
}
