//! `Special:ProtectedPages` (09 §4.5; 0016 §7; 0023 §9; 0056 §13; 19 §6.3): the
//! restrictions in force, each only to a viewer who may read its target, so a
//! confidential page is listed to its group alone.
//!
//! - **Pages**: every page and entity restriction (`list=protectedpages`, `read`
//!   included, with a **Read** column), and the reserved titles (`list=protectedtitles`).
//! - **Sets**: the sets the viewer may read (`list=protectedsets`), each with its
//!   restrictions and members, and for a holder of `protect` the forms that manage them:
//!   create a set, rename it or change what it restricts, add a member by title, remove
//!   one, retire the set.
//! - **Empty groups**: for a holder of `userrights`, each group a confidential restriction
//!   in force names that has no current member, with how many restrictions name it and no
//!   titles (`GET /acl/empty-groups`), so an orphaned group is found before anyone needs
//!   it.
//!
//! Every form posts back here; the API's refusal is shown above the section it came from.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use serde_json::json;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, Lookup, ProtectedRow, SetInfo};

use crate::Site;
use crate::codex::{Action, Button, Field, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};

const TITLE: &str = "Special:ProtectedPages";

fn expiry_text(m: &Messages, expiry: &str) -> String {
    if expiry == "infinity" || expiry == "infinite" {
        m.get("ts-protect-expiry-never")
    } else {
        expiry.to_string()
    }
}

fn kind_label(m: &Messages, kind: &str) -> String {
    let key = format!("ts-protect-action-{kind}");
    if m.has(&key) {
        m.get(&key)
    } else {
        kind.to_string()
    }
}

/// The pages table: one row per restriction.
fn pages_html(m: &Messages, pages: &[ProtectedRow], titles: &[ProtectedRow]) -> String {
    let mut s = format!(
        "<section id=\"pages\"><h2>{}</h2>",
        esc(&m.get("ts-protectedpages-pages"))
    );
    if pages.is_empty() && titles.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p></section>",
            esc(&m.get("ts-protectedpages-none"))
        );
        return s;
    }
    let _ = write!(
        s,
        "<table class=\"cdx-table__table ts-protectedpages__table\"><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
        esc(&m.get("ts-protectedpages-col-title")),
        esc(&m.get("ts-protectedpages-col-action")),
        esc(&m.get("ts-protectedpages-col-group")),
        esc(&m.get("ts-protectedpages-col-expiry")),
    );
    for r in pages.iter().chain(titles) {
        let kind = if r.kind == "create" || r.kind == "createpage" {
            "create"
        } else {
            r.kind.as_str()
        };
        let _ = write!(
            s,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            link(
                &format!("/w/index.php?title={}&action=protect", title_url(&r.title)),
                &esc(&r.title),
                ""
            ),
            esc(&kind_label(m, kind)),
            esc(&r.level),
            esc(&expiry_text(m, &r.expiry)),
        );
    }
    s.push_str("</tbody></table></section>");
    s
}

fn hidden(name: &str, value: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
        esc(name),
        esc(value)
    )
}

fn button(label: &str, action: Action, weight: Weight) -> String {
    Button {
        label,
        kind: "submit",
        action,
        weight,
    }
    .render()
    .unwrap_or_default()
}

/// A group picker for a set's restriction on `action`.
fn picker(m: &Messages, id: &str, action: &str, groups: &[String], current: &str) -> String {
    let value = serde_json::Value::String(current.to_string());
    let control = crate::special::controls::render(&crate::special::controls::Control {
        id,
        name: action,
        kind: crate::special::controls::Kind::Choice,
        choices: groups,
        value: Some(&value),
        empty_choice: Some(&m.get("ts-protect-level-all")),
        described_by: None,
        choice_label: None,
    });
    format!(
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"{id}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\">{control}</div></div>",
        esc(&kind_label(m, action)),
    )
}

