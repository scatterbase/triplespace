//! `Special:Version` (ADR 0077): the credit line and licence notice, installed software and
//! services, features, entry points, components, wikitext, the extensions that inspired
//! features and the AI agents used, all from `GET /version`; and its subpages:
//!
//! - `Special:Version/Credits`: every contributor and funder, and the AI agents (0077 §2).
//! - `Special:Version/License`: the GNU GPL, verbatim and in English only, from the
//!   `LICENSE` this build embeds (0077 §3).
//! - `Special:Version/License/{component}`: a component's licence files, from
//!   `GET /version/licenses/{id}`.
//!
//! The page names no ADRs (0077 A1).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;
use triplespace_client::version::{Build, Component, FeatureSwitch, Version};

use crate::Site;
use crate::codex::{Accordion, Cell, Column, MessageKind, Table};
use crate::features::Feature;
use crate::frame::Page;
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

/// The GNU General Public License, version 3, as the repository carries it: every build
/// serves its own copy (0077 §3).
pub const GPL: &str = include_str!("../../../../LICENSE");

/// How many contributors the credit line names before "and others" (0077 §2).
pub const CREDIT_LINE_NAMES: usize = 10;

/// A lineage list longer than this opens on demand.
pub const LONG_LIST: usize = 20;

/// The private-use characters a message's HTML parameters stand in as while the message
/// is escaped.
const SLOTS: [&str; 4] = ["\u{E000}", "\u{E001}", "\u{E002}", "\u{E003}"];

/// A message with HTML parameters: the message is escaped, the parameters are not.
fn rich(m: &Messages, key: &str, params: &[&str]) -> String {
    let mut s = esc(&m.with(key, &SLOTS[..params.len()]));
    for (slot, p) in SLOTS.iter().zip(params) {
        s = s.replace(slot, p);
    }
    s
}

/// A list in the reader's language: `A`, `A and B`, `A, B and C`.
fn list(m: &Messages, items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, middle @ .., last] => {
            let head = middle.iter().fold(first.clone(), |acc, item| {
                m.with("ts-version-list-comma", &[&acc, item])
            });
            m.with("ts-version-list-and", &[&head, last])
        }
    }
}

/// An external link.
fn ext(url: &str, text_html: &str) -> String {
    link(url, text_html, " class=\"external\" rel=\"nofollow\"")
}

fn table(caption: String, columns: &[String], rows: Vec<Vec<Cell>>, empty: String) -> String {
    headed_table(None, caption, columns, rows, empty)
}

fn headed_table(
    header: Option<String>,
    caption: String,
    columns: &[String],
    rows: Vec<Vec<Cell>>,
    empty: String,
) -> String {
    Table {
        header,
        caption,
        columns: columns
            .iter()
            .map(|c| Column {
                label: c.clone(),
                align: crate::codex::Align::Start,
            })
            .collect(),
        rows,
        empty,
    }
    .render()
    .unwrap_or_default()
}

fn text(s: &str) -> Cell {
    Cell::html(esc(s))
}

fn h2(id: &str, title: &str) -> String {
    format!("<h2 id=\"{id}\">{}</h2>", esc(title))
}

/// The page's own title as shown: `Special:Version/License/vue@3.5.43` → `License/vue@3.5.43`.
fn subpage(title: &str) -> &str {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map_or("", |(_, rest)| rest)
}

