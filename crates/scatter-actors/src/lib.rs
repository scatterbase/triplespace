//! Actors, issuers and permissions (ADR 0005 §2; 0007; 0016; 0023; 0024; 0028; 0040; 0056).
//!
//! **Identity** (0007). An actor is an issuer and a subject, written as an actor key
//! `{issuer}:{subject}` ([`ActorKey`]) that never contains a name. Issuers come from
//! `docs/registry/issuers.toml` ([`IssuerRegistry`]), with the `{tenant}` and `{farm}`
//! templates instantiated per tenant. Names, kinds and statuses live in actor records
//! ([`ActorRecord`]); account links ([`AccountLink`]) are the holder's opt-in; a
//! subsidiary's signing keys are [`KeyRecord`]s, and [`verify_submitted`] checks a
//! submitted signature against them.
//!
//! **Permissions** (0016, 0023, 0024, 0028, 0056): groups, memberships, blocks, ACL targets
//! and enclosure, and the evaluation of effective permissions for writes and reads, with
//! the visibility set of a target. These are the next slices of this crate.
//!
//! This crate is pure (0005 §3, rule 2): no I/O, no async, no globals beyond the embedded
//! registries. Everything it evaluates is passed in as data: the records, the registries
//! and the time.

#![forbid(unsafe_code)]

pub mod actor;
pub mod iri;
pub mod issuer;
pub mod key;
pub mod link;
pub mod signing;
pub mod time;

pub use actor::{
    ActorKind, ActorRecord, ActorRecordError, ActorStatus, check_name, normalize_name,
};
pub use iri::{IriContext, IriError, actor_iri, surrogate_iri};
pub use issuer::{ActorModel, Issuer, IssuerRegistry, IssuerRegistryError, RESERVED_CODES};
pub use key::{ActorKey, ActorKeyError, normalize_sub};
pub use link::{AccountLink, LinkError, LinkRequest, check_link};
pub use signing::{
    KeyRecord, PublicKey, SigningError, current_keys, key_id, signature_preimage, verify_submitted,
    zbase32,
};
pub use time::Timestamp;

/// The default issuer registry, `docs/registry/issuers.toml`, embedded at build time.
pub const ISSUERS_TOML: &str = include_str!("../../../docs/registry/issuers.toml");
