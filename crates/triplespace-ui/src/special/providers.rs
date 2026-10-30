//! `Special:Providers` (0022 §11; 19 §5): the data providers this instance knows, from
//! `siprop=providers`: each one's chip and name, the graph its mirror is written to, and
//! for each entity type it mints, the prefix its IDs carry here, the namespace their pages
//! are in and the concept IRI they stand for.
//!
//! The live columns 19 §5 also gives the page (trust mode, last verified checkpoint, set
//! size, queue and lag, and the page repositories) read `GET /provider/{code}/mirror` and
//! `GET /repo/{name}` (18 §3.2), which the API does not serve yet.

use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::ProviderInfo;

use crate::Site;
use crate::codex::{Chip, ChipKind};
use crate::frame::Page;
use crate::html::esc;
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};

const TITLE: &str = "Special:Providers";

/// `GET Special:Providers`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let body = body(&cx.m, &cx.site.providers);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-providers-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

fn body(m: &Messages, providers: &[ProviderInfo]) -> String {
    let mut s = format!("<p>{}</p>", esc(&m.get("ts-providers-intro")));
    if providers.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-providers-none"))
        );
        return s;
    }
    let _ = write!(
        s,
        "<table class=\"cdx-table__table ts-providers\"><thead><tr><th scope=\"col\">{}</th><th scope=\"col\">{}</th><th scope=\"col\">{}</th></tr></thead><tbody>",
        esc(&m.get("ts-providers-col-provider")),
        esc(&m.get("ts-providers-col-graph")),
        esc(&m.get("ts-providers-col-types")),
    );
    for p in providers {
        let chip = Chip::new(ChipKind::Provider(p.code.clone()), &p.code)
            .render()
            .unwrap_or_default();
        let mut types = String::from("<ul class=\"ts-providers__types\">");
        for t in &p.types {
            let namespace = t.namespace.clone().unwrap_or_default();
            let _ = write!(
                types,
                "<li>{}</li>",
                esc(&m.with(
                    "ts-providers-type",
                    &[
                        &t.entity_type,
                        &format!("{}{}", p.code, t.code),
                        &namespace,
                        &t.iri,
                    ],
                ))
            );
        }
        types.push_str("</ul>");
        if p.types.is_empty() {
            types = String::new();
        }
        let _ = write!(
            s,
            "<tr><td>{chip} {}</td><td><code>mirror/{}</code></td><td>{types}</td></tr>",
            esc(&p.name),
            esc(&p.slug),
        );
    }
    s.push_str("</tbody></table>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::ProviderType;

    #[test]
    fn each_type_names_its_prefix_namespace_and_iri() {
        let m = Messages::for_language("en");
        let wd = ProviderInfo {
            code: "WD".into(),
            slug: "wikidata".into(),
            name: "Wikidata".into(),
            types: vec![ProviderType {
                code: "Q".into(),
                entity_type: "item".into(),
                iri: "http://www.wikidata.org/entity/{upstream_id}".into(),
                upstream_prefix: "Q".into(),
                namespace: Some("Item".into()),
            }],
            chip: None,
        };
        let html = body(&m, &[wd]);
        assert!(
            html.contains("Wikidata</td><td><code>mirror/wikidata</code>"),
            "{html}"
        );
        assert!(html.contains("WDQ"), "{html}");
        assert!(html.contains("Item"), "{html}");
        assert!(html.contains("http://www.wikidata.org/entity/{upstream_id}"));
        assert!(body(&m, &[]).contains("ts-empty"));
    }
}