/// `Special:Version` and its subpages.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Version) {
        return pages::no_page(&cx, headers, "Special:Version".into());
    }
    let sub = subpage(title);
    let lower = sub.to_ascii_lowercase();
    if lower == "license" || lower == "licence" {
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-version-license-title"),
            returnto: Some("Special:Version/License".into()),
            body: format!(
                "<p>{}</p><pre class=\"ts-version__license\" lang=\"en\" dir=\"ltr\">{}</pre>",
                link("/wiki/Special:Version", &esc(&m.get("ts-version-back")), ""),
                esc(GPL)
            ),
            ..Page::default()
        });
    }
    if let Some(id) = sub
        .strip_prefix("License/")
        .or_else(|| sub.strip_prefix("license/"))
        .or_else(|| sub.strip_prefix("Licence/"))
    {
        return component_license(site, &mut cx, headers, &incoming, id).await;
    }
    let fetched = match site.client().version(&incoming).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(fetched.cache);
    let v = fetched.value;
    let local = site.build().cloned();
    if lower == "credits" {
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-version-credits-title"),
            returnto: Some("Special:Version/Credits".into()),
            body: credits_page(m, &v),
            ..Page::default()
        });
    }
    if !sub.is_empty() {
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: m.get("ts-not-found-title"),
            body: pages::message(MessageKind::Warning, &m.get("ts-not-found")),
            ..Page::default()
        });
    }
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-version-title"),
        returnto: Some("Special:Version".into()),
        body: body(m, &v, local.as_ref()),
        ..Page::default()
    })
}

async fn component_license(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &triplespace_client::Incoming,
    id: &str,
) -> Response {
    let fetched = match site.client().component_license(incoming, id).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(fetched.cache);
    let Some(c) = fetched.value else {
        return pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: m.get("ts-not-found-title"),
            body: pages::message(
                MessageKind::Warning,
                &m.with("ts-version-license-no-component", &[id]),
            ),
            ..Page::default()
        });
    };
    pages::respond(cx, headers, StatusCode::OK, |m| {
        let mut body = format!(
            "<p>{}</p><p>{}</p>",
            link(
                "/wiki/Special:Version#ts-version-components",
                &esc(&m.get("ts-version-back")),
                ""
            ),
            esc(&m.with("ts-version-license-of", &[&c.name, &c.version, &c.license])),
        );
        if let Some(r) = c.repository.as_deref().filter(|r| crate::html::linkable(r)) {
            let _ = write!(body, "<p>{}</p>", ext(r, &esc(r)));
        }
        if c.texts.is_empty() {
            body.push_str(&pages::message(
                MessageKind::Notice,
                &m.get("ts-version-license-no-files"),
            ));
        }
        for t in &c.texts {
            let _ = write!(
                body,
                "<h2>{}</h2><pre class=\"ts-version__license\" dir=\"ltr\">{}</pre>",
                esc(&t.file),
                esc(t.text.as_deref().unwrap_or_default())
            );
        }
        Page {
            title: m.with("ts-version-license-component-title", &[&c.id]),
            returnto: Some(format!("Special:Version/License/{}", c.id)),
            body,
            ..Page::default()
        }
    })
}