/// One set: its restrictions, members, and its forms for a holder of `protect`.
#[allow(clippy::too_many_lines)] // one section of markup, read top to bottom
fn set_html(m: &Messages, set: &SetInfo, groups: &[String], token: Option<&str>) -> String {
    let id = set.id;
    let name = if set.name.is_empty() {
        m.with("ts-protectedpages-set-unnamed", &[&id.to_string()])
    } else {
        set.name.clone()
    };
    let mut s = format!(
        "<section class=\"ts-set\" id=\"set-{id}\"><h3>{}</h3><p class=\"ts-set__restrictions\">{}</p>",
        esc(&name),
        esc(&set
            .protection
            .iter()
            .map(|(k, g, e)| {
                m.with(
                    "ts-protectedpages-set-restriction",
                    &[&kind_label(m, k), g, &expiry_text(m, e)],
                )
            })
            .collect::<Vec<_>>()
            .join(" · ")),
    );
    if set.members.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-protectedpages-set-no-members"))
        );
    } else {
        s.push_str("<ul class=\"ts-set__members\">");
        for (page_id, _, title) in &set.members {
            let _ = write!(
                s,
                "<li>{}",
                link(&format!("/wiki/{}", title_url(title)), &esc(title), "")
            );
            if let Some(t) = token {
                let _ = write!(
                    s,
                    " <form class=\"ts-set__remove\" method=\"post\" action=\"/wiki/{}\">{}{}{}{}{}</form>",
                    esc(&title_url(TITLE)),
                    hidden("token", t),
                    hidden("do", "remove-member"),
                    hidden("set", &id.to_string()),
                    hidden("page", &page_id.to_string()),
                    button(
                        &m.get("ts-protectedpages-remove-member"),
                        Action::Destructive,
                        Weight::Quiet
                    ),
                );
            }
            s.push_str("</li>");
        }
        s.push_str("</ul>");
    }
    if let Some(t) = token {
        let _ = write!(
            s,
            "<form class=\"ts-set__add\" method=\"post\" action=\"/wiki/{}\">{}{}{}{}{}</form>",
            esc(&title_url(TITLE)),
            hidden("token", t),
            hidden("do", "add-member"),
            hidden("set", &id.to_string()),
            Field {
                id: &format!("ts-set-{id}-title"),
                name: "member",
                kind: "text",
                value: "",
                label: &m.get("ts-protectedpages-add-member"),
                description: None,
                help: Some(&m.get("ts-protectedpages-add-member-help")),
                required: true,
                autocomplete: Some("off"),
            }
            .render()
            .unwrap_or_default(),
            button(
                &m.get("ts-protectedpages-add"),
                Action::Progressive,
                Weight::Normal
            ),
        );
        let level = |action: &str| -> String {
            set.protection
                .iter()
                .find(|(k, _, _)| k == action)
                .map(|(_, g, _)| g.clone())
                .unwrap_or_default()
        };
        let _ = write!(
            s,
            "<form class=\"ts-set__change\" method=\"post\" action=\"/wiki/{}\"><h4>{}</h4>{}{}{}{}{}{}{}</form>",
            esc(&title_url(TITLE)),
            esc(&m.get("ts-protectedpages-change-set")),
            hidden("token", t),
            hidden("do", "change"),
            hidden("set", &id.to_string()),
            Field {
                id: &format!("ts-set-{id}-name"),
                name: "name",
                kind: "text",
                value: &set.name,
                label: &m.get("ts-protectedpages-set-name"),
                description: None,
                help: None,
                required: false,
                autocomplete: Some("off"),
            }
            .render()
            .unwrap_or_default(),
            picker(
                m,
                &format!("ts-set-{id}-read"),
                "read",
                groups,
                &level("read")
            ),
            picker(
                m,
                &format!("ts-set-{id}-edit"),
                "edit",
                groups,
                &level("edit")
            ),
            button(
                &m.get("ts-protectedpages-change"),
                Action::Progressive,
                Weight::Normal
            ),
        );
        let _ = write!(
            s,
            "<form class=\"ts-set__retire\" method=\"post\" action=\"/wiki/{}\">{}{}{}{}</form>",
            esc(&title_url(TITLE)),
            hidden("token", t),
            hidden("do", "retire"),
            hidden("set", &id.to_string()),
            button(
                &m.get("ts-protectedpages-retire"),
                Action::Destructive,
                Weight::Normal
            ),
        );
    }
    s.push_str("</section>");
    s
}

/// The Sets tab.
fn sets_html(m: &Messages, sets: &[SetInfo], groups: &[String], token: Option<&str>) -> String {
    let mut s = format!(
        "<section id=\"sets\"><h2>{}</h2><p>{}</p>",
        esc(&m.get("ts-protectedpages-sets")),
        esc(&m.get("ts-protectedpages-sets-help"))
    );
    if sets.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-protectedpages-no-sets"))
        );
    }
    for set in sets {
        s.push_str(&set_html(m, set, groups, token));
    }
    if let Some(t) = token {
        let read = crate::special::controls::render(&crate::special::controls::Control {
            id: "ts-set-new-read",
            name: "read",
            kind: crate::special::controls::Kind::Choice,
            choices: groups,
            value: None,
            empty_choice: Some(&m.get("ts-protect-level-all")),
            described_by: None,
            choice_label: None,
        });
        let edit = crate::special::controls::render(&crate::special::controls::Control {
            id: "ts-set-new-edit",
            name: "edit",
            kind: crate::special::controls::Kind::Choice,
            choices: groups,
            value: None,
            empty_choice: Some(&m.get("ts-protect-level-all")),
            described_by: None,
            choice_label: None,
        });
        let _ = write!(
            s,
            "<form class=\"ts-set__new\" method=\"post\" action=\"/wiki/{}\"><h3>{}</h3>{}{}{}<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-set-new-read\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\">{read}</div><div class=\"cdx-field__help-text\">{}</div></div><div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-set-new-edit\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\">{edit}</div></div>{}</form>",
            esc(&title_url(TITLE)),
            esc(&m.get("ts-protectedpages-new-set")),
            hidden("token", t),
            hidden("do", "create"),
            Field {
                id: "ts-set-new-name",
                name: "name",
                kind: "text",
                value: "",
                label: &m.get("ts-protectedpages-set-name"),
                description: None,
                help: None,
                required: true,
                autocomplete: Some("off"),
            }
            .render()
            .unwrap_or_default(),
            esc(&m.get("ts-protect-action-read")),
            esc(&m.get("ts-protectedpages-set-read-help")),
            esc(&m.get("ts-protect-action-edit")),
            button(
                &m.get("ts-protectedpages-create"),
                Action::Progressive,
                Weight::Primary
            ),
        );
    }
    s.push_str("</section>");
    s
}

