//! Actors, issuers and permissions (ADR 0005 §2; 0007; 0016; 0023; 0024; 0028; 0040; 0056).
//!
//! **Identity** (0007). An actor is an issuer and a subject, written as an actor key
//! `{issuer}:{subject}` ([`ActorKey`]) that never contains a name. Issuers come from
//! `docs/registry/issuers.toml` ([`IssuerRegistry`]), with the `{tenant}` and `{farm}`
//! templates instantiated per tenant. Names, kinds and statuses live in actor records
//! ([`ActorRecord`]); actors with no stable ID get surrogates ([`ActorSurrogates`]);
//! account links ([`AccountLink`]) are the holder's opt-in; a subsidiary's signing keys
//! are [`KeyRecord`]s, and [`verify_submitted`] checks a submitted signature against them.
//!
//! The payload shapes this crate defines are documented in
//! [payloads-actors.md](../../../docs/api/payloads-actors.md).
//!
//! **Permissions** (0016 §3; 0024 §3–5; 0025 §3; 0028 §3–4). Groups come from
//! `docs/registry/groups.toml` ([`GroupRegistry`]) and grants from `grants.toml`
//! ([`GrantRegistry`]). Memberships and blocks are actor records ([`Membership`],
//! [`Block`]) folded as of a time; [`evaluate`] turns a [`Principal`] into its
//! [`Effective`] permissions: the union over groups, minus blocks (own, operator's,
//! farm account's), intersected with a credential's grants. [`RateLimitPolicy`] picks the
//! limit a request is under, and [`autopatrolled`] is the rule of 0023 §6.
//!
//! **ACLs and visibility** (0016 §4; 0023 §1–3; 0039 §10; 0056 §2–6). An [`Acl`] restricts
//! permissions on a [`Target`] to a group until an expiry; [`acl::check`] evaluates an
//! action conjunctively over the ACLs along a target's enclosure chain, which the caller
//! supplies; [`acl::visibility`] is the set of groups a reader must be in,
//! [`acl::read_decision`] tells readable from removed from absent, and
//! [`acl::may_include`] is the subset rule for includes. The tenancy policy
//! ([`TenancyPolicy`]) says which restrictions an instance allows and what is locked.
//!
//! This crate is pure (0005 §3, rule 2): no I/O, no async, no globals beyond the embedded
//! registries. Everything it evaluates is passed in as data: the records, the registries
//! and the time.

#![forbid(unsafe_code)]

pub mod acl;
pub mod actor;
pub mod evaluate;
pub mod grant;
pub mod group;
pub mod iri;
pub mod issuer;
pub mod key;
pub mod link;
pub mod membership;
pub mod ratelimit;
pub mod signing;
pub mod surrogate;
pub mod tenancy;
pub mod time;

pub use acl::{
    Acl, AclError, AclPartition, Denied, ReadDecision, ReadKind, Restriction, SetMember, Target,
    TargetError, Visibility, current_acl, may_include, read_decision, visibility,
};
pub use actor::{
    ActorKind, ActorRecord, ActorRecordError, ActorStatus, check_name, normalize_name,
};
pub use evaluate::{
    Credential, Effective, INSTANCE_RIGHTS, Principal, autopatrolled, evaluate, groups_of,
    instance_rights,
};
pub use grant::{Grant, GrantRegistry, GrantRegistryError};
pub use group::{
    GraphAclDefault, GraphWriters, Group, GroupRegistry, GroupRegistryError, Implicit, Scope,
};
pub use iri::{IriContext, IriError, actor_iri, surrogate_iri};
pub use issuer::{ActorModel, Issuer, IssuerRegistry, IssuerRegistryError, RESERVED_CODES};
pub use key::{ActorKey, ActorKeyError, normalize_sub};
pub use link::{AccountLink, LinkError, LinkRequest, check_link};
pub use membership::{
    Appended, Block, BlockScope, Membership, MembershipAction, blocked_as_of, blocked_in_layers,
    memberships_as_of,
};
pub use ratelimit::{Limit, RateLimitPolicy};
pub use signing::{
    KeyRecord, PublicKey, SigningError, current_keys, key_id, signature_preimage, verify_submitted,
    zbase32,
};
pub use surrogate::{ActorSurrogates, InMemoryActorSurrogates};
pub use tenancy::{Preset, TenancyError, TenancyPolicy, TenancyRegistry};
pub use time::Timestamp;

/// The default issuer registry, `docs/registry/issuers.toml`, embedded at build time.
pub const ISSUERS_TOML: &str = include_str!("../../../docs/registry/issuers.toml");
/// The default groups and permissions, `docs/registry/groups.toml`, embedded at build time.
pub const GROUPS_TOML: &str = include_str!("../../../docs/registry/groups.toml");
/// The grants, `docs/registry/grants.toml`, embedded at build time.
pub const GRANTS_TOML: &str = include_str!("../../../docs/registry/grants.toml");
/// The tenancy presets, `docs/registry/tenancy.toml`, embedded at build time.
pub const TENANCY_TOML: &str = include_str!("../../../docs/registry/tenancy.toml");
