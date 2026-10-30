//! `Special:AllPages` and `Special:PrefixIndex` (0047 §4.4; 21 §2.4): a namespace's
//! document pages in title order, from `list=allpages`, with MediaWiki's parameters.
//!
//! - **`AllPages`**: `from` (or the subpage), `to`, `namespace`, `hideredirects`.
//! - **`PrefixIndex`**: `prefix` (or the subpage), `namespace`, `hideredirects`,
//!   `stripprefix`, and `from` to continue.
//!
//! A title given with a namespace (`Special:PrefixIndex/Project:Help`) picks that
//! namespace. Titles are listed without the namespace, in columns, a redirect in italics,
//! which a second, standard call finds (`apfilterredir=redirects` over the same titles),
//! since `list=allpages` does not mark redirects. **Previous page** and **Next page**
//! links walk the list. The entity namespaces hold entities, not pages (0008 §1), so they
//! are not offered; asked for one, the page says so and points to search.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::{Kind as NsKind, Namespaces};
use triplespace_client::{ClientError, Incoming, NamespaceRow, PageListQuery};

use crate::Site;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};

/// Titles to a page, as MediaWiki's `AllPages` shows.
const LIMIT: u32 = 345;

/// The two pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:AllPages`.
    AllPages,
    /// `Special:PrefixIndex`.
    PrefixIndex,
}

impl Which {
    /// The page a special page's name names.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "AllPages" => Some(Self::AllPages),
            "PrefixIndex" => Some(Self::PrefixIndex),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::AllPages => "Special:AllPages",
            Self::PrefixIndex => "Special:PrefixIndex",
        }
    }
}

/// What the page was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    which: Which,
    namespace: i64,
    from: String,
    to: String,
    prefix: String,
    hide_redirects: bool,
    strip_prefix: bool,
}

/// A namespace by any name a reader may type: local, canonical or alias.
fn namespace_named(rows: &[NamespaceRow], name: &str) -> Option<i64> {
    let name = name.replace('_', " ");
    let name = name.trim();
    rows.iter()
        .find(|n| {
            n.id != 0
                && (n.name.eq_ignore_ascii_case(name)
                    || n.canonical
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(name))
                    || n.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
        })
        .map(|n| n.id)
}

/// A typed title's namespace, where its prefix names one, and the rest.
fn split(rows: &[NamespaceRow], text: &str) -> (Option<i64>, String) {
    let text = text.replace('_', " ");
    if let Some((prefix, rest)) = text.split_once(':')
        && let Some(id) = namespace_named(rows, prefix)
    {
        return (Some(id), rest.trim().to_string());
    }
    (None, text.trim().to_string())
}

/// Whether the namespace holds document pages a list may show.
fn listable(id: i64) -> bool {
    i32::try_from(id)
        .ok()
        .and_then(|n| Namespaces::embedded().get(n))
        .is_some_and(|n| n.kind == NsKind::Pages && !n.is_entity_namespace())
}

impl Ask {
    fn of(which: Which, title: &str, q: &BTreeMap<String, String>, rows: &[NamespaceRow]) -> Self {
        let text = |k: &str| q.get(k).map(|v| v.replace('_', " ").trim().to_string());
        let subpage = title
            .strip_prefix("Special:")
            .and_then(|t| t.split_once('/'))
            .map(|(_, rest)| rest.to_string())
            .filter(|s| !s.trim().is_empty());
        let mut namespace = q
            .get("namespace")
            .and_then(|n| n.trim().parse::<i64>().ok())
            .unwrap_or(0);
        // The subpage (or `from`, or `prefix`) may carry a namespace, which wins.
        let lead = match which {
            Which::AllPages => subpage.clone().or_else(|| text("from")),
            Which::PrefixIndex => subpage.clone().or_else(|| text("prefix")),
        }
        .unwrap_or_default();
        let (ns, lead) = split(rows, &lead);
        if let Some(ns) = ns {
            namespace = ns;
        }
        let flag = |k: &str| q.get(k).is_some_and(|v| !v.is_empty() && v != "0");
        let (from, prefix) = match which {
            Which::AllPages => (lead, String::new()),
            Which::PrefixIndex => (text("from").unwrap_or_default(), lead),
        };
        Self {
            which,
            namespace,
            from,
            to: if which == Which::AllPages {
                text("to").map(|t| split(rows, &t).1).unwrap_or_default()
            } else {
                String::new()
            },
            prefix,
            hide_redirects: flag("hideredirects"),
            strip_prefix: flag("stripprefix"),
        }
    }