/// The empty-group report.
fn empty_groups_html(m: &Messages, groups: &[triplespace_client::EmptyGroup]) -> String {
    let mut s = format!(
        "<section id=\"empty-groups\"><h2>{}</h2><p>{}</p>",
        esc(&m.get("ts-protectedpages-empty-groups")),
        esc(&m.get("ts-protectedpages-empty-groups-help"))
    );
    if groups.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-protectedpages-empty-groups-none"))
        );
    } else {
        s.push_str("<ul>");
        for g in groups {
            let _ = write!(
                s,
                "<li>{}</li>",
                esc(&m.with(
                    "ts-protectedpages-empty-group",
                    &[&g.group, &g.restrictions.to_string()]
                ))
            );
        }
        s.push_str("</ul>");
    }
    s.push_str("</section>");
    s
}

/// The page.
async fn render(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    notice: Option<(MessageKind, String)>,
) -> Response {
    let may_manage = cx.holds("protect") && cx.user.is_some();
    let (pages, sets, empty, groups, token) = tokio::join!(
        site.client().protected_pages(incoming),
        site.client().protected_sets(incoming),
        async {
            if cx.holds("userrights") {
                site.client().empty_groups(incoming).await
            } else {
                Ok(None)
            }
        },
        site.client().groups(incoming),
        async {
            if may_manage {
                site.client().token(incoming, "csrf").await.map(Some)
            } else {
                Ok(None)
            }
        }
    );
    let (pages, sets, empty, groups, token) = match (pages, sets, empty, groups, token) {
        (Ok(pages), Ok(sets), Ok(empty), Ok(groups), Ok(token)) => {
            (pages, sets, empty, groups, token)
        }
        (Err(e), ..)
        | (_, Err(e), ..)
        | (_, _, Err(e), ..)
        | (_, _, _, Err(e), _)
        | (.., Err(e)) => {
            return pages::api_failed(cx, headers, &e);
        }
    };
    cx.inputs.push(pages.cache);
    cx.inputs.push(sets.cache);
    cx.inputs.push(groups.cache);
    let group_names: Vec<String> = groups
        .value
        .into_iter()
        .filter(|g| !g.implicit && g.name != "*")
        .map(|g| g.name)
        .collect();
    let m = &cx.m;
    let mut body = String::new();
    if let Some((kind, text)) = notice {
        body.push_str(&pages::message(kind, &text));
    }
    body.push_str(&pages_html(m, &pages.value.0, &pages.value.1));
    body.push_str(&sets_html(
        m,
        &sets.value,
        &group_names,
        token.as_ref().map(|t| t.value.value.as_str()),
    ));
    let mut tabs = vec![
        Tab {
            label: m.get("ts-protectedpages-pages"),
            href: "#pages".into(),
            current: false,
        },
        Tab {
            label: m.get("ts-protectedpages-sets"),
            href: "#sets".into(),
            current: false,
        },
    ];
    if let Some(groups) = &empty {
        body.push_str(&empty_groups_html(m, groups));
        tabs.push(Tab {
            label: m.get("ts-protectedpages-empty-groups"),
            href: "#empty-groups".into(),
            current: false,
        });
    }
    let r = pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-protectedpages-title"),
        returnto: Some(TITLE.to_string()),
        tabs,
        body: format!("<div class=\"ts-protectedpages\">{body}</div>"),
        ..Page::default()
    });
    match token {
        Some(t) => pages::with_cookies(r, &t.set_cookies),
        None => r,
    }
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Protect) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    Ok(cx)
}

