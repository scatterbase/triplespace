//! The renderer against Wikibase 1.43's own output (`docs/samples/wikibase-1.43`): Q8 from
//! its `Special:EntityData` JSON must produce exactly the triples `dumpRdf.php` wrote about
//! Q8 in both flavors, and every property P1–P13 the declarations Wikibase wrote for it.
//!
//! Triples are compared as sets, with every blank node read as one node, since blank node
//! labels are not part of the contract (wikibase-compat.md §5.4). The `xsd:dateTime`
//! values WikibaseEDTF derives from `edtf` values are left out of the expectation: those
//! are not written yet (wikibase-compat.md §6.4, §7 item 5).
//!
//! Tests 0001 §3 (the standard Wikibase output is unchanged; 02 §4.2).

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use oxrdf::{NamedOrBlankNode, Term, Triple};
use oxttl::TurtleParser;
use scatter_wikibase_model::json::from_wbgetentities;
use scatter_wikibase_model::{DataType, Entity, EntityId, EntityType, Hasher};
use scatter_wikibase_rdf::{
    Flavor, IriScheme, Namespaces, Produce, Renderer, SiteFacts, dump_header, property_type_name,
    to_ntriples, to_turtle,
};

const BASE: &str = "http://localhost:8080/";
const SCRIPT: &str = "http://localhost:8080/index.php/";
const EDTF: &str = "http://www.w3.org/2001/XMLSchema#edtf";

fn sample(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/wikibase-1.43")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn parse_turtle(text: &str) -> Vec<Triple> {
    TurtleParser::new()
        .for_slice(text.as_bytes())
        .map(|t| t.expect("golden Turtle parses"))
        .collect()
}

fn scheme() -> IriScheme<'static> {
    IriScheme::with_default_registries(Namespaces::new(BASE, SCRIPT))
}

/// The reference install's one site: `testwiki`, language `en`, group `wikipedia`, whose
/// global ID resolves to `http://127.0.0.1/`.
fn testwiki(link: &scatter_wikibase_model::Sitelink) -> Option<SiteFacts> {
    Some(SiteFacts {
        url: link.url.clone()?,
        language: Some("en".into()),
        part_of: Some("http://127.0.0.1/".into()),
        group: Some("wikipedia".into()),
    })
}

/// A triple with its blank nodes read as one, as an N-Triples line.
fn key(t: &Triple) -> String {
    let s = match &t.subject {
        NamedOrBlankNode::BlankNode(_) => "_:b".to_string(),
        NamedOrBlankNode::NamedNode(n) => n.to_string(),
    };
    let o = match &t.object {
        Term::BlankNode(_) => "_:b".to_string(),
        other => other.to_string(),
    };
    format!("{s} {} {o}", t.predicate)
}

fn keys(triples: &[Triple]) -> BTreeSet<String> {
    triples.iter().map(key).collect()
}

/// A derived EDTF value: on a P13 predicate, a literal other than the raw `xsd:edtf` one,
/// or a value node.
fn derived_edtf(t: &Triple) -> bool {
    t.predicate.as_str().ends_with("/P13")
        && match &t.object {
            Term::Literal(l) => l.datatype().as_str() != EDTF,
            Term::NamedNode(n) => n.as_str().starts_with(&format!("{BASE}value/")),
            Term::BlankNode(_) => false,
        }
}

