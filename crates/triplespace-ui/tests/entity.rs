//! Entity pages against a fake API serving recorded responses (`tests/fixtures/`, taken
//! from a server loaded with `crates/triplespace-server/tests/fixtures/`, plus a foreign
//! item written by hand): snapshots of each page kind and region, and their caching.
//!
//! The statement shapes (0003 §3–5) are tested against the Wikidata snapshots in
//! `docs/api/snapshots`, served as a Wikidata mirror would serve them (`WDQ65`, `WDP1082`),
//! so that the mirror's role map applies.
//!
//! Update the snapshots with `INSTA_UPDATE=always cargo test -p triplespace-ui --test entity`
//! and review the diff.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

fn fixture(name: &str) -> Option<Value> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path)
        .ok()
        .map(|t| serde_json::from_str(&t).expect("fixture parses"))
}

/// An ID as the API normalizes a title.
fn id_of(text: &str) -> String {
    let t = text.trim();
    for (prefix, keyed) in [("Item:", ""), ("Property:", ""), ("Domain:", "domain:")] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return format!("{keyed}{rest}");
        }
    }
    t.to_string()
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

fn param(q: &str, name: &str) -> Option<String> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
        .map(decode)
}

#[derive(Clone)]
struct FixtureApi {
    entities: BTreeMap<String, Value>,
}

/// Whether `s` is a Wikidata item, property or lexeme ID (`Q65`, `P1082`), or a
/// statement ID with one as its subject.
fn wikidata_id(s: &str) -> bool {
    let subject = s.split_once('$').map_or(s, |(subject, _)| subject);
    let mut chars = subject.chars();
    matches!(chars.next(), Some('Q' | 'P' | 'L'))
        && !chars.as_str().is_empty()
        && chars.as_str().bytes().all(|b| b.is_ascii_digit())
}