/// The credit line, the funders' sentence and the licence notice (0077 §2, §3).
fn credit_and_notice(m: &Messages, v: &Version) -> String {
    let product = ext(&v.developer.source, &esc(&v.product));
    let developer = esc(&v.developer.name);
    let place = esc(&v.developer.place);
    let mut names: Vec<String> = v
        .contributors
        .iter()
        .take(CREDIT_LINE_NAMES)
        .map(
            |c| match c.url.as_deref().filter(|u| crate::html::linkable(u)) {
                Some(u) => ext(u, &esc(&c.name)),
                None => esc(&c.name),
            },
        )
        .collect();
    if v.contributors.len() > CREDIT_LINE_NAMES {
        names.push(link(
            "/wiki/Special:Version/Credits",
            &esc(&m.get("ts-version-poweredby-others")),
            "",
        ));
    }
    let mut out = String::from("<div class=\"ts-version__credits\"><p>");
    if names.is_empty() {
        out.push_str(&rich(
            m,
            "ts-version-poweredby",
            &[&product, &developer, &place],
        ));
    } else {
        // The list's parts are HTML, so it is joined from escaped parts here.
        let joined = html_list(m, &names);
        out.push_str(&rich(
            m,
            "ts-version-poweredby-contributors",
            &[&product, &developer, &place, &joined],
        ));
    }
    out.push_str("</p>");
    if !v.funders.is_empty() {
        let funders: Vec<String> = v
            .funders
            .iter()
            .map(
                |f| match f.url.as_deref().filter(|u| crate::html::linkable(u)) {
                    Some(u) => ext(u, &esc(&f.name)),
                    None => esc(&f.name),
                },
            )
            .collect();
        let _ = write!(
            out,
            "<p>{}</p>",
            rich(m, "ts-version-funders", &[&html_list(m, &funders)])
        );
    }
    let name = esc(&v.product);
    let copy = link(
        "/wiki/Special:Version/License",
        &esc(&m.get("ts-version-license-copy-link")),
        "",
    );
    let gnu = ext(
        "https://www.gnu.org/licenses/",
        "https://www.gnu.org/licenses/",
    );
    let _ = write!(
        out,
        "<p>{}</p><p>{}</p><p>{}</p><p>{}</p>",
        rich(m, "ts-version-license-info", &[&name]),
        rich(m, "ts-version-license-warranty", &[&name]),
        rich(m, "ts-version-license-copy", &[&copy, &gnu]),
        esc(&m.with("ts-version-dual-and-cc0", &[&v.developer.name])),
    );
    let _ = write!(
        out,
        "<p>{}</p></div>",
        link(
            "/wiki/Special:Version/Credits",
            &esc(&m.get("ts-version-credits-link")),
            ""
        )
    );
    out
}

/// A list of HTML parts, joined in the reader's language.
fn html_list(m: &Messages, parts: &[String]) -> String {
    let slots: Vec<String> = (0..parts.len())
        .map(|i| format!("\u{F000}{i}\u{F001}"))
        .collect();
    let mut s = esc(&list(m, &slots));
    for (i, p) in parts.iter().enumerate() {
        s = s.replace(&format!("\u{F000}{i}\u{F001}"), p);
    }
    s
}

fn build_cell(m: &Messages, b: &Build) -> String {
    let mut parts = Vec::new();
    if !b.commit.is_empty() {
        let short: String = b.commit.chars().take(12).collect();
        parts.push(format!("<code>{}</code>", esc(&short)));
    }
    if !b.commit_date.is_empty() {
        parts.push(esc(&b.commit_date));
    }
    if b.modified {
        parts.push(esc(&m.get("ts-version-build-modified")));
    }
    parts.join(" · ")
}

fn build_details(b: &Build) -> String {
    let mut parts = Vec::new();
    for p in [&b.rustc, &b.target] {
        if !p.is_empty() {
            parts.push(esc(p));
        }
    }
    if !b.features.is_empty() {
        parts.push(esc(&b.features.join(", ")));
    }
    parts.join(" · ")
}

/// Installed software: the build or builds, then the configured services (0077 §4).
fn installed(m: &Messages, v: &Version, local: Option<&Build>) -> String {
    let mut rows = Vec::new();
    let source = match v.license.source.as_deref() {
        Some(u) => ext(u, &esc(&m.get("ts-version-source"))),
        None => esc(&m.get("ts-version-modified-unsourced")),
    };
    rows.push(vec![
        Cell {
            header: true,
            ..text(&v.product)
        },
        Cell::html(format!(
            "{} · {}",
            esc(&v.build.version),
            build_cell(m, &v.build)
        )),
        Cell::html(format!("{}<br>{source}", build_details(&v.build))),
    ]);
    if let Some(w) = local {
        rows.push(vec![
            Cell {
                header: true,
                ..text(&m.with("ts-version-webtier", &[&v.product]))
            },
            Cell::html(format!("{} · {}", esc(&w.version), build_cell(m, w))),
            Cell::html(build_details(w)),
        ]);
    }
    for s in &v.services {
        let state = m.get(if s.state == "connected" {
            "ts-version-state-connected"
        } else {
            "ts-version-state-unreachable"
        });
        rows.push(vec![
            Cell {
                header: true,
                ..text(&s.name)
            },
            text(s.version.as_deref().unwrap_or("")),
            Cell::html(format!(
                "{}{}",
                s.role
                    .as_deref()
                    .map(|r| format!("{} · ", esc(r)))
                    .unwrap_or_default(),
                esc(&state)
            )),
        ]);
    }
    format!(
        "{}{}",
        h2("ts-version-installed", &m.get("ts-version-installed")),
        table(
            m.get("ts-version-installed"),
            &[
                m.get("ts-version-col-product"),
                m.get("ts-version-col-version"),
                m.get("ts-version-col-details")
            ],
            rows,
            String::new(),
        )
    )
}

