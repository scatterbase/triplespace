//! Wikibase's RDF, format 1.0.0, from the Wikibase data model (wikibase-compat.md §4–7;
//! 02 §4.2).
//!
//! The output is identical in shape to what Wikibase's `Special:EntityData` and
//! `dumpRdf.php` write, including their departures from `ontology.owl`
//! (wikibase-compat.md §4.5): no vocabulary is added (0001 §3). Nothing here does I/O;
//! the renderer returns triples and the writers return strings.
//!
//! - [`IriScheme`] names every node and predicate: local IDs under the instance's base,
//!   foreign and source IDs by their provider's template, keyed IDs by their type's
//!   (02 §2.3; 05 §1.5; 04 §3.5).
//! - [`Renderer`] writes an entity's triples under Wikibase's `RdfProducer` flags
//!   ([`Produce`]): `Special:EntityData`'s four [`Flavor`]s and the two dump-file flavors.
//!   The document node, the entity and its terms, sitelinks, the truthy triples of
//!   best-rank statements, and the statement, reference and value nodes; under the `full`
//!   flavor, [`Renderer::mentions`] lists what to stub and [`Renderer::stub`] writes each.
//!   A [`Seen`] set lets a dump write each shared node once.
//! - [`dump_header`], [`to_ntriples`] and [`to_turtle`] write the result.
//!
//! **What is not here yet:**
//! - `edtf` values are written as their `xsd:edtf` literal only; the `xsd:dateTime` values
//!   WikibaseEDTF derives, defects and all, wait on a decision (0001 A19).
//! - Normalized values (`wdtn:`, `psn:`, `pqn:`, `prn:` triples) need unit conversion and
//!   formatter-URL configuration, which no instance has; the predicates are declared on the
//!   property as Wikibase declares them.
//! - Lexemes, MediaInfo and the keyed types' extra nodes (0066 §6, 0065, 0048 §5), the
//!   skolemized form and the triple-set diff (0032 §9), the schema.org profile (0076 §2–3),
//!   and the fused cluster body (0082 §7).
//! - [`Renderer::with_upstream`] names the statements given to it as their upstream
//!   names them (05 §1.5; 0002 A43), each after the entity its upstream names it by, which
//!   for a key-mapped provider's statement is the item mapped onto its keyed subject
//!   (0002 A44); which statements qualify is the caller's to say, from the resolver's
//!   provenance. A dump needs that provenance per entity too (step 3).
//!
//! About 0001 §3 (02 §4.2): built for entities, in both flavors; the page subjects of
//! 02 §5.4 and the skolemized form of 02 §4.3 are not.

mod iri;
mod render;
mod time;
mod write;

pub use iri::{
    Family, IriScheme, Namespaces, PropertyIris, UpstreamNodes, escape_iri, rawurlencode,
    wf_urlencode,
};
pub use render::{
    EARTH, Flavor, GENID, Produce, Renderer, Seen, SiteFacts, Sites, UNIT_ONE, genid_hash,
    property_type_name,
};
pub use time::{JULIAN, xsd_date_time};
pub use write::{CC0, FORMAT_VERSION, ONTOLOGY, dump_header, to_ntriples, to_turtle};
