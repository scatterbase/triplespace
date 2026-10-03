//! Change sets (0002 §8; 0004 §9; 0009 §10; 0035 §3; 0038 §1; payloads.md §3): the
//! operations every write to a `local`, `mirror/{provider}` or `pages` partition is made
//! of, the NDJSON wire format they travel in, eager validation, the `changes` summary a
//! mirrored revision keeps, and the `Adapter` trait a provider's ingester implements.
//!
//! The crate is pure: it knows no log, no database and no network (0005 §2). The flow
//! an ingester follows with it is
//!
//! 1. [`wire::Batch::read`] the job line and the raw operations;
//! 2. [`wire::Batch::refs`] to learn the temporary handles; for each, look the
//!    `create-or-add`'s match key up or mint an ID, then [`wire::Batch::resolve_refs`]
//!    to rewrite every handle ([`wire::WireOp::assign_id`] for one without a handle);
//! 3. [`wire::Batch::operations`] to type them, then [`op::Operation::complete`] and
//!    [`validate::validate`] each against the job's [`validate::Context`]
//!    ([`validate::validate_all`] does both);
//! 4. turn each `create-or-add` into what happened
//!    ([`op::Operation::resolve_create_or_add`]) and fan a bulk `retain` out
//!    ([`op::Operation::fan_out`]);
//! 5. append what is left, one record per operation, keyed by [`op::Operation::key`].
//!
//! A mirror adapter instead builds [`op::Operation::Put`] values directly, with
//! [`changes::Changes::between`] for the summary, after [`adapter::Adapter::rewrite_entity`]
//! has put the provider's IDs in stored form.

#![forbid(unsafe_code)]

pub mod adapter;
pub mod apply;
pub mod changes;
pub mod job;
pub mod op;
pub mod rank;
pub mod validate;
pub mod wire;

pub use adapter::{
    Adapter, AdapterError, KeyMap, WikibaseAdapter, deterministic_statement_id,
    deterministic_statement_id_for,
};
pub use apply::{Correction, LocalState};
pub use changes::{Changes, Counts};
pub use job::{Graph, JobHeader, Mode};
pub use op::{Link, MatchKey, Operation, Resolution, Retention, SitelinkPatch, Upstream, Via};
pub use rank::ProviderOrder;
pub use validate::{Context, NoProperties, Problem, PropertyTypes, validate, validate_all};
pub use wire::{Batch, WireError, WireOp};