fn value_text(m: &Messages, f: &FeatureSwitch) -> String {
    let shown = match &f.value {
        None => return esc(&m.get("ts-version-not-set")),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => (if *b { "true" } else { "false" }).to_string(),
        Some(Value::Array(a)) if a.is_empty() => return esc(&m.get("ts-version-none")),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string))
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    };
    if f.set {
        format!("<code>{}</code>", esc(&shown))
    } else {
        format!(
            "<code>{}</code> {}",
            esc(&shown),
            esc(&m.get("ts-version-default"))
        )
    }
}

fn features(m: &Messages, v: &Version) -> String {
    let mut out = String::new();
    let mut section = |caption: String, list: &[FeatureSwitch], extra: Vec<Vec<Cell>>| {
        let mut rows: Vec<Vec<Cell>> = list
            .iter()
            .map(|f| {
                vec![
                    Cell {
                        header: true,
                        ..Cell::html(format!("<code>{}</code>", esc(&f.key)))
                    },
                    Cell::html(value_text(m, f)),
                ]
            })
            .collect();
        rows.extend(extra);
        out.push_str(&headed_table(
            Some(caption.clone()),
            caption,
            &[
                m.get("ts-version-col-setting"),
                m.get("ts-version-col-value"),
            ],
            rows,
            String::new(),
        ));
    };
    if let Some(t) = &v.features.tenant {
        let providers = if v.features.providers.is_empty() {
            esc(&m.get("ts-version-none"))
        } else {
            esc(&v.features.providers.join(", "))
        };
        section(
            m.get("ts-version-features-tenant"),
            t,
            vec![vec![
                Cell {
                    header: true,
                    ..text(&m.get("ts-version-providers"))
                },
                Cell::html(providers),
            ]],
        );
    }
    if let Some(i) = &v.features.instance {
        section(m.get("ts-version-features-instance"), i, Vec::new());
    }
    if out.is_empty() {
        return out;
    }
    format!(
        "{}{out}",
        h2("ts-version-features", &m.get("ts-version-features"))
    )
}

fn entry_points(m: &Messages, v: &Version) -> String {
    let rows = v
        .entry_points
        .iter()
        .map(|e| {
            vec![
                Cell {
                    header: true,
                    ..text(&m.get(&format!(
                        "ts-version-entry-{}",
                        e.name.replace(['.', '/'], "-")
                    )))
                },
                Cell::html(format!("<code>{}</code>", esc(&e.url))),
            ]
        })
        .collect();
    format!(
        "{}{}",
        h2("ts-version-entrypoints", &m.get("ts-version-entrypoints")),
        table(
            m.get("ts-version-entrypoints"),
            &[
                m.get("ts-version-col-entrypoint"),
                m.get("ts-version-col-url")
            ],
            rows,
            String::new()
        )
    )
}

fn component_rows(list: &[Component]) -> Vec<Vec<Cell>> {
    list.iter()
        .map(|c| {
            let name = match c.repository.as_deref().filter(|r| crate::html::linkable(r)) {
                Some(r) => ext(r, &esc(&c.name)),
                None => esc(&c.name),
            };
            vec![
                Cell {
                    header: true,
                    ..Cell::html(name)
                },
                text(&c.version),
                Cell::html(link(
                    &format!(
                        "/wiki/Special:Version/License/{}",
                        crate::frame::title_url(&c.id)
                    ),
                    &esc(&c.license),
                    "",
                )),
            ]
        })
        .collect()
}