    fn query(&self, from: &str, descending: bool, limit: u32) -> PageListQuery {
        PageListQuery {
            namespace: self.namespace,
            from: from.to_string(),
            to: if descending {
                String::new()
            } else {
                self.to.clone()
            },
            prefix: self.prefix.clone(),
            filter_redirects: if self.hide_redirects {
                "nonredirects".into()
            } else {
                String::new()
            },
            descending,
            limit,
        }
    }

    /// This page starting at another title.
    fn href(&self, from: &str) -> String {
        let mut q: Vec<(&str, String)> = vec![("title", self.which.title().to_string())];
        if self.which == Which::PrefixIndex {
            q.push(("prefix", self.prefix.clone()));
        }
        q.push(("from", from.to_string()));
        if !self.to.is_empty() {
            q.push(("to", self.to.clone()));
        }
        q.push(("namespace", self.namespace.to_string()));
        if self.hide_redirects {
            q.push(("hideredirects", "1".into()));
        }
        if self.strip_prefix {
            q.push(("stripprefix", "1".into()));
        }
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(q)
            .finish();
        format!("/w/index.php?{query}")
    }
}

/// A title without its namespace's prefix.
fn text_of(title: &str, namespace: i64, rows: &[NamespaceRow]) -> String {
    if namespace == 0 {
        return title.to_string();
    }
    title
        .split_once(':')
        .filter(|(p, _)| namespace_named(rows, p) == Some(namespace))
        .map_or_else(|| title.to_string(), |(_, rest)| rest.to_string())
}

/// `GET Special:AllPages` and `Special:PrefixIndex`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Pages) {
        return pages::no_page(&cx, headers, which.title().to_string());
    }
    let rows = match site.client().namespaces(&incoming).await {
        Ok(f) => {
            cx.inputs.push(f.cache);
            f.value
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let ask = Ask::of(which, title, &params.map, &rows);
    let mut body = form(&cx.m, &ask, &rows);
    if listable(ask.namespace) {
        match list(site, &mut cx, &incoming, &ask, &rows).await {
            Ok(html) => body.push_str(&html),
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    } else {
        let _ = write!(
            body,
            "<p class=\"ts-empty\">{}</p>",
            esc(&cx.m.get("ts-allpages-not-pages"))
        );
        let _ = write!(
            body,
            "<p>{}</p>",
            link(
                "/wiki/Special:Search",
                &esc(&cx.m.get("ts-search-title")),
                ""
            )
        );
    }
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get(match which {
            Which::AllPages => "ts-allpages-title",
            Which::PrefixIndex => "ts-prefixindex-title",
        }),
        returnto: Some(which.title().to_string()),
        body,
        ..Page::default()
    })
}

/// The list, its redirects marked, and the links to the pages either side.
async fn list(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    ask: &Ask,
    rows: &[NamespaceRow],
) -> Result<String, ClientError> {
    let client = site.client();
    let found = client
        .all_pages(incoming, &ask.query(&ask.from, false, LIMIT))
        .await?;
    cx.inputs.push(found.cache);
    let titles = found.value.titles;
    if titles.is_empty() {
        return Ok(format!(
            "<p class=\"ts-empty\">{}</p>",
            esc(&cx.m.get("ts-allpages-none"))
        ));
    }
    let texts: Vec<String> = titles
        .iter()
        .map(|t| text_of(t, ask.namespace, rows))
        .collect();
    // The redirects among them: one more call over the same range.
    let mut redirects = Vec::new();
    if !ask.hide_redirects {
        let mut q = ask.query(&texts[0], false, LIMIT);
        q.to.clone_from(&texts[texts.len() - 1]);
        q.filter_redirects = "redirects".into();
        let r = client.all_pages(incoming, &q).await?;
        cx.inputs.push(r.cache);
        redirects = r.value.titles;
    }
    // Where the previous page starts: `LIMIT` titles back from this one's first.
    let mut previous = None;
    if !ask.from.is_empty() {
        let back = client
            .all_pages(incoming, &ask.query(&texts[0], true, LIMIT + 1))
            .await?;
        cx.inputs.push(back.cache);
        let earlier: Vec<&String> = back
            .value
            .titles
            .iter()
            .filter(|t| text_of(t, ask.namespace, rows) != texts[0])
            .collect();
        if let Some(start) = earlier.get(LIMIT as usize - 1).or(earlier.last()) {
            previous = Some(text_of(start, ask.namespace, rows));
        }
    }
    let m = &cx.m;
    let mut s = String::from("<ul class=\"ts-allpages\">");
    for (full, text) in titles.iter().zip(&texts) {
        let shown = if ask.strip_prefix && ask.which == Which::PrefixIndex {
            text.strip_prefix(&ask.prefix)
                .filter(|t| !t.is_empty())
                .unwrap_or(text)
        } else {
            text
        };
        let class = if redirects.contains(full) {
            " class=\"ts-allpages__redirect\""
        } else {
            ""
        };
        let _ = write!(
            s,
            "<li{class}>{}</li>",
            link(&format!("/wiki/{}", title_url(full)), &esc(shown), "")
        );
    }
    s.push_str("</ul>");
    let mut nav = Vec::new();
    if let Some(p) = &previous {
        nav.push(link(
            &ask.href(p),
            &esc(&m.with("ts-allpages-previous", &[p])),
            " rel=\"prev\"",
        ));
    }
    if let Some(n) = &found.value.next {
        nav.push(link(
            &ask.href(n),
            &esc(&m.with("ts-allpages-next", &[n])),
            " rel=\"next\"",
        ));
    }
    if !nav.is_empty() {
        let _ = write!(
            s,
            "<nav class=\"ts-pager\" aria-label=\"{}\">{}</nav>",
            esc(&m.get("ts-search-pages-label")),
            nav.join(" | ")
        );
    }
    Ok(s)
}

