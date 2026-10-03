//! The projection contract (0013 §7; 0005 §2): how the tables the API reads are derived
//! from the log, and kept derived.
//!
//! A [`Projection`] consumes records and writes rows. It has a name, which keys its
//! position per partition in the backend's `projection_state`; a [`Step`], the
//! dependency order of 0013 §7, so that a projection only ever reads tables of lower
//! steps; and three operations: `apply` one record, `fanout` to one key the record's
//! effects reach, and `reset` for a rebuild.
//!
//! A [`Pipeline`] holds the projections in step order and drives them two ways:
//!
//! - **inline**, in the appending transaction ([`Pipeline::apply_inline`]): the write
//!   path appends a record, then applies every projection to it and follows the fan-out
//!   until a [`Budget`] is spent; the remainder is queued as [`Work`] for the worker;
//! - **catching up** ([`Pipeline::catch_up`]): behind the append, from each projection's
//!   position to the partition's head, in batches of one transaction each, as bulk jobs
//!   and the queued remainder are applied.
//!
//! [`Pipeline::rebuild`] truncates and replays from offset 0; [`Pipeline::lags`] is the
//! distance from each position to the head, which job pages report.
//!
//! The crate knows no database: a [`Backend`] supplies the unit of work (a transaction),
//! the positions and the work queue, and a projection's `apply` runs against that unit.
//! Futures are `Send` and no runtime is assumed (0005 rule 2, A56). [`MemoryBackend`] is
//! the in-memory backend the tests use; the Postgres one is `triplespace-db`'s.

pub mod backend;
pub mod memory;
pub mod pipeline;
pub mod projection;

pub use backend::{Backend, Lag, Work};
pub use memory::MemoryBackend;
pub use pipeline::{Budget, Inline, Pipeline, Progress};
pub use projection::{Applied, BoxFuture, Projection, ProjectionError, Step, Target};