fn components(m: &Messages, v: &Version) -> String {
    let mut out = h2("ts-version-crates", &m.get("ts-version-crates"));
    if !v.components_listed {
        out.push_str(&pages::message(
            MessageKind::Notice,
            &m.get("ts-version-components-unlisted"),
        ));
        return out;
    }
    let ws_rows = v
        .workspace
        .iter()
        .map(|c| {
            let licence = if c.commercial {
                format!(
                    "{} {}",
                    esc(&c.license),
                    esc(&m.get("ts-version-commercial-mark"))
                )
            } else {
                esc(&c.license)
            };
            vec![
                Cell {
                    header: true,
                    ..text(&c.name)
                },
                text(&c.version),
                Cell::html(licence),
            ]
        })
        .collect();
    let cols = [
        m.get("ts-version-col-name"),
        m.get("ts-version-col-version"),
        m.get("ts-version-col-license"),
    ];
    out.push_str(&table(
        m.get("ts-version-crates"),
        &cols,
        ws_rows,
        String::new(),
    ));
    out.push_str(&h2(
        "ts-version-components",
        &m.get("ts-version-components"),
    ));
    for (key, list) in [
        ("ts-version-components-crates", &v.crates),
        ("ts-version-components-packages", &v.packages),
        ("ts-version-components-vendored", &v.vendored),
    ] {
        if list.is_empty() {
            continue;
        }
        // Hundreds of crates: each list opens on demand.
        let content = table(m.get(key), &cols, component_rows(list), String::new());
        out.push_str(
            &Accordion {
                title: &m.with(&format!("{key}-count"), &[&list.len().to_string()]),
                description: None,
                content: &content,
                open: false,
            }
            .render()
            .unwrap_or_default(),
        );
    }
    out
}

fn wikitext(m: &Messages, v: &Version) -> String {
    if v.wikitext.is_empty() {
        return String::new();
    }
    let rows = v
        .wikitext
        .iter()
        .map(|w| {
            let shown = if w.written.is_empty() {
                w.name.clone()
            } else {
                w.written.clone()
            };
            vec![
                Cell {
                    header: true,
                    ..Cell::html(format!("<code>{}</code>", esc(&shown)))
                },
                text(w.origin.as_deref().unwrap_or("")),
                text(&m.get(&format!("ts-version-wikitext-{}", w.status))),
                text(&m.get(if w.active {
                    "ts-version-active"
                } else {
                    "ts-version-inactive"
                })),
            ]
        })
        .collect();
    let count = v.wikitext.len().to_string();
    let content = table(
        m.get("ts-version-wikitext"),
        &[
            m.get("ts-version-col-name"),
            m.get("ts-version-col-origin"),
            m.get("ts-version-col-status"),
            m.get("ts-version-col-active"),
        ],
        rows,
        String::new(),
    );
    format!(
        "{}{}",
        h2("ts-version-wikitext", &m.get("ts-version-wikitext")),
        Accordion {
            title: &m.with("ts-version-wikitext-count", &[&count]),
            description: None,
            content: &content,
            open: false,
        }
        .render()
        .unwrap_or_default()
    )
}