fn form(m: &Messages, ask: &Ask, rows: &[NamespaceRow]) -> String {
    let field = |name: &str, label: &str, value: &str| {
        format!(
            "<label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"{name}\" value=\"{}\"></label> ",
            esc(&m.get(label)),
            esc(value)
        )
    };
    let check = |name: &str, label: &str, on: bool| {
        format!(
            "<label class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" name=\"{name}\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><span class=\"cdx-checkbox__label\">{}</span></label> ",
            if on { " checked" } else { "" },
            esc(&m.get(label))
        )
    };
    let mut s = format!(
        "<form class=\"ts-changes-form ts-allpages__form\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"title\" value=\"{}\">",
        ask.which.title()
    );
    match ask.which {
        Which::AllPages => {
            s.push_str(&field("from", "ts-allpages-from", &ask.from));
            s.push_str(&field("to", "ts-allpages-to", &ask.to));
        }
        Which::PrefixIndex => {
            s.push_str(&field("prefix", "ts-prefixindex-prefix", &ask.prefix));
        }
    }
    let _ = write!(
        s,
        "<label class=\"ts-changes-form__field\"><span>{}</span> <select class=\"cdx-select\" name=\"namespace\">",
        esc(&m.get("ts-changes-namespace"))
    );
    for n in rows.iter().filter(|n| listable(n.id)) {
        let name = if n.id == 0 {
            m.get("ts-namespaceinfo-main")
        } else {
            n.name.clone()
        };
        let _ = write!(
            s,
            "<option value=\"{}\"{}>{}</option>",
            n.id,
            if n.id == ask.namespace {
                " selected"
            } else {
                ""
            },
            esc(&name)
        );
    }
    s.push_str("</select></label> ");
    s.push_str(&check(
        "hideredirects",
        "ts-allpages-hideredirects",
        ask.hide_redirects,
    ));
    if ask.which == Which::PrefixIndex {
        s.push_str(&check(
            "stripprefix",
            "ts-prefixindex-stripprefix",
            ask.strip_prefix,
        ));
    }
    let _ = write!(
        s,
        "<button class=\"cdx-button cdx-button--action-progressive\" type=\"submit\">{}</button></form>",
        esc(&m.get("ts-allpages-go"))
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<NamespaceRow> {
        vec![
            NamespaceRow {
                id: 0,
                ..NamespaceRow::default()
            },
            NamespaceRow {
                id: 4,
                name: "Librarybase".into(),
                canonical: Some("Project".into()),
                aliases: vec!["LB".into()],
                ..NamespaceRow::default()
            },
        ]
    }

    fn q(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn a_namespace_in_the_title_wins() {
        let a = Ask::of(
            Which::PrefixIndex,
            "Special:PrefixIndex/Project:Help_desk",
            &q(&[("namespace", "0")]),
            &rows(),
        );
        assert_eq!((a.namespace, a.prefix.as_str()), (4, "Help desk"));
        let a = Ask::of(
            Which::AllPages,
            "Special:AllPages",
            &q(&[("from", "LB:M"), ("hideredirects", "1")]),
            &rows(),
        );
        assert_eq!(
            (a.namespace, a.from.as_str(), a.hide_redirects),
            (4, "M", true)
        );
        let a = Ask::of(
            Which::AllPages,
            "Special:AllPages/Douglas: a life",
            &q(&[]),
            &rows(),
        );
        assert_eq!((a.namespace, a.from.as_str()), (0, "Douglas: a life"));
    }

    #[test]
    fn titles_are_shown_without_their_namespace() {
        assert_eq!(text_of("Librarybase:Home", 4, &rows()), "Home");
        assert_eq!(text_of("Douglas: a life", 0, &rows()), "Douglas: a life");
        assert!(listable(0) && listable(4) && !listable(120) && !listable(-1));
    }
}
