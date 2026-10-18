//! Settings (0085 §1–3): the registry of every `site` setting, the resolver that gives a
//! setting's value with its origin, and the one validator every writer of a `site`
//! record calls.
//!
//! - [`registry`]: `docs/registry/settings.toml`, with the instance's `setting:` records
//!   laid over it.
//! - [`value`]: whether a value fits a setting's type.
//! - [`resolve`]: tenant record → instance default → registry default, with locks,
//!   ceilings and origins, as a pure function of a [`Snapshot`].
//! - [`store`]: reading a snapshot from `view.registry`.
//! - [`mod@plan`]: a save turned into the records it appends, or refused whole.

pub mod plan;
pub mod registry;
pub mod resolve;
pub mod store;
pub mod value;

pub use plan::{Patch, Plan, Refusal, Rights, Target, plan};
pub use registry::{Bound, Scope, SettingDef, SettingType, SettingsRegistry};
pub use resolve::{Origin, Resolved, Row, Snapshot, TenantRows};
pub use store::{Partitions, load};
