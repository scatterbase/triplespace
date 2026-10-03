//! The `view` projections over Postgres (0013 §5, §7; 0005 §2): the [`PgBackend`] that
//! drives `scatter-projection`'s pipeline on the `ops` tables, and the projections in
//! step order.
//!
//! - Step 1, what the write path reads: [`registry`], [`actors`] (actor, group,
//!   membership, block, acl), and the keyed surrogates ([`sources`]).
//! - Step 2, the source graphs: `entity_source` and `keyed_map` ([`sources`]), `job`
//!   ([`jobs`]).
//! - Step 4, resolution: `entity` with its `term` and `identifier` tables ([`entities`]).
//! - Step 5: `activity` ([`activity`]).
//!
//! Clusters (step 3), pages, sitelinks, references, corrections, constraints, patrol,
//! filters, notifications and RDF follow as the crates that produce their records are
//! built. [`milestone_pipeline`] assembles the set built so far.

pub mod activity;
pub mod actors;
pub mod backend;
pub mod common;
pub mod entities;
pub mod jobs;
pub mod registry;
pub mod sources;

pub use activity::ActivityProjection;
pub use actors::Farm;
pub use backend::{PgBackend, PgCx};
pub use entities::EntityProjection;
pub use jobs::JobProjection;
pub use registry::RegistryProjection;
pub use sources::{EntitySourceProjection, KeyedMapProjection, KeyedSurrogateProjection};

use scatter_actors::issuer::IssuerRegistry;
use scatter_projection::{Pipeline, ProjectionError};

/// The pipeline of every projection built so far, in step order.
pub fn milestone_pipeline(
    issuers: IssuerRegistry,
    farm: Farm,
) -> Result<Pipeline<PgBackend>, ProjectionError> {
    Pipeline::new()
        .with(RegistryProjection)?
        .with(KeyedSurrogateProjection)?
        .with(actors::ActorProjection::new(issuers, farm))?
        .with(actors::GroupProjection)?
        .with(actors::MembershipProjection)?
        .with(actors::BlockProjection)?
        .with(actors::AclProjection)?
        .with(EntitySourceProjection)?
        .with(KeyedMapProjection::default())?
        .with(JobProjection)?
        .with(EntityProjection::default())?
        .with(ActivityProjection)
}