/// Every golden triple about Q8: its document, entity, statements and sitelink, and the
/// reference and value nodes those reach.
fn expected_q8(golden: &[Triple]) -> BTreeSet<String> {
    let roots: HashSet<String> = [
        format!("<{SCRIPT}Special:EntityData/Q8>"),
        format!("<{BASE}entity/Q8>"),
        "<http://127.0.0.1:8080/index.php/Douglas_Adams>".into(),
        "<http://127.0.0.1/>".into(),
    ]
    .into_iter()
    .collect();
    let statement_prefix = format!("<{BASE}entity/statement/Q8-");
    let mut kept: Vec<&Triple> = golden
        .iter()
        .filter(|t| {
            let s = t.subject.to_string();
            (roots.contains(&s) || s.starts_with(&statement_prefix)) && !derived_edtf(t)
        })
        .collect();
    // Reference and value nodes reached from what is kept, transitively.
    let mut reached: HashSet<String> = HashSet::new();
    loop {
        let before = reached.len();
        for t in &kept {
            if let Term::NamedNode(n) = &t.object
                && (n.as_str().starts_with(&format!("{BASE}reference/"))
                    || n.as_str().starts_with(&format!("{BASE}value/")))
            {
                reached.insert(n.to_string());
            }
        }
        kept = golden
            .iter()
            .filter(|t| {
                let s = t.subject.to_string();
                ((roots.contains(&s) || s.starts_with(&statement_prefix)) && !derived_edtf(t))
                    || reached.contains(&s)
            })
            .collect();
        if reached.len() == before {
            break;
        }
    }
    kept.into_iter().map(key).collect()
}

fn q8() -> scatter_wikibase_model::ParsedEntity {
    from_wbgetentities(&sample("wikibase-Q8.json"))
        .expect("Q8 parses")
        .remove(0)
}

fn diff(rendered: &BTreeSet<String>, expected: &BTreeSet<String>) -> String {
    let extra: Vec<_> = rendered.difference(expected).collect();
    let missing: Vec<_> = expected.difference(rendered).collect();
    format!(
        "written but not by Wikibase: {extra:#?}\nwritten by Wikibase but not here: {missing:#?}"
    )
}

#[test]
fn q8_full_flavor_matches_wikibase() {
    let golden = parse_turtle(&sample("wikibase-rdf-full-dump.ttl"));
    let parsed = q8();
    let iris = scheme();
    let hasher = Hasher::local();
    let sites = testwiki;
    let r = Renderer::new(&iris, &hasher, Produce::DUMP_FILE).with_sites(&sites);
    let rendered = keys(&r.entity(&parsed.entity, parsed.page.as_ref()));
    let expected = expected_q8(&golden);
    assert_eq!(rendered, expected, "{}", diff(&rendered, &expected));
}

#[test]
fn q8_truthy_flavor_matches_wikibase() {
    let golden = parse_turtle(&sample("wikibase-rdf-truthy-dump.ttl"));
    let parsed = q8();
    let iris = scheme();
    let hasher = Hasher::local();
    let sites = testwiki;
    let r = Renderer::new(&iris, &hasher, Produce::TRUTHY_DUMP_FILE).with_sites(&sites);
    let rendered = keys(&r.entity(&parsed.entity, parsed.page.as_ref()));
    let expected = expected_q8(&golden);
    assert_eq!(rendered, expected, "{}", diff(&rendered, &expected));
}

/// The data types the reference install's properties use.
const DATATYPES: &[DataType] = &[
    DataType::String,
    DataType::WikibaseItem,
    DataType::WikibaseProperty,
    DataType::Time,
    DataType::Quantity,
    DataType::GlobeCoordinate,
    DataType::MonolingualText,
    DataType::Url,
    DataType::ExternalId,
    DataType::CommonsMedia,
    DataType::GeoShape,
    DataType::TabularData,
    DataType::Edtf,
];

