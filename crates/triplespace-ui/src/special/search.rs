//! `Special:Search` (0010 §3; 0047 §9): the full results page, readable without
//! JavaScript.
//!
//! - **Go to.** Text that names an entity (an ID such as `Q42` or `WDQ65`, a page title
//!   such as `Item:Q42` or `Domain:wikipedia.org`, or a bare domain key such as
//!   `wikipedia.org`) goes straight to its page when it exists. As in MediaWiki, the
//!   header's box asks for this (no `fulltext`), and the results page's own form does
//!   not: there, an entity the text names is offered as **Go to** above the results.
//!   Text that is sure to be an ID (a local ID, or a namespaced title) and names nothing
//!   says so; text that merely could be one (a foreign ID's shape, a dotted word) falls
//!   through to the results quietly.
//! - **Results** are grouped by kind (items, properties, domains), seven of each, with
//!   a link to the rest of a kind. `ns{n}=1` (120, 122, 210) asks for one kind, paged by
//!   `offset` and `limit` (MediaWiki's parameters).
//!
//! The search itself is the API's `wbsearchentities`: labels and aliases, by prefix, in
//! the interface language and its fallbacks (0014 §8). Page titles and page text join
//! when the pages stack does (0010 §3).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use scatter_wikibase_model::id::{EntityId, IdForm};
use triplespace_client::{ClientError, Fetched, Hit, Hits, SiteInfo};

use crate::Site;
use crate::codex::{Chip, ChipKind, MessageKind, SearchInput};
use crate::entity;
use crate::frame::{Page, title_url};
use crate::html::{esc, lang_attrs, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

/// A kind of entity the search covers: its `wbsearchentities` type, its namespace, and
/// the messages of its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kind {
    entity_type: &'static str,
    ns: u32,
    heading: &'static str,
    more: &'static str,
}

const KINDS: [Kind; 3] = [
    Kind {
        entity_type: "item",
        ns: 120,
        heading: "ts-search-group-item",
        more: "ts-search-more-item",
    },
    Kind {
        entity_type: "property",
        ns: 122,
        heading: "ts-search-group-property",
        more: "ts-search-more-property",
    },
    Kind {
        entity_type: "domain",
        ns: 210,
        heading: "ts-search-group-domain",
        more: "ts-search-more-domain",
    },
];

/// Results per kind when every kind is shown.
const PER_GROUP: u32 = 7;
/// Results per page for one kind, unless `limit` says otherwise; and the most allowed.
const PER_PAGE: u32 = 20;
const MAX_PER_PAGE: u32 = 50;

/// The namespaces whose pages are entities, with the type of entity each holds.
const NAMESPACES: [(&str, &str); 5] = [
    ("item", "item"),
    ("property", "property"),
    ("domain", "domain"),
    ("keyword", "keyword"),
    ("notation", "notation"),
];

/// Whether a local ID's shape: a letter, then digits only.
fn local_shape(id: &EntityId) -> bool {
    id.form() == IdForm::Local
}

/// Whether a foreign ID begins with a provider code and one of its type codes that the
/// site knows, so that `Paris` (which has the shape of a foreign ID) is not taken for one.
fn known_foreign(id: &str, site: &SiteInfo) -> bool {
    site.providers.iter().any(|p| {
        id.get(..2) == Some(p.code.as_str())
            && p.types
                .iter()
                .any(|t| id.get(2..3) == Some(t.code.as_str()))
    }) && id.len() > 3
}

