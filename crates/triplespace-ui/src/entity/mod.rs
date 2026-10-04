//! Entity pages (0010 §2; 0003): an item, property, foreign entity or keyed entity
//! (`Domain:wikipedia.org`), read from the API in the viewer's name.
//!
//! A page is made of regions, and `action=render` serves each region alone (0057 §8):
//!
//! | Region | Content |
//! |---|---|
//! | `terms` | The description and aliases under the title |
//! | `statements/{P}` | One statement group, also for a property the entity does not use |
//! | `identifiers` | The Identifiers tab's groups |
//! | `sitelinks` | The Sitelinks tab, grouped by host |
//! | (none) | The Statements tab: its groups and "Where this comes from" |
//!
//! Each statement group takes the shape `scatter-wikibase-shape` gives it (0003 §3):
//! Single, Timeline, Series, Table (or a numbered list, or a Matrix), Chips or List, with
//! best values leading, other and deprecated values folded, shared qualifiers stated once
//! and references as footnotes (§4–5). `shapes` draws them; `chart` draws the
//! Series chart and the Timeline axis as SVG.
//!
//! One page costs three API calls in parallel with the frame's (`wbgetentities` for the
//! entity, its provenance, and `siteinfo`), then one `wbgetentities` per 50 entities it
//! refers to, for their labels and data types.

mod chart;
mod shapes;
mod values;
mod view;

use std::collections::{BTreeMap, BTreeSet};

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::DataValue;
use serde_json::Value;
use triplespace_client::{CacheInfo, ClientError, Provenance, SiteInfo};

use scatter_wikibase_shape::Roles;

use crate::Site;
use crate::codex::MessageKind;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

/// The namespaces whose pages are entities, by prefix (`namespaces.toml`: 120, 122,
/// 210, 212, 216).
const ENTITY_NAMESPACES: &[&str] = &["Item:", "Property:", "Domain:", "Keyword:", "Notation:"];