#[test]
fn property_declarations_match_wikibase() {
    let terms = [
        "http://www.w3.org/2000/01/rdf-schema#label",
        "http://www.w3.org/2004/02/skos/core#prefLabel",
        "http://www.w3.org/2004/02/skos/core#altLabel",
        "http://schema.org/name",
        "http://schema.org/description",
    ];
    for (flavor, file) in [
        (Produce::DUMP_FILE, "wikibase-rdf-full-dump.ttl"),
        (Produce::TRUTHY_DUMP_FILE, "wikibase-rdf-truthy-dump.ttl"),
    ] {
        let golden = parse_turtle(&sample(file));
        let iris = scheme();
        let hasher = Hasher::local();
        let mut checked = 0;
        for n in 1..=13 {
            let id = format!("P{n}");
            let concept = format!("<{BASE}entity/{id}>");
            let type_iri = golden
                .iter()
                .find(|t| {
                    t.subject.to_string() == concept
                        && t.predicate.as_str() == "http://wikiba.se/ontology#propertyType"
                })
                .map_or_else(
                    || panic!("{id} has a propertyType"),
                    |t| t.object.to_string(),
                );
            let datatype = DATATYPES
                .iter()
                .find(|d| {
                    type_iri == format!("<http://wikiba.se/ontology#{}>", property_type_name(d))
                })
                .unwrap_or_else(|| panic!("{id}: no data type for {type_iri}"))
                .clone();
            let mut entity = Entity::new(EntityId::parse(&id).unwrap(), EntityType::Property);
            entity.datatype = Some(datatype);
            let r = Renderer::new(&iris, &hasher, flavor);
            let rendered: Vec<Triple> = r
                .entity(&entity, None)
                .into_iter()
                .filter(|t| !t.subject.to_string().contains("Special:EntityData"))
                .collect();

            // Wikibase's: every triple on the concept or on one of the property's
            // predicates, and the novalue restriction, without terms or statements.
            let predicate_suffix = format!("/{id}>");
            let declared: HashSet<String> =
                rendered.iter().map(|t| t.subject.to_string()).collect();
            let expected: BTreeSet<String> = golden
                .iter()
                .filter(|t| {
                    let s = t.subject.to_string();
                    let on_property = s == concept
                        || (s.starts_with(&format!("<{BASE}prop/"))
                            && s.ends_with(&predicate_suffix));
                    let restriction = matches!(t.subject, NamedOrBlankNode::BlankNode(_))
                        && golden.iter().any(|u| {
                            u.subject == t.subject
                                && u.predicate.as_str()
                                    == "http://www.w3.org/2002/07/owl#onProperty"
                                && u.object.to_string() == format!("<{BASE}prop/direct/{id}>")
                        });
                    (on_property || restriction)
                        && !terms.contains(&t.predicate.as_str())
                        && !t.predicate.as_str().starts_with(&format!("{BASE}prop/"))
                })
                .map(key)
                .collect();
            let rendered = keys(&rendered);
            assert!(declared.contains(&concept));
            assert_eq!(
                rendered,
                expected,
                "{id} ({flavor:?}): {}",
                diff(&rendered, &expected)
            );
            checked += 1;
        }
        assert_eq!(checked, 13);
    }
}

#[test]
fn dump_header_matches_wikibase() {
    let golden = parse_turtle(&sample("wikibase-rdf-full-dump.ttl"));
    let expected: BTreeSet<String> = golden
        .iter()
        .filter(|t| t.subject.to_string() == "<http://wikiba.se/ontology#Dump>")
        .map(key)
        .collect();
    assert_eq!(keys(&dump_header("2026-09-25T01:07:23Z")), expected);
}

#[test]
fn writers_round_trip() {
    let parsed = q8();
    let iris = scheme();
    let hasher = Hasher::local();
    let sites = testwiki;
    let triples = Renderer::new(&iris, &hasher, Produce::DUMP_FILE)
        .with_sites(&sites)
        .entity(&parsed.entity, parsed.page.as_ref());
    let turtle = to_turtle(&iris, &triples);
    assert!(turtle.contains("@prefix wd: <http://localhost:8080/entity/> ."));
    assert_eq!(keys(&parse_turtle(&turtle)), keys(&triples));
    let nt = to_ntriples(&triples);
    let back: Vec<Triple> = oxttl::NTriplesParser::new()
        .for_slice(nt.as_bytes())
        .map(Result::unwrap)
        .collect();
    assert_eq!(keys(&back), keys(&triples));
}

