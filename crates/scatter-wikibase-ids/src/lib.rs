//! How Triplespace names Wikibase entities (0115 §13): an extension crate over
//! `scatter-wikibase-model`, under 22 §1.2 rule 12.
//!
//! - **Value IRIs stored as entity IDs** ([`value_iri`]): the `unit`, `globe` and
//!   `calendarmodel` of a value kept as the ID they name (0004 A20), expanded back to the
//!   IRIs Wikibase writes through the model's [`scatter_wikibase_model::ValueIriMap`].
//!
//! The foreign, keyed and tenant-relative ID forms of 0017 §1 and 0044 §1 move here in
//! 0115 §13's second phase.
//!
//! Pure (0005 §3, rule 2).

#![forbid(unsafe_code)]

pub mod value_iri;

pub use value_iri::ValueIris;
