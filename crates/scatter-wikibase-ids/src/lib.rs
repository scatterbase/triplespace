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
//! Canonicalizing IDs written by users (the tenant-relative `QQQ5`, case) and the keyed data
//! types are still in the model, until the write path has one place where client IDs can be
//! canonicalized (0115 §13).
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
pub mod value_iri;

#[cfg(feature = "providers")]
pub use hash::hasher_for;
pub use id::{IdForm, TriplespaceId};
pub use provider::ProviderModel;
pub use value_iri::ValueIris;