/// The page title the text names, if it looks like an ID or key, and whether it surely
/// is one (a local ID, or a title in an entity namespace) rather than possibly.
#[must_use]
pub fn go_title(text: &str, site: &SiteInfo) -> Option<(String, bool)> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((prefix, rest)) = t.split_once(':') {
        let lower = prefix.to_ascii_lowercase();
        let (_, kind) = NAMESPACES.iter().find(|(ns, _)| *ns == lower)?;
        if rest.is_empty() {
            return None;
        }
        return match *kind {
            "item" | "property" => {
                let id = EntityId::parse(rest).ok()?;
                (id.form() != IdForm::Keyed).then(|| (entity::page_title(id.as_str(), kind), true))
            }
            keyed => {
                let key = if keyed == "domain" {
                    rest.to_lowercase()
                } else {
                    rest.to_string()
                };
                Some((entity::page_title(&format!("{keyed}:{key}"), keyed), true))
            }
        };
    }
    if let Ok(id) = EntityId::parse(t) {
        if local_shape(&id) {
            return Some((
                entity::page_title(id.as_str(), &entity::entity_type_of(id.as_str())),
                true,
            ));
        }
        if id.form() == IdForm::Foreign && known_foreign(id.as_str(), site) {
            return Some((
                entity::page_title(id.as_str(), &entity::entity_type_of(id.as_str())),
                false,
            ));
        }
    }
    let dotted = t.contains('.')
        && !t.starts_with('.')
        && !t.ends_with('.')
        && t.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-'));
    dotted.then(|| (format!("Domain:{}", t.to_lowercase()), false))
}

/// The kinds `ns{n}` asks for; every kind when it asks for none it knows.
fn wanted(query: &BTreeMap<String, String>) -> Vec<Kind> {
    let asked: Vec<Kind> = KINDS
        .iter()
        .copied()
        .filter(|k| {
            query
                .get(&format!("ns{}", k.ns))
                .is_some_and(|v| !v.is_empty() && v != "0")
        })
        .collect();
    if asked.is_empty() {
        KINDS.to_vec()
    } else {
        asked
    }
}

/// A link to the results for `text`, optionally of one kind and from an offset.
fn results_href(text: &str, kind: Option<Kind>, offset: u32, limit: Option<u32>) -> String {
    let mut s = format!(
        "/w/index.php?title=Special:Search&search={}&fulltext=1",
        utf8_percent_encode(text, NON_ALPHANUMERIC)
    );
    if let Some(k) = kind {
        let _ = write!(s, "&ns{}=1", k.ns);
    }
    if offset > 0 {
        let _ = write!(s, "&offset={offset}");
    }
    if let Some(l) = limit {
        let _ = write!(s, "&limit={l}");
    }
    s
}

/// A number from the query, within bounds.
fn number(query: &BTreeMap<String, String>, key: &str, default: u32, max: u32) -> u32 {
    query
        .get(key)
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map_or(default, |n| n.min(max))
}

/// A `302` to an entity's page, cached as its inputs allow.
fn redirect(cx: &Context, title: &str) -> Response {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    if let Ok(v) = HeaderValue::from_str(&format!("/wiki/{}", title_url(title))) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    pages::caching(r.headers_mut(), None, &cx.inputs);
    r
}

/// The ID's chip: the local ID, or the foreign ID in its provider's colours. A keyed
/// entity's key is its label already, so it has none.
fn id_chip(id: &str) -> String {
    let chip = match EntityId::parse(id).map(|e| e.form()) {
        Ok(IdForm::Keyed) => return String::new(),
        Ok(IdForm::Foreign) => Chip::new(ChipKind::Provider(id[..2].to_string()), id),
        _ => Chip::new(ChipKind::Local, id),
    };
    format!(" {}", chip.render().unwrap_or_default())
}

