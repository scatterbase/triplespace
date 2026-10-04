//! Statement group shapes (ADR 0003 §3–5, §7): how one property's statements on one entity
//! are laid out, computed from the statements themselves.
//!
//! The data picks the layout (0003 §1.3): the same property can be a row of chips on one
//! item and a table on another. Detection is deterministic (§1.4): given the statements
//! and the role map, [`classify`] always returns the same [`Group`], so it can be run over
//! a Wikidata sample and measured (the `audit` example), and the browser runs the same
//! code through `scatter-wasm`.
//!
//! - [`classify`]: the shape, the order of values, best values and folds, the Table's
//!   columns and Matrix view, hoisted qualifiers and footnotes.
//! - [`Roles`]: which properties are `time-point`, `time-start` and the other roles.
//! - [`order`]: how values compare, for row order and the time axis.
//!
//! The `triplespace-table` content model (0045) will join this crate when the pages stack
//! is built.
//!
//! This crate is pure (0005 §3 rule 2) and builds for `wasm32-unknown-unknown` (rule 7).

#![forbid(unsafe_code)]

mod classify;
pub mod order;
mod roles;

pub use classify::{Footnotes, Group, Shape, Thresholds, classify};
pub use roles::{Role, Roles};