fn lineage(m: &Messages, v: &Version) -> String {
    let rows = v
        .inspired_by
        .iter()
        .map(|l| {
            let name = match l.url.as_deref().filter(|u| crate::html::linkable(u)) {
                Some(u) => ext(u, &esc(&l.name)),
                None => esc(&l.name),
            };
            let mut what = Vec::new();
            let mut part = |key: &str, items: &[String]| {
                if items.len() > LONG_LIST {
                    // A long list (MediaWiki core's) opens on demand.
                    what.push(
                        Accordion {
                            title: &m.with(&format!("{key}-count"), &[&items.len().to_string()]),
                            description: None,
                            content: &esc(&items.join(", ")),
                            open: false,
                        }
                        .render()
                        .unwrap_or_default(),
                    );
                } else if !items.is_empty() {
                    what.push(format!("{} {}", esc(&m.get(key)), esc(&items.join(", "))));
                }
            };
            part("ts-version-from-special-pages", &l.special_pages);
            part("ts-version-from-content-models", &l.content_models);
            part("ts-version-from-wikitext", &l.wikitext.implemented);
            part("ts-version-from-recognized", &l.wikitext.recognized);
            for i in &l.inspired_by {
                what.push(esc(i));
            }
            vec![
                Cell {
                    header: true,
                    ..Cell::html(name)
                },
                Cell::html(what.join("<br>")),
            ]
        })
        .collect();
    format!(
        "{}<p>{}</p>{}",
        h2("ts-version-inspiredby", &m.get("ts-version-inspiredby")),
        esc(&m.with("ts-version-inspiredby-intro", &[&v.product])),
        table(
            m.get("ts-version-inspiredby"),
            &[m.get("ts-version-col-name"), m.get("ts-version-col-what")],
            rows,
            String::new()
        )
    )
}

fn agents_table(m: &Messages, v: &Version) -> String {
    let rows = v
        .agents
        .iter()
        .map(|a| {
            vec![
                Cell {
                    header: true,
                    ..text(&a.name)
                },
                text(&a.maker),
                text(&a.versions.join(", ")),
            ]
        })
        .collect();
    table(
        m.get("ts-version-agents"),
        &[
            m.get("ts-version-col-name"),
            m.get("ts-version-col-maker"),
            m.get("ts-version-col-versions"),
        ],
        rows,
        String::new(),
    )
}

fn agents(m: &Messages, v: &Version) -> String {
    if v.agents.is_empty() {
        return String::new();
    }
    format!(
        "{}<p>{}</p>{}",
        h2("ts-version-agents", &m.get("ts-version-agents")),
        esc(&m.with("ts-version-agents-intro", &[&v.product])),
        agents_table(m, v)
    )
}

fn instance(m: &Messages, v: &Version) -> String {
    let mut rows = vec![vec![
        Cell {
            header: true,
            ..text(&m.get("ts-version-instance-name"))
        },
        text(&v.instance.name),
    ]];
    if let Some(owner) = &v.instance.owner_of_record {
        let shown = owner
            .as_str()
            .map_or_else(|| owner.to_string(), str::to_string);
        rows.push(vec![
            Cell {
                header: true,
                ..text(&m.get("ts-version-owner"))
            },
            text(&shown),
        ]);
    }
    format!(
        "{}{}",
        h2("ts-version-instance", &m.get("ts-version-instance")),
        table(
            m.get("ts-version-instance"),
            &[
                m.get("ts-version-col-setting"),
                m.get("ts-version-col-value")
            ],
            rows,
            String::new()
        )
    )
}

/// The page body, in the order of 0077 §1.
#[must_use]
pub fn body(m: &Messages, v: &Version, local: Option<&Build>) -> String {
    [
        credit_and_notice(m, v),
        installed(m, v, local),
        features(m, v),
        entry_points(m, v),
        components(m, v),
        wikitext(m, v),
        lineage(m, v),
        agents(m, v),
        instance(m, v),
    ]
    .concat()
}

