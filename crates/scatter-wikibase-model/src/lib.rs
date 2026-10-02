//! The Wikibase data model (ADR 0005 §2; [wikibase-compat.md](../../../docs/api/wikibase-compat.md)
//! §2–3), with the three entity ID forms of 0017 §1 and the keyed entity types of 0009 §3 and
//! 0017 §5.
//!
//! The model mirrors Wikibase's canonical JSON faithfully. 0006 §2 makes the stored form
//! "the JSON's structure and nothing else", and 0004 §7 says stored values are never
//! changed, so this crate holds what a source asserted, in the shape it asserted it:
//! sitelinks stay keyed by site with their title and URL, values keep the strings
//! Wikibase gives them, and property order inside qualifiers and references is data.
//! Normalization (URLs, value keys, IDs) is a function over the model, in
//! `scatter-normalize` and the projections, not a transformation at parse time.
//!
//! Two derivable fields are dropped at ingest and recomputed on output (0006 §2, as
//! amended): the `hash` on snaks and references, and `numeric-id` beside `id` on entity
//! values. [`json::Form::Storage`] is the form without them, which the log stores;
//! [`json::Form::Wikibase`] is what the API serves. The hash computation itself is the
//! next slice of this crate; until then a stored hash is emitted and an absent one is
//! left out.
//!
//! Canonical JSON ([`json::canonical`]) sorts object keys the way the core deterministic
//! CBOR encoding does (by encoded bytes: shorter keys first, then bytewise), so that
//! JSON → CBOR → JSON reproduces the bytes.
//!
//! This crate is pure (0005 §3, rule 2) and, through `scatter-wikibase-shape`, has to build
//! for `wasm32-unknown-unknown`.

#![forbid(unsafe_code)]

pub mod entity;
pub mod id;
pub mod json;
pub mod sites;
pub mod statement;
pub mod value;

pub use entity::{Entity, EntityType, PageInfo, ParsedEntity, Sitelink, Term};
pub use id::{EntityId, IdForm, IdParseError, StatementId, Subject};
pub use json::{Form, JsonError};
pub use sites::{Site, SiteRegistry, SiteRegistryError};
pub use statement::{Rank, Reference, Snak, SnakKind, Statement};
pub use value::{
    DataType, DataValue, EntityIdValue, GlobeCoordinate, MonolingualText, Quantity, Time, ValueType,
};

/// The default site-alias registry, `docs/registry/sites.toml`, embedded at build time.
pub const SITES_TOML: &str = include_str!("../../../docs/registry/sites.toml");