/// A Wikidata entity as the mirror serves it: every Wikidata ID it names, in IDs,
/// properties, keys, orders and badges, in its `WD` form (0017 §2).
fn mirrored(v: &Value, key: &str) -> Value {
    let wd = |s: &str| {
        if wikidata_id(s) {
            format!("WD{s}")
        } else {
            s.to_string()
        }
    };
    match v {
        Value::String(s) if matches!(key, "id" | "property" | "list") => Value::String(wd(s)),
        Value::Array(a) => {
            let inner = if matches!(key, "qualifiers-order" | "snaks-order" | "badges") {
                "list"
            } else {
                key
            };
            Value::Array(a.iter().map(|x| mirrored(x, inner)).collect())
        }
        Value::Object(o) => {
            let keyed = matches!(key, "claims" | "qualifiers" | "snaks");
            Value::Object(
                o.iter()
                    .filter(|(k, _)| k.as_str() != "numeric-id")
                    .map(|(k, x)| {
                        let k2 = if keyed { wd(k) } else { k.clone() };
                        (k2, mirrored(x, k))
                    })
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

impl FixtureApi {
    fn new() -> Self {
        let e = fixture("entities.json").unwrap();
        Self {
            entities: e["entities"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// The fixtures, with the Wikidata snapshots mirrored over them (the real `WDQ65`
    /// replaces the one written by hand), and an item made up for the shapes the
    /// snapshots lack.
    fn wikidata() -> Self {
        let mut api = Self::new();
        for q in ["Q65", "Q339", "Q731", "Q1520"] {
            let path = format!(
                "{}/../../docs/api/snapshots/wikidata-{q}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            api.entities
                .insert(format!("WD{q}"), mirrored(&v["entities"][q], ""));
        }
        api.entities.insert("WDQ900001".into(), made_up());
        api
    }

    fn answer(&self, path_and_query: &str) -> (StatusCode, Value, Vec<String>) {
        let (path, query) = path_and_query
            .split_once('?')
            .unwrap_or((path_and_query, ""));
        if let Some(rest) = path.strip_prefix("/w/rest.php/triplespace/v0/entity/") {
            let id = id_of(&decode(rest.trim_end_matches("/provenance")));
            return match fixture(&format!("provenance-{}.json", id.replace(':', "-"))) {
                Some(v) => (StatusCode::OK, v, vec![format!("entity:{id}")]),
                None => (
                    StatusCode::NOT_FOUND,
                    json!({"code": "entity-not-found", "message": "no entity"}),
                    vec![],
                ),
            };
        }
        if query.contains("meta=siteinfo") {
            return (StatusCode::OK, fixture("siteinfo.json").unwrap(), vec![]);
        }
        if query.contains("meta=userinfo") {
            return (
                StatusCode::OK,
                json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
                vec![],
            );
        }
        if query.contains("action=wbsearchentities") {
            return (StatusCode::OK, self.search(query), vec![]);
        }
        if query.contains("action=wbgetentities") {
            let mut out = serde_json::Map::new();
            let mut tags = Vec::new();
            for given in param(query, "ids").unwrap_or_default().split('|') {
                let id = id_of(given);
                if id.chars().next().is_some_and(char::is_lowercase) && !id.contains(':') {
                    return (
                        StatusCode::OK,
                        json!({"error": {"code": "invalid-entity-id", "info": "Invalid entity ID"}}),
                        vec![],
                    );
                }
                tags.push(format!("entity:{id}"));
                match self.entities.get(&id) {
                    Some(v) => {
                        out.insert(id, v.clone());
                    }
                    None => {
                        out.insert(given.to_string(), json!({"id": given, "missing": ""}));
                    }
                }
            }
            return (StatusCode::OK, json!({"entities": out, "success": 1}), tags);
        }
        (StatusCode::NOT_FOUND, json!({}), vec![])
    }
}

impl FixtureApi {
    /// `wbsearchentities` over the fixtures, as the API's Postgres fallback answers it: a
    /// prefix of a label (first) or an alias, in the language, `mul` or English.
    fn search(&self, query: &str) -> Value {
        let text = param(query, "search").unwrap_or_default().to_lowercase();
        let lang = param(query, "language").unwrap_or_else(|| "en".into());
        let ty = param(query, "type").unwrap_or_else(|| "item".into());
        let limit: usize = param(query, "limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(7);
        let offset: usize = param(query, "continue")
            .and_then(|l| l.parse().ok())
            .unwrap_or(0);
        let langs = [lang.as_str(), "mul", "en"];
        let mut hits = Vec::new();
        for (id, e) in &self.entities {
            if e["type"].as_str() != Some(ty.as_str()) {
                continue;
            }
            let label = langs
                .iter()
                .find_map(|l| e["labels"][*l]["value"].as_str().map(|v| (*l, v)));
            let matched = langs
                .iter()
                .find_map(|l| {
                    e["labels"][*l]["value"]
                        .as_str()
                        .filter(|v| v.to_lowercase().starts_with(&text))
                        .map(|v| ("label", *l, v))
                })
                .or_else(|| {
                    langs.iter().find_map(|l| {
                        e["aliases"][*l].as_array().and_then(|a| {
                            a.iter()
                                .filter_map(|x| x["value"].as_str())
                                .find(|v| v.to_lowercase().starts_with(&text))
                                .map(|v| ("alias", *l, v))
                        })
                    })
                });
            let Some((kind, ml, mt)) = matched else {
                continue;
            };
            let mut hit = json!({
                "id": id, "title": e["title"].as_str().unwrap_or(id),
                "display": {}, "match": {"type": kind, "language": ml, "text": mt}
            });
            if let Some((l, v)) = label {
                hit["display"]["label"] = json!({"value": v, "language": l});
            }
            if let Some(d) = langs
                .iter()
                .find_map(|l| e["descriptions"][*l]["value"].as_str().map(|v| (*l, v)))
            {
                hit["display"]["description"] = json!({"value": d.1, "language": d.0});
            }
            hits.push(hit);
        }
        let mut body = json!({"searchinfo": {"search": text}, "search": hits.iter().skip(offset).take(limit).collect::<Vec<_>>(), "success": 1});
        if hits.len() > offset + limit {
            body["search-continue"] = json!(offset + limit);
        }
        body
    }
}

impl Transport for FixtureApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let (status, body, tags) = self.answer(&request.path_and_query);
            let body = serde_json::to_vec(&body).unwrap();
            let mut headers = HeaderMap::new();
            if status == StatusCode::OK {
                let etag = format!("\"{:x}\"", body.len() * 31 + request.path_and_query.len());
                headers.insert(header::ETAG, etag.parse().unwrap());
                headers.insert(
                    header::CACHE_CONTROL,
                    "public, max-age=0, s-maxage=60".parse().unwrap(),
                );
                if !tags.is_empty() {
                    headers.insert("cache-tag", tags.join(", ").parse().unwrap());
                }
            }
            Ok(ApiResponse {
                status,
                headers,
                body,
            })
        })
    }

    fn embedded(&self) -> bool {
        false
    }
}

/// An item with an article's authors by series ordinal (a numbered list) and a property
/// whose only value is deprecated (no current value).
fn made_up() -> Value {
    let author = |n: u32| {
        json!({
            "id": format!("WDQ900001$00000000-0000-0000-0000-{n:012}"),
            "mainsnak": {"snaktype": "value", "property": "WDP2093", "datatype": "string",
                "datavalue": {"type": "string", "value": format!("Author {n}")}},
            "qualifiers": {"WDP1545": [{"snaktype": "value", "property": "WDP1545", "datatype": "string",
                "datavalue": {"type": "string", "value": n.to_string()}}]},
            "qualifiers-order": ["WDP1545"],
            "rank": "normal", "type": "statement"
        })
    };
    let authors: Vec<Value> = [12, 3, 1, 10, 2].into_iter().map(author).collect();
    json!({
        "type": "item", "id": "WDQ900001", "title": "Item:WDQ900001", "ns": 120,
        "labels": {"en": {"language": "en", "value": "A made-up article"}},
        "claims": {
            "WDP2093": authors,
            "WDP31": [{
                "id": "WDQ900001$00000000-0000-0000-0000-00000000aaaa",
                "mainsnak": {"snaktype": "value", "property": "WDP31", "datatype": "wikibase-item",
                    "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "WDQ5"}}},
                "qualifiers": {"WDP2241": [{"snaktype": "value", "property": "WDP2241", "datatype": "wikibase-item",
                    "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "WDQ6"}}}]},
                "qualifiers-order": ["WDP2241"],
                "rank": "deprecated", "type": "statement"
            }]
        }
    })
}

async fn get(uri: &str) -> (StatusCode, HeaderMap, String) {
    get_from(FixtureApi::new(), uri).await
}

async fn get_from(api: FixtureApi, uri: &str) -> (StatusCode, HeaderMap, String) {
    let req = Request::get(uri)
        .header(header::HOST, "librarybase.org")
        .body(Body::empty())
        .unwrap();
    let r = triplespace_ui::router(Client::new(api))
        .oneshot(req)
        .await
        .unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

/// The page's `<main>`, which is what these snapshots are about; the frame has its own
/// tests.
fn main_of(html: &str) -> String {
    let start = html.find("<main").expect("a <main>");
    let end = html.find("</main>").expect("a </main>") + "</main>".len();
    html[start..end].replace("><", ">\n<")
}

#[tokio::test]
async fn a_local_item_reads_without_javascript() {
    let (status, h, html) = get("/wiki/Item:Q6").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Six &#38; more (Q6) – librarybase</title>"));
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    let tags = h["cache-tag"].to_str().unwrap();
    assert!(tags.contains("entity:Q6"), "{tags}");
    assert!(
        tags.contains("entity:P2"),
        "the labels' tags are the page's too: {tags}"
    );
    assert!(h.get(header::ETAG).is_some());
    insta::assert_snapshot!("item_statements", main_of(&html));
}

#[tokio::test]
async fn item_tabs() {
    for (tab, name) in [
        ("identifiers", "item_identifiers"),
        ("labels", "item_labels"),
        ("sitelinks", "item_sitelinks"),
    ] {
        let (status, _, html) = get(&format!("/wiki/Item:Q6?tab={tab}")).await;
        assert_eq!(status, StatusCode::OK, "{tab}");
        insta::assert_snapshot!(name, main_of(&html));
    }
}

#[tokio::test]
async fn a_property_a_domain_and_a_foreign_item() {
    for (uri, name) in [
        ("/wiki/Property:P3", "property"),
        ("/wiki/Domain:wikipedia.org", "domain"),
        ("/wiki/Item:WDQ65", "foreign_item"),
        ("/wiki/Item:WDQ65?tab=sitelinks", "foreign_item_sitelinks"),
    ] {
        let (status, _, html) = get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        insta::assert_snapshot!(name, main_of(&html));
    }
    let (_, _, prop) = get("/wiki/Property:P3").await;
    assert!(
        !prop.contains("tab=sitelinks"),
        "a property has no Sitelinks tab"
    );
}

#[tokio::test]
async fn regions_are_the_pages_own_parts() {
    let (_, _, page) = get("/wiki/Item:Q6").await;
    let (_, _, identifiers_tab) = get("/wiki/Item:Q6?tab=identifiers").await;
    for (region, name) in [
        ("statements/P2", "region_statements_p2"),
        ("statements/P99", "region_statements_empty"),
        ("terms", "region_terms"),
        ("identifiers", "region_identifiers"),
    ] {
        let (status, h, html) = get(&format!(
            "/w/index.php?title=Item:Q6&action=render&region={region}"
        ))
        .await;
        assert_eq!(status, StatusCode::OK, "{region}");
        assert!(!html.contains("<html"), "{region} has no frame");
        assert!(
            h[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .starts_with("public")
        );
        let holder = if region == "identifiers" {
            &identifiers_tab
        } else {
            &page
        };
        if region != "statements/P99" {
            assert!(
                holder.contains(&html),
                "{region} is drawn on its page as served alone"
            );
        }
        insta::assert_snapshot!(name, html.replace("><", ">\n<"));
    }
    let (status, _, _) = get("/wiki/Item:Q6?action=render&region=nonsense").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let req = Request::get("/wiki/Item:Q6?action=render&region=terms")
        .header(header::HOST, "librarybase.org")
        .header("x-triplespace-ui-build", "an-older-build")
        .body(Body::empty())
        .unwrap();
    let r = triplespace_ui::router(Client::new(FixtureApi::new()))
        .oneshot(req)
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::CONFLICT,
        "build skew reloads instead of swapping"
    );
}

#[tokio::test]
async fn missing_and_invalid_entities_are_404() {
    for uri in ["/wiki/Item:Q404", "/wiki/Item:bogus"] {
        let (status, _, html) = get(uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(html.contains("There is no entity at"), "{uri}");
    }
}

#[tokio::test]
async fn uselang_sets_language_direction_and_labels() {
    let (_, _, html) = get("/wiki/Item:Q6?uselang=ar").await;
    assert!(html.contains("<html class=\"client-nojs\" lang=\"ar\" dir=\"rtl\">"));
    assert!(html.contains(">ستة</h1>"), "the Arabic label");
    let (_, _, de) = get("/wiki/Item:Q6?uselang=de").await;
    assert!(de.contains(">Sechs</h1>"));
    assert!(
        de.contains(">Zahl</a>"),
        "linked labels follow the reader's language too"
    );
    let (_, _, fallback) = get("/wiki/Item:WDQ65?uselang=de").await;
    assert!(
        fallback.contains("lang=\"en\" dir=\"auto\">Los Angeles</h1>"),
        "an English fallback is marked"
    );
}

/// One group of a mirrored Wikidata item, as `action=render` serves it.
async fn wd_group(q: &str, p: &str) -> String {
    let (status, _, html) = get_from(
        FixtureApi::wikidata(),
        &format!("/w/index.php?title=Item:WD{q}&action=render&region=statements/WD{p}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{q} {p}");
    html
}

#[tokio::test]
async fn los_angeles_population_is_a_series_with_a_chart_and_a_table() {
    let html = wd_group("Q65", "P1082").await;
    assert!(html.contains("data-shape=\"series\""));
    assert!(html.contains("<svg class=\"ts-chart\""), "the chart");
    assert!(html.contains("ts-views__panel--b"), "a table to switch to");
    assert!(
        html.contains("<span class=\"ts-subtle\">in "),
        "when the headline was measured"
    );
    insta::assert_snapshot!("shape_series", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn los_angeles_mayors_are_a_timeline() {
    let html = wd_group("Q65", "P6").await;
    assert!(html.contains("data-shape=\"timeline\""));
    assert!(html.contains("<svg class=\"ts-axis\""));
    assert!(html.contains("class=\"ts-period\">since ") || html.contains("class=\"ts-period\">"));
    assert!(html.contains("Best value"), "the best value leads");
    insta::assert_snapshot!("shape_timeline", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn pluto_folds_its_other_classes_and_the_deprecated_planet() {
    let html = wd_group("Q339", "P31").await;
    assert!(html.contains(">5 other values</summary>"), "{html}");
    assert!(html.contains(">1 deprecated</summary>"));
    assert!(html.contains("ts-statement--deprecated"));
    assert!(
        html.contains("title=\"Queries and infoboxes return only this value\""),
        "the badge's tooltip"
    );
    let best = html.find("ts-chip--best").unwrap();
    assert!(
        best < html.find("other values").unwrap(),
        "the best value leads"
    );
    insta::assert_snapshot!("shape_best_and_deprecated", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn manganese_ionic_radii_are_a_table_with_a_matrix() {
    let html = wd_group("Q731", "P10685").await;
    assert!(html.contains("data-shape=\"table\""));
    assert!(html.contains(">Matrix</label>"));
    assert!(html.contains("<table class=\"cdx-table__table\">"));
    assert!(
        html.contains("<span class=\"ts-visually-hidden\">"),
        "repeated cells are hidden but kept"
    );
    insta::assert_snapshot!("shape_table_matrix", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn large_groups_show_ten_values_then_the_rest() {
    let html = wd_group("Q731", "P2877").await;
    assert!(html.contains("data-shape=\"chips\""));
    assert!(html.contains(">Show 87 more values</summary>"));
    let names = wd_group("Q1520", "P1448").await;
    assert!(names.contains("data-shape=\"timeline\""));
    assert!(names.contains(">Show 18 more values</summary>"));
}

#[tokio::test]
async fn authors_by_ordinal_are_a_numbered_list_and_deprecated_only_is_no_current_value() {
    let html = wd_group("Q900001", "P2093").await;
    assert!(html.contains("<ol class=\"ts-numbered\">"));
    let order: Vec<usize> = [
        "value=\"1\"",
        "value=\"2\"",
        "value=\"3\"",
        "value=\"10\"",
        "value=\"12\"",
    ]
    .iter()
    .map(|v| html.find(v).unwrap_or_else(|| panic!("{v} in {html}")))
    .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "10 after 3, not after 1"
    );
    let none = wd_group("Q900001", "P31").await;
    assert!(none.contains(">No current value</p>"));
    assert!(none.contains("<details class=\"ts-fold ts-fold--deprecated\" open>"));
    assert!(
        none.contains("ts-deprecated__reason"),
        "labeled with its reason"
    );
}

#[tokio::test]
async fn every_mirrored_group_renders_on_its_page() {
    let (status, _, html) = get_from(FixtureApi::wikidata(), "/wiki/Item:WDQ731").await;
    assert_eq!(status, StatusCode::OK);
    let region = wd_group("Q731", "P10685").await;
    assert!(
        html.contains(&region),
        "the region is drawn on its page as served alone"
    );
}

/// Serves the fixture site, the mirrored Wikidata snapshots included, for screenshots
/// and axe: `TS_PREVIEW=127.0.0.1:8099 cargo test -p triplespace-ui --test entity
/// preview -- --ignored`, then open `/wiki/Item:WDQ65`.
#[tokio::test]
#[ignore = "serves until stopped"]
async fn preview() {
    let addr = std::env::var("TS_PREVIEW").unwrap_or_else(|_| "127.0.0.1:8099".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        triplespace_ui::router(Client::new(FixtureApi::wikidata())),
    )
    .await
    .unwrap();
}

/// The redirect a request answers with, if any.
async fn location(uri: &str) -> (StatusCode, Option<String>) {
    let (status, h, _) = get(uri).await;
    (
        status,
        h.get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string()),
    )
}

#[tokio::test]
async fn the_header_box_goes_straight_to_an_entity() {
    for (text, to) in [
        ("Q6", "/wiki/Item:Q6"),
        ("q6", "/wiki/Item:Q6"),
        ("P3", "/wiki/Property:P3"),
        ("Item:Q6", "/wiki/Item:Q6"),
        ("WDQ65", "/wiki/Item:WDQ65"),
        ("wikipedia.org", "/wiki/Domain:wikipedia.org"),
        ("Domain:Wikipedia.org", "/wiki/Domain:wikipedia.org"),
    ] {
        let (status, loc) = location(&format!(
            "/w/index.php?title=Special:Search&search={}",
            text.replace(':', "%3A")
        ))
        .await;
        assert_eq!(status, StatusCode::FOUND, "{text}");
        assert_eq!(loc.as_deref(), Some(to), "{text}");
    }
}

#[tokio::test]
async fn search_results_are_grouped_by_kind() {
    let (status, h, html) = get("/w/index.php?title=Special:Search&search=six&fulltext=1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    assert!(html.contains("<title>Search – librarybase</title>"));
    assert!(html.contains("href=\"/wiki/Item:Q6\" class=\"ts-hit__title\""));
    assert!(html.contains(">Items</h2>"));
    assert!(
        !html.contains(">Properties</h2>"),
        "a kind with no results is left out"
    );
    assert!(html.contains("value=\"six\""), "the boxes keep the text");
    insta::assert_snapshot!("search_six", main_of(&html));

    let (_, _, alias) = get("/w/index.php?title=Special:Search&search=half&fulltext=1").await;
    assert!(alias.contains("Also known as <bdi><span>half dozen</span></bdi>"));

    let (_, _, props) = get("/w/index.php?title=Special:Search&search=in&fulltext=1&ns122=1").await;
    assert!(props.contains(">Properties</h2>"));
    assert!(props.contains(">instance of<"));
    assert!(!props.contains(">Items</h2>"));
    assert!(props.contains("aria-current=\"page\">Properties</a>"));
}

#[tokio::test]
async fn an_id_is_offered_or_reported_missing() {
    let (_, _, html) = get("/w/index.php?title=Special:Search&search=Q6&fulltext=1").await;
    assert!(html.contains(">Go to Item:Q6</a>"));
    let (status, _, missing) = get("/w/index.php?title=Special:Search&search=Q404").await;
    assert_eq!(status, StatusCode::OK);
    assert!(missing.contains("There is no entity at Item:Q404."));
    let (status, _, word) = get("/w/index.php?title=Special:Search&search=Paris").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a word is searched, not taken for an ID"
    );
    assert!(!word.contains("There is no entity"));
    assert!(word.contains("No items, properties or domains match “Paris”."));
    let (_, _, empty) = get("/wiki/Special:Search").await;
    assert!(empty.contains("class=\"ts-search-intro\""));
}

#[tokio::test]
async fn search_pages_through_one_kind() {
    let (_, _, first) =
        get("/w/index.php?title=Special:Search&search=&fulltext=1&ns122=1&limit=5").await;
    assert!(
        first.contains("class=\"ts-search-intro\""),
        "no text, no search"
    );
    let (_, _, page1) =
        get("/w/index.php?title=Special:Search&search=p&fulltext=1&ns122=1&limit=1").await;
    assert!(page1.contains("rel=\"next\""), "{page1}");
    assert!(!page1.contains("rel=\"prev\""));
    let (_, _, page2) =
        get("/w/index.php?title=Special:Search&search=p&fulltext=1&ns122=1&limit=1&offset=1").await;
    assert!(page2.contains("rel=\"prev\""));
}