/// One result.
fn hit(m: &Messages, h: &Hit, kind: Kind) -> String {
    let lang = m.lang();
    let title = match &h.label {
        Some((text, l)) => format!("<span{}>{}</span>", lang_attrs(l, lang), esc(text)),
        None => match EntityId::parse(&h.id)
            .ok()
            .and_then(|e| e.keyed_parts().map(|(_, k)| k.to_string()))
        {
            Some(key) => format!("<span class=\"ts-mono\" dir=\"ltr\">{}</span>", esc(&key)),
            None => format!("<span class=\"ts-mono\">{}</span>", esc(&h.id)),
        },
    };
    let mut s = format!(
        "<li class=\"ts-hit\"><div class=\"ts-hit__head\">{}{}</div>",
        link(
            &entity::entity_href(&h.id, kind.entity_type),
            &title,
            " class=\"ts-hit__title\""
        ),
        id_chip(&h.id)
    );
    if let Some((text, l)) = &h.description {
        let _ = write!(
            s,
            "<p class=\"ts-hit__description\"{}>{}</p>",
            lang_attrs(l, lang),
            esc(text)
        );
    }
    if h.match_type == "alias" {
        let (text, l) = &h.match_text;
        let alias = format!("<span{}>{}</span>", lang_attrs(l, lang), esc(text));
        let message = esc(&m.with("ts-search-matched-alias", &["\u{E000}"]));
        let _ = write!(
            s,
            "<p class=\"ts-hit__match\">{}</p>",
            message.replace('\u{E000}', &format!("<bdi>{alias}</bdi>"))
        );
    }
    s.push_str("</li>");
    s
}

/// The search form at the top of the page: results, not **Go to**.
fn form(m: &Messages, text: &str, kinds: &[Kind], single: bool) -> String {
    let mut s = format!(
        "<form class=\"ts-search-form\" role=\"search\" aria-label=\"{}\" action=\"/w/index.php\" method=\"get\"><input type=\"hidden\" name=\"title\" value=\"Special:Search\"><input type=\"hidden\" name=\"fulltext\" value=\"1\">",
        esc(&m.get("ts-search-title"))
    );
    if single {
        for k in kinds {
            let _ = write!(s, "<input type=\"hidden\" name=\"ns{}\" value=\"1\">", k.ns);
        }
    }
    s.push_str(
        &SearchInput {
            name: "search",
            value: text,
            placeholder: &m.get("ts-search-placeholder"),
            label: &m.get("ts-search-label"),
            button: &m.get("ts-search-button"),
        }
        .render()
        .unwrap_or_default(),
    );
    s.push_str("</form>");
    s
}

/// The links that narrow the search to one kind, or widen it to all.
fn filter(m: &Messages, text: &str, current: Option<Kind>) -> String {
    let mut s = format!(
        "<nav class=\"ts-search-filter\" aria-label=\"{}\"><ul>",
        esc(&m.get("ts-search-filter-label"))
    );
    let mut item = |label: String, kind: Option<Kind>| {
        let here = kind == current;
        let _ = write!(
            s,
            "<li>{}</li>",
            link(
                &results_href(text, kind, 0, None),
                &esc(&label),
                if here { " aria-current=\"page\"" } else { "" }
            )
        );
    };
    item(m.get("ts-search-filter-all"), None);
    for k in KINDS {
        item(m.get(k.heading), Some(k));
    }
    s.push_str("</ul></nav>");
    s
}

/// One kind's results: its heading, its hits, and a link to more.
fn group(m: &Messages, text: &str, kind: Kind, hits: &Hits, single: bool) -> String {
    let id = format!("ts-results-{}", kind.entity_type);
    let mut s = format!(
        "<section class=\"ts-results\" aria-labelledby=\"{id}\"><h2 class=\"ts-results__heading\" id=\"{id}\">{}</h2><ol class=\"ts-hits\">",
        esc(&m.get(kind.heading))
    );
    for h in &hits.hits {
        s.push_str(&hit(m, h, kind));
    }
    s.push_str("</ol>");
    if !single && hits.next.is_some() {
        let _ = write!(
            s,
            "<p class=\"ts-results__more\">{}</p>",
            link(
                &results_href(text, Some(kind), 0, None),
                &esc(&m.get(kind.more)),
                ""
            )
        );
    }
    s.push_str("</section>");
    s
}