/// Whether a title is an entity page's.
#[must_use]
pub fn is_entity_title(title: &str) -> bool {
    ENTITY_NAMESPACES
        .iter()
        .any(|p| title.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

/// The entity type an ID's shape implies; `item` where it implies none.
#[must_use]
pub fn entity_type_of(id: &str) -> String {
    EntityId::parse(id)
        .ok()
        .and_then(|e| e.implied_entity_type().map(str::to_string))
        .unwrap_or_else(|| "item".to_string())
}

/// The page title of an entity (0017 §1; `namespaces.toml`).
#[must_use]
pub fn page_title(id: &str, entity_type: &str) -> String {
    if let Some((t, key)) = id.split_once(':') {
        return match t {
            "domain" => format!("Domain:{key}"),
            "keyword" => format!("Keyword:{key}"),
            "notation" => format!("Notation:{key}"),
            _ => id.to_string(),
        };
    }
    match entity_type {
        "property" => format!("Property:{id}"),
        _ => format!("Item:{id}"),
    }
}

/// The URL of an entity's page.
#[must_use]
pub fn entity_href(id: &str, entity_type: &str) -> String {
    format!("/wiki/{}", title_url(&page_title(id, entity_type)))
}

/// The local or foreign ID a concept IRI names: the tenant's own concept base, or a
/// provider's IRI template (`http://www.wikidata.org/entity/{upstream_id}` → `WDQ…`).
#[must_use]
pub fn unit_id(iri: &str, site: &SiteInfo) -> Option<String> {
    if !site.concept_base.is_empty()
        && let Some(id) = iri.strip_prefix(&site.concept_base)
        && EntityId::parse(id).is_ok()
    {
        return Some(id.to_string());
    }
    for p in &site.providers {
        for t in &p.types {
            let Some((head, tail)) = t.iri.split_once("{upstream_id}") else {
                continue;
            };
            let Some(upstream) = iri.strip_prefix(head).and_then(|r| r.strip_suffix(tail)) else {
                continue;
            };
            let Some(rest) = upstream.strip_prefix(&t.upstream_prefix) else {
                continue;
            };
            let id = format!("{}{}{rest}", p.code, t.code);
            if !rest.is_empty() && EntityId::parse(&id).is_ok() {
                return Some(id);
            }
        }
    }
    None
}

/// The labels and data types of the entities a page refers to.
#[derive(Debug, Clone, Default)]
pub struct Lookup {
    /// Each entity's label in the reader's term chain, with its language.
    labels: BTreeMap<String, (String, String)>,
    /// Each property's data type.
    datatypes: BTreeMap<String, String>,
}

impl Lookup {
    /// From `wbgetentities` results, choosing each label along `chain`.
    #[must_use]
    pub fn from_entities(found: &BTreeMap<String, Value>, chain: &[String]) -> Self {
        let mut out = Self::default();
        for (id, v) in found {
            if let Some(labels) = v["labels"].as_object()
                && let Some((lang, text)) = chain.iter().find_map(|l| {
                    labels
                        .get(l)
                        .and_then(|t| t["value"].as_str())
                        .map(|t| (l.clone(), t.to_string()))
                })
            {
                out.labels.insert(id.clone(), (text, lang));
            }
            if let Some(dt) = v["datatype"].as_str() {
                out.datatypes.insert(id.clone(), dt.to_string());
            }
        }
        out
    }

    /// An entity's label and its language.
    #[must_use]
    pub fn label(&self, id: &str) -> Option<(&str, &str)> {
        self.labels.get(id).map(|(t, l)| (t.as_str(), l.as_str()))
    }

    /// A property's data type.
    #[must_use]
    pub fn datatype(&self, id: &str) -> Option<&str> {
        self.datatypes.get(id).map(String::as_str)
    }
}

/// What the views render from.
pub struct Render<'a> {
    /// The interface messages.
    pub m: &'a Messages,
    /// The site.
    pub site: &'a SiteInfo,
    /// Labels and data types of what the entity refers to.
    pub lookup: &'a Lookup,
    /// The entity.
    pub entity: &'a Entity,
    /// Where it comes from, if the API said.
    pub provenance: Option<&'a Provenance>,
    /// The role map shape detection reads (0003 §7).
    pub roles: &'a Roles,
}

/// The role map: Wikidata's roles in their mirrored form for each provider that copies
/// Wikidata's properties (`WDP585`). A local property plays a role only where the
/// instance maps it, which the API does not report yet, so local groups take the shapes
/// that need no role (Table, Chips, List).
#[must_use]
pub fn site_roles(site: &SiteInfo) -> Roles {
    let mut roles = Roles::empty();
    for p in &site.providers {
        if p.slug == "wikidata" {
            roles = roles.with_mirror(&p.code);
        }
    }
    roles
}

/// The IDs a page needs labels for: properties and entity values in statements,
/// qualifiers and references, units and globes, and sitelink badges.
fn referenced(entity: &Entity, site: &SiteInfo) -> Vec<String> {
    let mut ids = BTreeSet::new();
    let snak = |s: &Snak, ids: &mut BTreeSet<String>| {
        ids.insert(s.property.as_str().to_string());
        if let SnakKind::Value(v) = &s.kind {
            match v {
                DataValue::EntityId(e) => {
                    ids.insert(e.id.as_str().to_string());
                }
                DataValue::Quantity(q) if q.unit != "1" => {
                    if let Some(id) = unit_id(&q.unit, site) {
                        ids.insert(id);
                    }
                }
                DataValue::GlobeCoordinate(g) => {
                    if let Some(id) = unit_id(&g.globe, site) {
                        ids.insert(id);
                    }
                }
                _ => {}
            }
        }
    };
    for st in entity.all_statements() {
        for s in st.snaks() {
            snak(s, &mut ids);
        }
    }
    for link in entity.sitelinks.values() {
        for b in &link.badges {
            ids.insert(b.as_str().to_string());
        }
    }
    ids.remove(entity.id.as_str());
    ids.into_iter().collect()
}

/// The tab a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabName {
    Statements,
    Identifiers,
    Sitelinks,
    Labels,
}

impl TabName {
    fn of(query: &BTreeMap<String, String>) -> Self {
        match query.get("tab").map(String::as_str) {
            Some("identifiers") => Self::Identifiers,
            Some("sitelinks") => Self::Sitelinks,
            Some("labels") => Self::Labels,
            _ => Self::Statements,
        }
    }
}

