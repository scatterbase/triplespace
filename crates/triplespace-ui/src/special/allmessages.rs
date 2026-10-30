//! `Special:AllMessages` (0047 §9; 0095 §8): the interface messages, each with the text
//! the build ships and, where the wiki overrides it, the wiki's text and a link to the
//! Interface page that holds it, from `meta=allmessages&amprop=default`.
//!
//! MediaWiki's parameters: `prefix` (the start of the key), `filter` (`all`, `modified`,
//! `unmodified`), `lang` (the viewer's interface language by default) and `limit`; and
//! `from`, the first key shown, which the **Next page** link carries.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::{INTERFACE, Namespaces};
use triplespace_client::{MessageQuery, MessageRow};

use crate::Site;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n;
use crate::pages::{self, Context, Params, Peer};

const TITLE: &str = "Special:AllMessages";
const LIMITS: &[usize] = &[20, 50, 100, 250, 500, 5000];
const DEFAULT_LIMIT: usize = 50;

/// What the page was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    prefix: String,
    filter: &'static str,
    lang: String,
    limit: usize,
    from: String,
}

impl Ask {
    fn of(q: &BTreeMap<String, String>, viewer_lang: &str) -> Self {
        let text = |k: &str| q.get(k).map(|v| v.trim().to_string()).unwrap_or_default();
        Self {
            prefix: text("prefix"),
            filter: match q.get("filter").map(String::as_str) {
                Some("modified") => "modified",
                Some("unmodified") => "unmodified",
                _ => "all",
            },
            lang: q
                .get("lang")
                .and_then(|l| i18n::valid_code(l.trim()))
                .unwrap_or_else(|| viewer_lang.to_string()),
            limit: q
                .get("limit")
                .and_then(|l| l.trim().parse::<usize>().ok())
                .filter(|l| LIMITS.contains(l))
                .unwrap_or(DEFAULT_LIMIT),
            from: text("from"),
        }
    }

    /// The query of this page starting at another key.
    fn href_from(&self, from: &str) -> String {
        let mut q = BTreeMap::new();
        q.insert("title", TITLE.to_string());
        if !self.prefix.is_empty() {
            q.insert("prefix", self.prefix.clone());
        }
        if self.filter != "all" {
            q.insert("filter", self.filter.to_string());
        }
        q.insert("lang", self.lang.clone());
        if self.limit != DEFAULT_LIMIT {
            q.insert("limit", self.limit.to_string());
        }
        q.insert("from", from.to_string());
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(q)
            .finish();
        format!("/w/index.php?{query}")
    }
}

/// The Interface page that overrides a message in a language: `Interface:{Key}`, with
/// `/{lang}` unless the language is the wiki's own (0095 §2).
fn interface_title(key: &str, lang: &str, wiki_lang: &str) -> String {
    let ns = Namespaces::embedded()
        .get(INTERFACE)
        .and_then(|n| n.name.clone().or_else(|| Some(n.canonical.clone())))
        .unwrap_or_else(|| "Interface".to_string());
    let mut chars = key.chars();
    let key: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    if lang == wiki_lang {
        format!("{ns}:{key}")
    } else {
        format!("{ns}:{key}/{lang}")
    }
}

/// `GET Special:AllMessages`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let ask = Ask::of(&params.map, cx.m.lang());
    let query = MessageQuery {
        prefix: ask.prefix.clone(),
        customised: ask.filter.to_string(),
        lang: ask.lang.clone(),
        from: ask.from.clone(),
        ..MessageQuery::default()
    };
    let found = match site.client().all_messages(&incoming, &query).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(found.cache);
    let body = body(&cx, &ask, &found.value);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-allmessages-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