/// The previous and next pages of one kind's results.
fn pager(
    m: &Messages,
    text: &str,
    kind: Kind,
    offset: u32,
    limit: u32,
    next: Option<u32>,
) -> String {
    if offset == 0 && next.is_none() {
        return String::new();
    }
    let custom = (limit != PER_PAGE).then_some(limit);
    let mut s = format!(
        "<nav class=\"ts-pager\" aria-label=\"{}\">",
        esc(&m.get("ts-search-pages-label"))
    );
    if offset > 0 {
        let _ = write!(
            s,
            "{} ",
            link(
                &results_href(text, Some(kind), offset.saturating_sub(limit), custom),
                &esc(&m.get("ts-search-previous")),
                " rel=\"prev\""
            )
        );
    }
    if let Some(n) = next {
        s.push_str(&link(
            &results_href(text, Some(kind), n, custom),
            &esc(&m.get("ts-search-next")),
            " rel=\"next\"",
        ));
    }
    s.push_str("</nav>");
    s
}

/// The entity the text names, if it exists: its page title. An ID the API refuses
/// outright (`invalid-entity-id`) names nothing.
async fn existing(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    title: &str,
) -> Result<Option<String>, ClientError> {
    match site
        .client()
        .entities(incoming, &[title.to_string()], "info", "")
        .await
    {
        Ok(Fetched { value, cache }) => {
            cx.inputs.push(cache);
            Ok(value
                .found
                .iter()
                .next()
                .map(|(id, v)| entity::page_title(id, v["type"].as_str().unwrap_or("item"))))
        }
        Err(ClientError::Api { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// What the query asks for.
struct Ask {
    text: String,
    kinds: Vec<Kind>,
    single: bool,
    limit: u32,
    offset: u32,
}

impl Ask {
    fn of(query: &BTreeMap<String, String>) -> Self {
        let kinds = wanted(query);
        let single = kinds.len() == 1;
        let (limit, offset) = if single {
            (
                number(query, "limit", PER_PAGE, MAX_PER_PAGE).max(1),
                number(query, "offset", 0, 10_000),
            )
        } else {
            (PER_GROUP, 0)
        };
        Self {
            text: query.get("search").map_or("", |s| s.trim()).to_string(),
            kinds,
            single,
            limit,
            offset,
        }
    }
}

/// Searches each kind asked for, in parallel.
async fn fetch(
    site: &Site,
    incoming: &triplespace_client::Incoming,
    ask: &Ask,
    lang: &str,
) -> Result<Vec<(Kind, Fetched<Hits>)>, ClientError> {
    let client = site.client();
    let run = |k: Kind| {
        let client = &client;
        async move {
            if ask.kinds.contains(&k) {
                Some(
                    client
                        .search(
                            incoming,
                            &ask.text,
                            lang,
                            k.entity_type,
                            ask.limit,
                            ask.offset,
                        )
                        .await,
                )
            } else {
                None
            }
        }
    };
    let (a, b, c) = tokio::join!(run(KINDS[0]), run(KINDS[1]), run(KINDS[2]));
    let mut out = Vec::new();
    for (k, r) in KINDS.into_iter().zip([a, b, c]) {
        if let Some(r) = r {
            out.push((k, r?));
        }
    }
    Ok(out)
}

/// The results: the kind filter, each kind's group, and the pager.
fn results_html(m: &Messages, ask: &Ask, results: &[(Kind, Hits)]) -> String {
    let text = &ask.text;
    let mut body = filter(m, text, ask.single.then(|| ask.kinds[0]));
    if results.iter().any(|(_, h)| !h.hits.is_empty()) {
        for (k, h) in results {
            if !h.hits.is_empty() || ask.single {
                body.push_str(&group(m, text, *k, h, ask.single));
            }
        }
    } else {
        let _ = write!(
            body,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.with("ts-search-none", &[text]))
        );
    }
    if let (true, Some((k, h))) = (ask.single, results.first()) {
        body.push_str(&pager(m, text, *k, ask.offset, ask.limit, h.next));
    }
    body
}

/// `Special:Search`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let ask = Ask::of(query);

    let mut go = None;
    let mut missing = None;
    if let Some((title, sure)) = go_title(&ask.text, &cx.site) {
        match existing(site, &mut cx, &incoming, &title).await {
            Ok(Some(found)) => go = Some(found),
            Ok(None) if sure => missing = Some(title),
            Ok(None) => {}
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    if let (Some(title), false) = (&go, query.contains_key("fulltext")) {
        return redirect(&cx, title);
    }

    let mut results = Vec::new();
    if !ask.text.is_empty() {
        let lang = cx.m.lang().to_string();
        match fetch(site, &incoming, &ask, &lang).await {
            Ok(r) => {
                for (k, f) in r {
                    cx.inputs.push(f.cache);
                    results.push((k, f.value));
                }
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }

    let m = &cx.m;
    let mut body = form(m, &ask.text, &ask.kinds, ask.single);
    if let Some(title) = &missing {
        body.push_str(&pages::message(
            MessageKind::Warning,
            &m.with("ts-search-no-entity", &[title]),
        ));
    }
    if let Some(title) = &go {
        let _ = write!(
            body,
            "<p class=\"ts-search-go\">{}</p>",
            link(
                &format!("/wiki/{}", title_url(title)),
                &esc(&m.with("ts-search-go-to", &[title])),
                ""
            )
        );
    }
    if ask.text.is_empty() {
        let _ = write!(
            body,
            "<p class=\"ts-search-intro\">{}</p>",
            esc(&m.get("ts-search-intro"))
        );
    } else {
        body.push_str(&results_html(m, &ask, &results));
    }
    let body = format!("<div class=\"ts-search\">{body}</div>");
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-search-title"),
        returnto: Some("Special:Search".into()),
        body,
        search: ask.text,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::{ProviderInfo, ProviderType};

    fn site() -> SiteInfo {
        SiteInfo {
            providers: vec![ProviderInfo {
                code: "WD".into(),
                slug: "wikidata".into(),
                types: vec![ProviderType {
                    code: "Q".into(),
                    entity_type: "item".into(),
                    ..ProviderType::default()
                }],
                ..ProviderInfo::default()
            }],
            ..SiteInfo::default()
        }
    }

    #[test]
    fn go_titles() {
        let s = site();
        let go = |t: &str| go_title(t, &s);
        assert_eq!(go("Q6"), Some(("Item:Q6".into(), true)));
        assert_eq!(go("q6"), Some(("Item:Q6".into(), true)));
        assert_eq!(go("P3"), Some(("Property:P3".into(), true)));
        assert_eq!(go("item:q6"), Some(("Item:Q6".into(), true)));
        assert_eq!(go("WDQ65"), Some(("Item:WDQ65".into(), false)));
        assert_eq!(
            go("Domain:Wikipedia.org"),
            Some(("Domain:wikipedia.org".into(), true))
        );
        assert_eq!(
            go("wikipedia.org"),
            Some(("Domain:wikipedia.org".into(), false))
        );
        assert_eq!(go("Paris"), None, "a word with a foreign ID's shape");
        assert_eq!(go("six and more"), None);
        assert_eq!(go("Talk:Q6"), None);
        assert_eq!(go(""), None);
    }

    #[test]
    fn kinds_from_the_query() {
        let mut q = BTreeMap::new();
        assert_eq!(wanted(&q).len(), 3);
        q.insert("ns122".into(), "1".into());
        assert_eq!(wanted(&q), vec![KINDS[1]]);
        q.insert("ns122".into(), "0".into());
        assert_eq!(wanted(&q).len(), 3);
    }

    #[test]
    fn links_encode_the_text() {
        assert_eq!(
            results_href("six & more", Some(KINDS[0]), 20, None),
            "/w/index.php?title=Special:Search&search=six%20%26%20more&fulltext=1&ns120=1&offset=20"
        );
    }
}