/// `Special:EntityData`'s flavors: `dump` is the dump file's triples plus the version
/// information on the document node; `simple` is truthy triples and sitelinks; `long` is
/// statement nodes without truthy triples or value nodes (`EntityDataSerializationService`).
#[test]
fn entity_data_flavors() {
    let parsed = q8();
    let iris = scheme();
    let hasher = Hasher::local();
    let sites = testwiki;
    let render = |p: Produce| {
        keys(
            &Renderer::new(&iris, &hasher, p)
                .with_sites(&sites)
                .entity(&parsed.entity, parsed.page.as_ref()),
        )
    };
    let doc = format!("<{SCRIPT}Special:EntityData/Q8>");
    let version_info: BTreeSet<String> = [
        format!("{doc} <http://creativecommons.org/ns#license> <http://creativecommons.org/publicdomain/zero/1.0/>"),
        format!("{doc} <http://schema.org/softwareVersion> \"1.0.0\""),
    ]
    .into_iter()
    .collect();

    let dump = render(Flavor::Dump.produce());
    let file = render(Produce::DUMP_FILE);
    assert_eq!(
        dump.difference(&file).cloned().collect::<BTreeSet<_>>(),
        version_info
    );
    assert!(file.is_subset(&dump));

    let simple = render(Flavor::Simple.produce());
    assert!(version_info.is_subset(&simple));
    assert!(
        simple
            .iter()
            .any(|t| t.contains("<http://schema.org/Article>"))
    );
    assert!(simple.iter().any(|t| t.contains("/prop/direct/P1>")));
    assert!(!simple.iter().any(|t| t.contains("/entity/statement/")));
    assert!(!simple.iter().any(|t| t.contains("#statements>")));

    let long = render(Flavor::Long.produce());
    assert!(long.iter().any(|t| t.contains("/entity/statement/")));
    assert!(long.iter().any(|t| t.contains("/prop/qualifier/P4>")));
    assert!(!long.iter().any(|t| t.contains("/prop/direct/P1>")));
    assert!(!long.iter().any(|t| t.contains("/value/")));

    assert_eq!(Flavor::parse(None), Some(Flavor::Full));
    assert_eq!(Flavor::parse(Some("")), Some(Flavor::Full));
    assert_eq!(Flavor::parse(Some("dump")), Some(Flavor::Dump));
    assert_eq!(Flavor::parse(Some("nonsense")), None);
}

/// Under `full`, the stubs: every property a written snak uses and every entity a value
/// names, in order of first mention, never the entity itself, never units or badges.
#[test]
fn full_flavor_mentions() {
    let parsed = q8();
    let iris = scheme();
    let hasher = Hasher::local();
    let mentions: Vec<String> = Renderer::new(&iris, &hasher, Flavor::Full.produce())
        .mentions(&parsed.entity)
        .iter()
        .map(|id| id.as_str().to_string())
        .collect();
    for id in [
        "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "P13", "Q3",
    ] {
        assert!(mentions.contains(&id.to_string()), "{id} in {mentions:?}");
    }
    assert!(!mentions.contains(&"Q8".to_string()));
    // Q2 is only a badge; units, globes and calendars are not mentions.
    assert!(!mentions.contains(&"Q2".to_string()), "{mentions:?}");
    assert_eq!(
        mentions.len(),
        mentions.iter().collect::<HashSet<_>>().len(),
        "each once"
    );
    assert!(
        Renderer::new(&iris, &hasher, Flavor::Dump.produce())
            .mentions(&parsed.entity)
            .is_empty()
    );
}