fn tabs(m: &Messages, title: &str, current: TabName, items: bool) -> Vec<Tab> {
    let base = format!("/wiki/{}", title_url(title));
    let tab = |name: TabName, key: &str, href: String| Tab {
        label: m.get(key),
        href,
        current: name == current,
    };
    let mut v = vec![
        tab(TabName::Statements, "ts-tab-statements", base.clone()),
        tab(
            TabName::Identifiers,
            "ts-tab-identifiers",
            format!("{base}?tab=identifiers"),
        ),
    ];
    // Only items have sitelinks (wikibase-compat §3.1).
    if items {
        v.push(tab(
            TabName::Sitelinks,
            "ts-tab-sitelinks",
            format!("{base}?tab=sitelinks"),
        ));
    }
    v.push(tab(
        TabName::Labels,
        "ts-tab-labels",
        format!("{base}?tab=labels"),
    ));
    // History joins when the API serves it and the site has built it (Feature::History).
    v
}

/// A not-found page for an entity title: the frame, a 404, and the API's reason where it
/// gave one.
fn missing(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    let title = title.to_string();
    pages::respond(cx, headers, StatusCode::NOT_FOUND, move |m| Page {
        returnto: Some(title.clone()),
        body: pages::message(
            MessageKind::Warning,
            &m.with("ts-entity-missing", &[&title]),
        ),
        title,
        ..Page::default()
    })
}

/// Serves an entity title: the page with a tab, or with `action=render` a region.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: String,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let client = site.client();
    let one = [title.clone()];
    let (cx, fetched, provenance) = tokio::join!(
        pages::context(site, &incoming, query),
        client.entities(&incoming, &one, "", ""),
        client.provenance(&incoming, &title),
    );
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let render = query.get("action").is_some_and(|a| a == "render");
    let fetched = match fetched {
        Ok(f) => f,
        Err(ClientError::Api { code, .. })
            if code == "invalid-entity-id" || code == "no-such-entity" =>
        {
            return if render {
                pages::fragment(&cx, headers, StatusCode::NOT_FOUND, String::new)
            } else {
                missing(&cx, headers, &title)
            };
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(fetched.cache.clone());
    let Some((_, json)) = fetched.value.found.into_iter().next() else {
        return if render {
            pages::fragment(&cx, headers, StatusCode::NOT_FOUND, String::new)
        } else {
            missing(&cx, headers, &title)
        };
    };
    let parsed = match Entity::from_value(json) {
        Ok(p) => p,
        Err(e) => {
            return pages::api_failed(
                &cx,
                headers,
                &ClientError::Unexpected(format!("the entity's JSON: {e}")),
            );
        }
    };
    let entity = parsed.entity;
    let provenance = match provenance {
        Ok(p) => {
            cx.inputs.push(p.cache);
            p.value
        }
        Err(e) => {
            eprintln!("triplespace-ui: provenance of {title}: {e}");
            cx.inputs.push(CacheInfo::default());
            None
        }
    };
    let chain = cx.m.term_chain();
    let ids = referenced(&entity, &cx.site);
    let lookup = if ids.is_empty() {
        Lookup::default()
    } else {
        match client
            .entities(&incoming, &ids, "labels|datatype", &chain.join("|"))
            .await
        {
            Ok(f) => {
                cx.inputs.push(f.cache);
                Lookup::from_entities(&f.value.found, &chain)
            }
            Err(e) => {
                eprintln!("triplespace-ui: labels for {title}: {e}");
                cx.inputs.push(CacheInfo::default());
                Lookup::default()
            }
        }
    };
    let roles = site_roles(&cx.site);
    let r = Render {
        m: &cx.m,
        site: &cx.site,
        lookup: &lookup,
        entity: &entity,
        provenance: provenance.as_ref(),
        roles: &roles,
    };
    if render {
        region(&r, &cx, headers, query)
    } else {
        page(&r, &cx, headers, query)
    }
}

/// `action=render`: one region, or the Statements tab without the frame (0057 §8).
fn region(
    r: &Render<'_>,
    cx: &Context,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
) -> Response {
    if headers
        .get("x-triplespace-ui-build")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|b| b != crate::assets::build_id())
    {
        return pages::build_skew();
    }
    let html = match query.get("region").map(String::as_str) {
        None => r.statements_tab(),
        Some("terms") => r.terms(),
        Some("identifiers") => r.identifiers(),
        Some("sitelinks") => r.sitelinks(),
        Some(region) => match region.strip_prefix("statements/") {
            Some(p) if EntityId::parse(p).is_ok() => r.group(p),
            _ => return pages::unknown_region(region),
        },
    };
    pages::fragment(cx, headers, StatusCode::OK, move || html)
}

