//! Read-only pages about the wiki itself (0047 §9; 21 §6.1), each drawn from what the API
//! reports or the site knows:
//!
//! - `Special:NamespaceInfo`: the namespaces (`siprop=namespaces|namespacealiases`), with
//!   their names, other names and properties.
//! - `Special:ListGroupRights` (`UserGroupRights`): each group and its permissions
//!   (`siprop=usergroups`).
//! - `Special:ListDatatypes`: the data types the repository registers
//!   (`general.wikibase-propertytypes`), with their value types, in the order
//!   `Special:NewProperty` offers the same types.
//! - `Special:MyLanguageFallbackChain`: the languages a label, description or alias is
//!   looked up in for the viewer, in order (0009 §5).

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::Site;
use crate::frame::Page;
use crate::html::esc;
use crate::pages::{self, Context, Params, Peer};

/// The pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:NamespaceInfo`.
    NamespaceInfo,
    /// `Special:ListGroupRights`.
    ListGroupRights,
    /// `Special:ListDatatypes`.
    ListDatatypes,
    /// `Special:MyLanguageFallbackChain`.
    MyLanguageFallbackChain,
}

impl Which {
    /// The page a special page's name (or a registry alias) names.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        Some(match name {
            "NamespaceInfo" => Self::NamespaceInfo,
            "ListGroupRights" | "UserGroupRights" => Self::ListGroupRights,
            "ListDatatypes" => Self::ListDatatypes,
            "MyLanguageFallbackChain" => Self::MyLanguageFallbackChain,
            _ => return None,
        })
    }

    fn title(self) -> &'static str {
        match self {
            Self::NamespaceInfo => "Special:NamespaceInfo",
            Self::ListGroupRights => "Special:ListGroupRights",
            Self::ListDatatypes => "Special:ListDatatypes",
            Self::MyLanguageFallbackChain => "Special:MyLanguageFallbackChain",
        }
    }

    fn title_key(self) -> &'static str {
        match self {
            Self::NamespaceInfo => "ts-namespaceinfo-title",
            Self::ListGroupRights => "ts-listgrouprights-title",
            Self::ListDatatypes => "ts-listdatatypes-title",
            Self::MyLanguageFallbackChain => "ts-fallbackchain-title",
        }
    }
}

/// `GET Special:{which}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let body = match which {
        Which::NamespaceInfo => match site.client().namespaces(&incoming).await {
            Ok(found) => {
                cx.inputs.push(found.cache);
                namespaces_html(&cx, &found.value)
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        },
        Which::ListGroupRights => match site.client().groups(&incoming).await {
            Ok(found) => {
                cx.inputs.push(found.cache);
                groups_html(&cx, &found.value)
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        },
        Which::ListDatatypes => datatypes_html(&cx),
        Which::MyLanguageFallbackChain => chain_html(&cx),
    };
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get(which.title_key()),
        returnto: Some(which.title().to_string()),
        body,
        ..Page::default()
    })
}

/// A table with a header row; each row's cells are HTML already.
fn table(class: &str, heads: &[String], rows: &[Vec<String>]) -> String {
    let mut s = format!("<table class=\"cdx-table__table {class}\"><thead><tr>");
    for h in heads {
        let _ = write!(s, "<th scope=\"col\">{}</th>", esc(h));
    }
    s.push_str("</tr></thead><tbody>");
    for row in rows {
        s.push_str("<tr>");
        for cell in row {
            let _ = write!(s, "<td>{cell}</td>");
        }
        s.push_str("</tr>");
    }
    s.push_str("</tbody></table>");
    s
}

/// A list; each item is HTML already.
fn bullets(class: &str, items: impl Iterator<Item = String>) -> String {
    let mut s = format!("<ul class=\"{class}\">");
    for item in items {
        let _ = write!(s, "<li>{item}</li>");
    }
    s.push_str("</ul>");
    s
}

fn empty(cx: &Context, key: &str) -> String {
    format!("<p class=\"ts-empty\">{}</p>", esc(&cx.m.get(key)))
}

