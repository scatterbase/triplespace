//! How Triplespace names Wikibase entities (0115 §13): an extension crate over
//! `scatter-wikibase-model`, under 22 §1.2 rule 12.
//!
//! - **The ID forms** ([`id`]): local, foreign and keyed (0017 §1), told apart and validated
//!   against the registries a caller passes in, as the [`TriplespaceId`] trait over the model's
//!   `EntityId`.
//! - **Value IRIs stored as entity IDs** ([`value_iri`]): the `unit`, `globe` and
//!   `calendarmodel` of a value kept as the ID they name (0004 A20), expanded back to the
//!   IRIs Wikibase writes through the model's [`scatter_wikibase_model::ValueIriMap`].
//!
//! Canonicalizing IDs written by users (the tenant-relative `QQQ5`, case), the keyed data
//! types and provider hashing move here in the rest of 0115 §13's second phase.
//!
//! Pure (0005 §3, rule 2).

#![forbid(unsafe_code)]

pub mod id;
pub mod value_iri;

pub use id::{IdForm, TriplespaceId};
pub use value_iri::ValueIris;