fn credits_page(m: &Messages, v: &Version) -> String {
    let mut out = format!(
        "<p>{}</p>{}",
        link("/wiki/Special:Version", &esc(&m.get("ts-version-back")), ""),
        h2(
            "ts-version-contributors",
            &m.get("ts-version-credits-contributors")
        )
    );
    if v.contributors.is_empty() {
        let _ = write!(
            out,
            "<p>{}</p>",
            esc(&m.with(
                "ts-version-credits-developer-only",
                &[&v.product, &v.developer.name]
            ))
        );
    } else {
        let rows = v
            .contributors
            .iter()
            .map(|c| {
                vec![
                    Cell {
                        header: true,
                        ..text(&c.name)
                    },
                    text(c.since.as_deref().unwrap_or("")),
                ]
            })
            .collect();
        out.push_str(&table(
            m.get("ts-version-credits-contributors"),
            &[m.get("ts-version-col-name"), m.get("ts-version-col-since")],
            rows,
            String::new(),
        ));
    }
    if !v.funders.is_empty() {
        out.push_str(&h2(
            "ts-version-funders",
            &m.get("ts-version-credits-funders"),
        ));
        let rows = v
            .funders
            .iter()
            .map(|f| {
                let when = match (&f.since, &f.until) {
                    (Some(s), Some(u)) => format!("{s} – {u}"),
                    (Some(s), None) => s.clone(),
                    _ => String::new(),
                };
                vec![
                    Cell {
                        header: true,
                        ..text(&f.name)
                    },
                    text(f.funded.as_deref().unwrap_or("")),
                    text(&when),
                ]
            })
            .collect();
        out.push_str(&table(
            m.get("ts-version-credits-funders"),
            &[
                m.get("ts-version-col-name"),
                m.get("ts-version-col-funded"),
                m.get("ts-version-col-since"),
            ],
            rows,
            String::new(),
        ));
    }
    if !v.agents.is_empty() {
        out.push_str(&h2("ts-version-agents", &m.get("ts-version-agents")));
        out.push_str(&agents_table(m, v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::version::{Credit, Developer, License};

    fn sample() -> Version {
        serde_json::from_value(serde_json::json!({
            "product": "Triplespace",
            "developer": {"name": "Scatter LLC", "place": "Portland, Oregon", "source": "https://github.com/scatterbase/triplespace"},
            "contributors": [], "funders": [],
            "license": {"spdx": "GPL-3.0-or-later", "source": "https://github.com/scatterbase/triplespace/tree/abc", "modified": false},
            "build": {"version": "0.0.1", "commit": "abcdef0123456789", "modified": false, "rustc": "rustc 1.95.0", "target": "x86_64-unknown-linux-gnu"},
            "services": [{"name": "PostgreSQL", "role": "Log store and serving model", "state": "connected", "version": "16.4"}],
            "components_listed": true,
            "workspace": [{"name": "scatter-log", "version": "0.0.1", "license": "GPL-3.0-or-later", "commercial": true}],
            "crates": [{"id": "url@2.5.4", "name": "url", "version": "2.5.4", "license": "MIT OR Apache-2.0", "repository": "https://github.com/servo/rust-url"}],
            "packages": [{"id": "@wikimedia/codex@2.7.0", "name": "@wikimedia/codex", "version": "2.7.0", "license": "GPL-2.0+"}],
            "vendored": [],
            "features": {"tenant": [{"key": "wikitext.expansion", "set": false, "value": "off"}, {"key": "content.licence", "set": false}], "providers": ["wikidata"]},
            "entry_points": [{"name": "api.php", "url": "https://x.example/w/api.php"}],
            "wikitext": [],
            "inspired_by": [{"name": "MediaWiki", "url": "https://www.mediawiki.org/wiki/MediaWiki", "special_pages": ["Search", "Version"], "content_models": [], "wikitext": {"implemented": [], "recognized": []}, "inspired_by": []}],
            "agents": [{"name": "Claude Opus", "maker": "Anthropic", "versions": ["5.5"]}],
            "instance": {"name": "scatter"}
        }))
        .unwrap()
    }

    #[test]
    fn the_credit_line_without_contributors() {
        let m = Messages::for_language("en");
        let html = credit_and_notice(&m, &sample());
        assert!(
            html.contains(
                "This wiki is powered by <a href=\"https://github.com/scatterbase/triplespace\""
            ),
            "{html}"
        );
        assert!(
            html.contains(">Triplespace</a>, developed by Scatter LLC in Portland, Oregon.</p>"),
            "{html}"
        );
        assert!(!html.contains("with contributions"));
        assert!(
            html.contains("either version 3 of the License, or (at your option) any later version")
        );
        assert!(html.contains("<a href=\"/wiki/Special:Version/License\""));
        assert!(!html.contains("Development has been supported"));
    }

    #[test]
    fn the_credit_line_with_contributors_and_funders() {
        let m = Messages::for_language("en");
        let mut v = sample();
        v.contributors = ["Alice", "Bob"]
            .iter()
            .map(|n| Credit {
                name: (*n).into(),
                ..Credit::default()
            })
            .collect();
        v.funders = vec![Credit {
            name: "A Foundation".into(),
            ..Credit::default()
        }];
        let html = credit_and_notice(&m, &v);
        assert!(html.contains("developed by Scatter LLC in Portland, Oregon, with contributions from Alice and Bob."), "{html}");
        assert!(
            html.contains("Development has been supported by A Foundation."),
            "{html}"
        );
        v.contributors = (0..12)
            .map(|i| Credit {
                name: format!("C{i}"),
                ..Credit::default()
            })
            .collect();
        let html = credit_and_notice(&m, &v);
        assert!(
            html.contains("C9 and <a href=\"/wiki/Special:Version/Credits\">others</a>."),
            "{html}"
        );
        assert!(!html.contains("C10"));
    }

    #[test]
    fn names_are_escaped() {
        let m = Messages::for_language("en");
        let mut v = sample();
        v.developer = Developer {
            name: "<b>X</b>".into(),
            ..v.developer
        };
        let html = credit_and_notice(&m, &v);
        assert!(html.contains("&lt;b&gt;X&lt;/b&gt;") && !html.contains("<b>X"));
    }

    #[test]
    fn the_body_in_order() {
        let m = Messages::for_language("en");
        let html = body(&m, &sample(), None);
        let order = [
            "This wiki is powered by",
            "Installed software",
            "PostgreSQL",
            "Features",
            "Entry point URLs",
            "Triplespace crates",
            "Third-party components",
            "Inspired by",
            "AI agents",
            "Instance",
        ];
        let mut at = 0;
        for o in order {
            let i = html[at..]
                .find(o)
                .unwrap_or_else(|| panic!("{o} missing or out of order"));
            at += i;
        }
        let href = format!(
            "/wiki/Special:Version/License/{}",
            crate::frame::title_url("@wikimedia/codex@2.7.0")
        );
        assert!(html.contains(&href), "{html}");
        assert!(html.contains("also available under a commercial licence"));
        assert!(html.contains("not set"));
        assert!(!html.contains("⧼"), "a message is missing: {html}");
        assert!(!html.contains("ADR"));
    }

    #[test]
    fn an_unlisted_build_and_a_modified_one() {
        let m = Messages::for_language("en");
        let mut v = sample();
        v.components_listed = false;
        v.license = License {
            source: None,
            modified: true,
            ..v.license
        };
        let html = body(&m, &v, None);
        assert!(
            html.contains("Development build: components are not listed."),
            "{html}"
        );
        assert!(html.contains("source location not configured"));
    }

    #[test]
    fn the_credits_page() {
        let m = Messages::for_language("en");
        let html = credits_page(&m, &sample());
        assert!(html.contains("Claude Opus"));
        assert!(!html.contains("⧼"), "{html}");
    }

    #[test]
    fn subpages() {
        assert_eq!(subpage("Special:Version"), "");
        assert_eq!(
            subpage("Special:Version/License/@wikimedia/codex@2.7.0"),
            "License/@wikimedia/codex@2.7.0"
        );
        assert!(GPL.contains("GNU GENERAL PUBLIC LICENSE"));
    }
}