fn namespaces_html(cx: &Context, rows: &[triplespace_client::NamespaceRow]) -> String {
    let m = &cx.m;
    if rows.is_empty() {
        return empty(cx, "ts-namespaceinfo-none");
    }
    let heads = [
        m.get("ts-namespaceinfo-col-id"),
        m.get("ts-namespaceinfo-col-name"),
        m.get("ts-namespaceinfo-col-canonical"),
        m.get("ts-namespaceinfo-col-aliases"),
        m.get("ts-namespaceinfo-col-notes"),
    ];
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|n| {
            let name = if n.id == 0 {
                format!("<i>{}</i>", esc(&m.get("ts-namespaceinfo-main")))
            } else {
                esc(&n.name)
            };
            let canonical = n
                .canonical
                .as_deref()
                .filter(|c| *c != n.name)
                .map(esc)
                .unwrap_or_default();
            let mut notes = Vec::new();
            if n.content {
                notes.push(m.get("ts-namespaceinfo-content"));
            }
            if n.subpages {
                notes.push(m.get("ts-namespaceinfo-subpages"));
            }
            if n.case == "case-sensitive" {
                notes.push(m.get("ts-namespaceinfo-case-sensitive"));
            }
            if let Some(model) = &n.model {
                notes.push(m.with("ts-namespaceinfo-model", &[model]));
            }
            let notes = if notes.is_empty() {
                String::new()
            } else {
                bullets("ts-namespaceinfo__notes", notes.iter().map(|t| esc(t)))
            };
            vec![
                n.id.to_string(),
                name,
                canonical,
                n.aliases
                    .iter()
                    .map(|a| esc(a))
                    .collect::<Vec<_>>()
                    .join(", "),
                notes,
            ]
        })
        .collect();
    format!(
        "<p>{}</p>{}",
        esc(&m.get("ts-namespaceinfo-intro")),
        table("ts-namespaceinfo", &heads, &cells)
    )
}

fn groups_html(cx: &Context, groups: &[triplespace_client::rights::GroupInfo]) -> String {
    let m = &cx.m;
    if groups.is_empty() {
        return empty(cx, "ts-listgrouprights-none");
    }
    let heads = [
        m.get("ts-listgrouprights-col-group"),
        m.get("ts-listgrouprights-col-rights"),
    ];
    let cells: Vec<Vec<String>> = groups
        .iter()
        .map(|g| {
            let mut name = format!("<code>{}</code>", esc(&g.name));
            if g.name == "*" {
                let _ = write!(name, " {}", esc(&m.get("ts-listgrouprights-everyone")));
            } else if g.implicit {
                let _ = write!(name, " {}", esc(&m.get("ts-listgrouprights-implicit")));
            }
            let mut rights = g.rights.clone();
            rights.sort();
            let rights = if rights.is_empty() {
                format!("<i>{}</i>", esc(&m.get("ts-listgrouprights-no-rights")))
            } else {
                bullets(
                    "ts-listgrouprights__rights",
                    rights.iter().map(|r| format!("<code>{}</code>", esc(r))),
                )
            };
            vec![name, rights]
        })
        .collect();
    format!(
        "<p>{}</p>{}",
        esc(&m.get("ts-listgrouprights-intro")),
        table("ts-listgrouprights", &heads, &cells)
    )
}

fn datatypes_html(cx: &Context) -> String {
    let m = &cx.m;
    let types = &cx.site.property_types;
    if types.is_empty() {
        return empty(cx, "ts-listdatatypes-none");
    }
    let heads = [
        m.get("ts-listdatatypes-col-name"),
        m.get("ts-listdatatypes-col-id"),
        m.get("ts-listdatatypes-col-valuetype"),
    ];
    // The order `NewProperty` offers them in.
    let cells: Vec<Vec<String>> = super::new_entity::offered(&cx.site)
        .iter()
        .filter_map(|id| types.iter().find(|(t, _)| t == id))
        .map(|(id, vt)| {
            vec![
                esc(&super::new_entity::datatype_name(m, id)),
                format!("<code>{}</code>", esc(id)),
                format!("<code>{}</code>", esc(vt)),
            ]
        })
        .collect();
    format!(
        "<p>{}</p>{}",
        esc(&m.get("ts-listdatatypes-intro")),
        table("ts-listdatatypes", &heads, &cells)
    )
}

fn chain_html(cx: &Context) -> String {
    let m = &cx.m;
    let mut items = String::new();
    for lang in m.term_chain() {
        let note = if lang == "mul" {
            format!(" – {}", esc(&m.get("ts-fallbackchain-mul")))
        } else {
            String::new()
        };
        let _ = write!(
            items,
            "<li><code lang=\"{0}\">{0}</code>{note}</li>",
            esc(&lang)
        );
    }
    format!(
        "<p>{}</p><ol class=\"ts-fallbackchain\">{items}</ol><p>{}</p>",
        esc(&m.get("ts-fallbackchain-intro")),
        esc(&m.get("ts-fallbackchain-uselang"))
    )
}
