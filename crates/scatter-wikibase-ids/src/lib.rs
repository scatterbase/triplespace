//! How Triplespace names Wikibase entities (0115 §13): an extension crate over
//! `scatter-wikibase-model`, under 22 §1.2 rule 12.
//!
//! - **The ID forms** ([`id`]): local, foreign and keyed (0017 §1), told apart and validated
//!   against the registries a caller passes in, as the [`TriplespaceId`] trait over the model's
//!   `EntityId`.
//! - **Providers** ([`ProviderModel`]): what the ID forms and value IRIs need to know about
//!   providers, as a trait. Triplespace's model, `scatter-providers`, implements it behind the
//!   default `providers` feature, which also gives `hasher_for`, the model's hasher set to hash
//!   a provider's IDs in the bare form the provider wrote them.
//! - **Value IRIs stored as entity IDs** ([`value_iri`]): the `unit`, `globe` and
//!   `calendarmodel` of a value kept as the ID they name (0004 A20), expanded back to the
//!   IRIs Wikibase writes through the model's [`scatter_wikibase_model::ValueIriMap`].
//!
//! - **Entity IRIs in RDF** (`rdf`, with the `rdf` feature): `TriplespaceIris`, which names
//!   foreign, source and keyed IDs for `scatter-wikibase-rdf`, and `rdf::upstream_nodes`,
//!   the nodes a mirrored statement is named by as its upstream names them (0115 §15).
//!
//! Canonicalizing IDs written by users (the tenant-relative `QQQ5`, case) and the keyed data
//! types stay in the model, so that every path that makes an ID canonicalizes it
//! (0115 A4).
//!
//! Pure (0005 §3, rule 2).

#![forbid(unsafe_code)]

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(feature = "providers")]
pub mod hash;
pub mod id;
pub mod provider;
#[cfg(feature = "rdf")]
pub mod rdf;
pub mod value_iri;

#[cfg(feature = "providers")]
pub use hash::hasher_for;
pub use id::{IdForm, TriplespaceId};
pub use provider::ProviderModel;
#[cfg(feature = "rdf")]
pub use rdf::TriplespaceIris;
pub use value_iri::ValueIris;
