//! The `view` projections over Postgres (0013 §5, §7; 0005 §2): the [`PgBackend`] that
//! drives `scatter-projection`'s pipeline on the `ops` tables, and the projections in
//! step order.
//!
//! Step 1, what the write path reads: [`registry`], [`actors`] (actor, group, membership,
//! block, acl). The later steps (entity sources and maps, resolution, terms, identifiers,
//! jobs, activity) follow as the ingest and write paths that produce their records are
//! built.
//!
//! [`milestone_pipeline`] assembles the set built so far.

pub mod actors;
pub mod backend;
pub mod common;
pub mod registry;

pub use actors::Farm;
pub use backend::{PgBackend, PgCx};
pub use registry::RegistryProjection;

use scatter_actors::issuer::IssuerRegistry;
use scatter_projection::{Pipeline, ProjectionError};

/// The pipeline of every projection built so far, in step order.
pub fn milestone_pipeline(
    issuers: IssuerRegistry,
    farm: Farm,
) -> Result<Pipeline<PgBackend>, ProjectionError> {
    Pipeline::new()
        .with(RegistryProjection)?
        .with(actors::ActorProjection::new(issuers, farm))?
        .with(actors::GroupProjection)?
        .with(actors::MembershipProjection)?
        .with(actors::BlockProjection)?
        .with(actors::AclProjection)
}
