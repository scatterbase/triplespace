//! The dump header, and triples written as N-Triples or Turtle (02 §4.1: `oxttl`, no
//! store). Writing goes to memory; where the bytes go is the caller's.

use oxrdf::{Literal, NamedNode, Triple, TripleRef};
use oxttl::{NTriplesSerializer, TurtleSerializer};
use scatter_vocab::{cc, owl, rdf, schema, wikibase, xsd};

use crate::iri::IriScheme;

/// RDF format version Wikibase writes in the dump header (`RdfVocabulary::FORMAT_VERSION`).
pub const FORMAT_VERSION: &str = "1.0.0";
/// The ontology a dump imports.
pub const ONTOLOGY: &str = "http://wikiba.se/ontology-1.0.owl";
/// The licence a Wikibase dump declares.
pub const CC0: &str = "http://creativecommons.org/publicdomain/zero/1.0/";

/// The `wikibase:Dump` node a dump opens with (wikibase-compat.md §5.2). `modified` is the
/// dump's time as an `xsd:dateTime`.
#[must_use]
pub fn dump_header(modified: &str) -> Vec<Triple> {
    let dump = wikibase::DUMP;
    vec![
        Triple::new(dump, rdf::TYPE, schema::DATASET),
        Triple::new(dump, rdf::TYPE, owl::ONTOLOGY),
        Triple::new(dump, cc::LICENSE, NamedNode::new_unchecked(CC0)),
        Triple::new(
            dump,
            schema::SOFTWARE_VERSION,
            Literal::new_simple_literal(FORMAT_VERSION),
        ),
        Triple::new(
            dump,
            schema::DATE_MODIFIED,
            Literal::new_typed_literal(modified, xsd::DATE_TIME),
        ),
        Triple::new(dump, owl::IMPORTS, NamedNode::new_unchecked(ONTOLOGY)),
    ]
}

/// Triples as N-Triples, one per line, in the order given.
#[must_use]
pub fn to_ntriples<'t>(triples: impl IntoIterator<Item = impl Into<TripleRef<'t>>>) -> String {
    let mut w = NTriplesSerializer::new().for_writer(Vec::new());
    for t in triples {
        // Writing to a Vec cannot fail.
        let _ = w.serialize_triple(t);
    }
    String::from_utf8(w.finish()).unwrap_or_default()
}

/// Triples as Turtle with `iris`' prefixes declared, in the order given, so that a
/// subject's consecutive triples share one block.
#[must_use]
pub fn to_turtle<'t>(
    iris: &IriScheme<'_>,
    triples: impl IntoIterator<Item = impl Into<TripleRef<'t>>>,
) -> String {
    let mut s = TurtleSerializer::new();
    for (prefix, ns) in iris.prefixes() {
        if let Ok(next) = s.clone().with_prefix(prefix, ns) {
            s = next;
        }
    }
    let mut w = s.for_writer(Vec::new());
    for t in triples {
        let _ = w.serialize_triple(t);
    }
    w.finish()
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}