/// `GET Special:ProtectedPages`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let notice = params.map.get("done").map(|d| {
        (
            MessageKind::Success,
            cx.m.get(match d.as_str() {
                "created" => "ts-protectedpages-done-created",
                "changed" => "ts-protectedpages-done-changed",
                "added" => "ts-protectedpages-done-added",
                "removed" => "ts-protectedpages-done-removed",
                "retired" => "ts-protectedpages-done-retired",
                _ => "ts-protectedpages-done",
            }),
        )
    });
    render(site, &mut cx, headers, &incoming, notice).await
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        "ts-policy" => m.get("ts-protect-policy"),
        "ts-unknown-group" => m.get("ts-protect-unknown-group"),
        "ts-no-target" | "missingtitle" => m.get("ts-protectedpages-no-page"),
        "ts-already-member" => m.get("ts-protectedpages-already-member"),
        "badbody" => m.get("ts-protectedpages-set-needs-restriction"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" => m.get("ts-protect-denied"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `POST Special:ProtectedPages`: one of the Sets tab's forms.
#[allow(clippy::too_many_lines)] // one match over the forms, read top to bottom
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
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let token = field("token");
    let set = field("set");
    let (done, result) = match field("do") {
        "create" => {
            let mut restrictions = serde_json::Map::new();
            for action in ["read", "edit"] {
                let g = field(action).trim();
                if !g.is_empty() {
                    restrictions.insert(action.into(), json!({"group": g}));
                }
            }
            let body = json!({
                "name": field("name").trim(),
                "restrictions": restrictions,
                "members": [],
                "token": token,
            });
            (
                "created",
                site.client()
                    .set_write(&incoming, Method::POST, "", &body)
                    .await,
            )
        }
        "change" => {
            let mut restrictions = serde_json::Map::new();
            for action in ["read", "edit"] {
                let g = field(action).trim();
                if !g.is_empty() {
                    restrictions.insert(action.into(), json!({"group": g}));
                }
            }
            let body = json!({
                "name": field("name").trim(),
                "restrictions": restrictions,
                "token": token,
            });
            (
                "changed",
                site.client()
                    .set_write(&incoming, Method::PUT, &format!("/{set}"), &body)
                    .await,
            )
        }
        "add-member" => {
            // The member is named by title; the API takes its page ID.
            let title = field("member").trim();
            let info = match site.client().page_info(&incoming, title).await {
                Ok(i) => i,
                Err(e) => return pages::api_failed(&cx, headers, &e),
            };
            let Lookup::Found(p) = info.value else {
                let notice = Some((MessageKind::Error, cx.m.get("ts-protectedpages-no-page")));
                return render(site, &mut cx, headers, &incoming, notice).await;
            };
            (
                "added",
                site.client()
                    .set_write(
                        &incoming,
                        Method::POST,
                        &format!("/{set}/members"),
                        &json!({"kind": "page", "id": p.page_id, "token": token}),
                    )
                    .await,
            )
        }
        "remove-member" => (
            "removed",
            site.client()
                .set_write(
                    &incoming,
                    Method::DELETE,
                    &format!("/{set}/members/page/{}", field("page")),
                    &json!({"token": token}),
                )
                .await,
        ),
        "retire" => (
            "retired",
            site.client()
                .set_write(
                    &incoming,
                    Method::DELETE,
                    &format!("/{set}"),
                    &json!({"token": token}),
                )
                .await,
        ),
        _ => return render(site, &mut cx, headers, &incoming, None).await,
    };
    match result {
        Ok(Saved::Ok(_)) => {
            pages::see_other(&format!("/wiki/{}?done={done}", title_url(TITLE)), &[])
        }
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            render(
                site,
                &mut cx,
                headers,
                &incoming,
                Some((MessageKind::Error, text)),
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
    fn a_set_lists_its_members_and_offers_forms_to_a_manager() {
        let m = Messages::for_language("en");
        let set = SetInfo {
            id: 4,
            name: "Board docs".into(),
            protection: vec![("read".into(), "board".into(), "infinity".into())],
            members: vec![(9, 4, "Project:Minutes".into())],
        };
        let groups = vec!["board".to_string(), "sysop".to_string()];
        let reader = set_html(&m, &set, &groups, None);
        assert!(reader.contains("Board docs"));
        assert!(reader.contains("/wiki/Project:Minutes"));
        assert!(!reader.contains("<form"));
        let manager = set_html(&m, &set, &groups, Some("t"));
        assert!(manager.contains("name=\"do\" value=\"change\""));
        assert!(manager.contains("<option value=\"board\" selected>"));
        assert!(manager.contains("name=\"do\" value=\"remove-member\""));
        assert!(manager.contains("name=\"page\" value=\"9\""));
        assert!(manager.contains("name=\"do\" value=\"retire\""));
    }
}