/// The page in the frame, with the tab the request asks for.
fn page(
    r: &Render<'_>,
    cx: &Context,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
) -> Response {
    let entity = r.entity;
    let canonical_title = page_title(entity.id.as_str(), entity.entity_type.name());
    let tab = TabName::of(query);
    let body = match tab {
        TabName::Statements => r.statements_tab(),
        TabName::Identifiers => r.identifiers(),
        TabName::Sitelinks => r.sitelinks(),
        TabName::Labels => r.labels(),
    };
    let title_lang = r
        .label()
        .map(|(_, l)| l.to_string())
        .filter(|l| l != cx.m.lang());
    let tabs = tabs(
        &cx.m,
        &canonical_title,
        tab,
        entity.entity_type.name() == "item",
    );
    let (display, identity, terms, doc_title) = (
        r.display_title(),
        r.identity(),
        r.terms(),
        r.document_title(),
    );
    pages::respond(cx, headers, StatusCode::OK, move |_| Page {
        title: display,
        title_lang,
        doc_title: Some(doc_title),
        returnto: Some(canonical_title),
        identity_html: Some(identity),
        subtitle_html: Some(terms),
        tabs,
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::{ProviderInfo, ProviderType};

    fn site() -> SiteInfo {
        SiteInfo {
            concept_base: "https://librarybase.org/entity/".into(),
            providers: vec![ProviderInfo {
                code: "WD".into(),
                slug: "wikidata".into(),
                name: "Wikidata".into(),
                types: vec![ProviderType {
                    code: "Q".into(),
                    entity_type: "item".into(),
                    iri: "http://www.wikidata.org/entity/{upstream_id}".into(),
                    upstream_prefix: "Q".into(),
                }],
                chip: None,
            }],
            ..SiteInfo::default()
        }
    }

    #[test]
    fn titles_and_links() {
        assert!(is_entity_title("Item:Q6"));
        assert!(is_entity_title("Domain:wikipedia.org"));
        assert!(!is_entity_title("Item:"));
        assert!(!is_entity_title("Main Page"));
        assert_eq!(page_title("P12", "property"), "Property:P12");
        assert_eq!(page_title("WDQ65", "item"), "Item:WDQ65");
        assert_eq!(
            page_title("domain:wikipedia.org", "domain"),
            "Domain:wikipedia.org"
        );
        assert_eq!(entity_href("Q6", "item"), "/wiki/Item:Q6");
        assert_eq!(entity_type_of("WDP31"), "property");
        assert_eq!(entity_type_of("domain:x.org"), "domain");
    }

    #[test]
    fn units_resolve_to_local_and_foreign_ids() {
        let s = site();
        assert_eq!(
            unit_id("https://librarybase.org/entity/Q9", &s).as_deref(),
            Some("Q9")
        );
        assert_eq!(
            unit_id("http://www.wikidata.org/entity/Q11573", &s).as_deref(),
            Some("WDQ11573")
        );
        assert_eq!(unit_id("http://example.org/unit/metre", &s), None);
    }

    #[test]
    fn labels_follow_the_chain() {
        let found: BTreeMap<String, Value> = [(
            "Q1".to_string(),
            serde_json::json!({"labels": {"en": {"value": "one"}, "mul": {"value": "uno"}}, "datatype": "string"}),
        )]
        .into();
        let l = Lookup::from_entities(&found, &["de".into(), "mul".into(), "en".into()]);
        assert_eq!(l.label("Q1"), Some(("uno", "mul")));
        assert_eq!(l.datatype("Q1"), Some("string"));
    }
}