fn body(cx: &Context, ask: &Ask, rows: &[MessageRow]) -> String {
    let m = &cx.m;
    let mut s = format!(
        "<p>{}</p>{}",
        esc(&m.get("ts-allmessages-intro")),
        form(cx, ask)
    );
    let shown = &rows[..rows.len().min(ask.limit)];
    if shown.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-allmessages-none"))
        );
        return s;
    }
    let _ = write!(
        s,
        "<table class=\"cdx-table__table ts-allmessages\" lang=\"{}\"><thead><tr><th scope=\"col\">{}</th><th scope=\"col\">{}</th><th scope=\"col\">{}</th></tr></thead><tbody>",
        esc(&ask.lang),
        esc(&m.get("ts-allmessages-col-name")),
        esc(&m.get("ts-allmessages-col-default")),
        esc(&m.get("ts-allmessages-col-current")),
    );
    for r in shown {
        let page = interface_title(&r.name, &ask.lang, &cx.site.lang);
        let name = link(
            &format!("/wiki/{}", title_url(&page)),
            &format!("<code>{}</code>", esc(&r.name)),
            "",
        );
        let default = r.default.as_deref().map(esc).unwrap_or_default();
        let current = if r.customised {
            r.content.as_deref().map(esc).unwrap_or_default()
        } else {
            String::new()
        };
        let class = if r.customised {
            " class=\"ts-allmessages__customised\""
        } else {
            ""
        };
        let _ = write!(
            s,
            "<tr{class}><td>{name}</td><td>{default}</td><td>{current}</td></tr>"
        );
    }
    s.push_str("</tbody></table>");
    if let Some(next) = rows.get(ask.limit) {
        let _ = write!(
            s,
            "<p class=\"ts-allmessages__more\">{}</p>",
            link(
                &ask.href_from(&next.name),
                &esc(&m.get("ts-allmessages-next")),
                ""
            )
        );
    }
    s
}

fn form(cx: &Context, ask: &Ask) -> String {
    let m = &cx.m;
    let option = |value: &str, label: &str, on: bool| {
        format!(
            "<option value=\"{}\"{}>{}</option>",
            esc(value),
            if on { " selected" } else { "" },
            esc(label)
        )
    };
    let filters: String = [
        ("all", "ts-allmessages-filter-all"),
        ("modified", "ts-allmessages-filter-modified"),
        ("unmodified", "ts-allmessages-filter-unmodified"),
    ]
    .iter()
    .map(|(v, k)| option(v, &m.get(k), ask.filter == *v))
    .collect();
    let limits: String = LIMITS
        .iter()
        .map(|l| option(&l.to_string(), &l.to_string(), ask.limit == *l))
        .collect();
    format!(
        "<form class=\"ts-changes-form ts-allmessages__form\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"title\" value=\"{TITLE}\">\
<label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"prefix\" value=\"{}\"></label> \
<label class=\"ts-changes-form__field\"><span>{}</span> <select class=\"cdx-select\" name=\"filter\">{filters}</select></label> \
<label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"lang\" value=\"{}\" size=\"8\"></label> \
<label class=\"ts-changes-form__field\"><span>{}</span> <select class=\"cdx-select\" name=\"limit\">{limits}</select></label> \
<button class=\"cdx-button cdx-button--action-progressive\" type=\"submit\">{}</button></form>",
        esc(&m.get("ts-allmessages-prefix")),
        esc(&ask.prefix),
        esc(&m.get("ts-allmessages-filter")),
        esc(&m.get("ts-allmessages-lang")),
        esc(&ask.lang),
        esc(&m.get("ts-allmessages-limit")),
        esc(&m.get("ts-allmessages-go")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_titles_follow_0095() {
        assert_eq!(interface_title("ts-new", "en", "en"), "Interface:Ts-new");
        assert_eq!(interface_title("ts-new", "de", "en"), "Interface:Ts-new/de");
    }

    #[test]
    fn the_ask_keeps_only_what_mediawiki_takes() {
        let q: BTreeMap<String, String> = [
            ("filter", "modified"),
            ("limit", "7"),
            ("lang", "not a code"),
            ("prefix", " ts-"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
        let a = Ask::of(&q, "fr");
        assert_eq!(a.filter, "modified");
        assert_eq!(a.limit, DEFAULT_LIMIT);
        assert_eq!(a.lang, "fr");
        assert_eq!(a.prefix, "ts-");
        assert!(
            a.href_from("ts-x")
                .contains("filter=modified&from=ts-x&lang=fr&prefix=ts-")
        );
    }
}