/// A property's stub is its declarations as Wikibase writes them (checked against the
/// dump above) and its labels and descriptions in the request's languages only.
#[test]
fn stubs() {
    let golden = parse_turtle(&sample("wikibase-rdf-full-dump.ttl"));
    let iris = scheme();
    let hasher = Hasher::local();
    let r = Renderer::new(&iris, &hasher, Flavor::Full.produce());
    let mut p5 = Entity::new(EntityId::parse("P5").unwrap(), EntityType::Property);
    p5.datatype = Some(DataType::Quantity);
    p5.labels.insert("en".into(), "test quantity".into());
    p5.labels.insert("de".into(), "Testmenge".into());
    p5.aliases.insert("en".into(), vec!["amount".into()]);
    let stub = keys(&r.stub(&p5, &["en".to_string()]));
    let concept = format!("<{BASE}entity/P5>");
    let golden_p5: BTreeSet<String> = golden
        .iter()
        .filter(|t| t.subject.to_string() == concept)
        .map(key)
        .collect();
    assert!(golden_p5.is_subset(&stub), "{}", diff(&stub, &golden_p5));
    assert!(
        !stub
            .iter()
            .any(|t| t.contains("Testmenge") || t.contains("amount"))
    );

    let mut q3 = Entity::new(EntityId::parse("Q3").unwrap(), EntityType::Item);
    q3.labels.insert("en".into(), "metre".into());
    q3.descriptions.insert("en".into(), "unit".into());
    q3.aliases.insert("en".into(), vec!["m".into()]);
    let stub = keys(&r.stub(&q3, &["en".to_string()]));
    let concept = format!("<{BASE}entity/Q3>");
    let expected: BTreeSet<String> = [
        format!("{concept} <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://wikiba.se/ontology#Item>"),
        format!("{concept} <http://www.w3.org/2000/01/rdf-schema#label> \"metre\"@en"),
        format!("{concept} <http://www.w3.org/2004/02/skos/core#prefLabel> \"metre\"@en"),
        format!("{concept} <http://schema.org/name> \"metre\"@en"),
        format!("{concept} <http://schema.org/description> \"unit\"@en"),
    ]
    .into_iter()
    .collect();
    assert_eq!(stub, expected);
}

/// The skolemized form (02 §4.3; wikibase-compat.md §5.7) replaces every blank node with
/// `https://scatter.red/genid/{hash}`, the hash its blank node is labelled with, so the
/// two forms map one to one and nothing else changes.
#[test]
fn skolem_form_maps_one_to_one() {
    let parsed = q8();
    let iris = scheme();
    let hasher = Hasher::local();
    let sites = testwiki;
    let render = |skolem: bool| {
        Renderer::new(&iris, &hasher, Produce::DUMP_FILE)
            .with_sites(&sites)
            .with_skolem(skolem)
            .entity(&parsed.entity, parsed.page.as_ref())
    };
    let plain = render(false);
    let skolem = render(true);
    assert!(plain.iter().any(|t| matches!(t.object, Term::BlankNode(_))));
    assert!(
        !skolem.iter().any(|t| matches!(t.object, Term::BlankNode(_))
            || matches!(t.subject, NamedOrBlankNode::BlankNode(_)))
    );
    let as_skolem = |t: &Triple| {
        let s = match &t.subject {
            NamedOrBlankNode::BlankNode(b) => {
                format!("<{}{}>", scatter_wikibase_rdf::GENID, b.as_str())
            }
            NamedOrBlankNode::NamedNode(n) => n.to_string(),
        };
        let o = match &t.object {
            Term::BlankNode(b) => format!("<{}{}>", scatter_wikibase_rdf::GENID, b.as_str()),
            other => other.to_string(),
        };
        format!("{s} {} {o}", t.predicate)
    };
    let mapped: BTreeSet<String> = plain.iter().map(as_skolem).collect();
    let written: BTreeSet<String> = skolem
        .iter()
        .map(|t| t.to_string().trim_end_matches(" .").to_string())
        .collect();
    assert_eq!(mapped, written);

    // The test vector of wikibase-compat.md §5.7: Q8's unknown-value P4 statement.
    let preimage = "main|1D2AD30E-661B-4261-8031-D87FFD783EAE|http://localhost:8080/entity/P4|0";
    assert_eq!(
        scatter_wikibase_rdf::genid_hash(preimage),
        "e8bbd8b5d234a4104ecacb4cc2f97e66ab3cd9a4f0c36d61cbe9504940fd5aa0"
    );
    let node = format!(
        "<{}e8bbd8b5d234a4104ecacb4cc2f97e66ab3cd9a4f0c36d61cbe9504940fd5aa0>",
        scatter_wikibase_rdf::GENID
    );
    assert!(
        written.contains(&format!(
            "<{BASE}entity/statement/Q8-1D2AD30E-661B-4261-8031-D87FFD783EAE> <{BASE}prop/statement/P4> {node}"
        )),
        "{written:#?}"
    );
}
