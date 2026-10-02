//! The append-only log (0006; 0015; 0033): the bytes every other crate agrees on.
//!
//! This crate holds, in build order:
//!
//! - [`cbor`]: core-deterministic CBOR (RFC 8949 §4.2.1) with a strict decoder, a
//!   JSON mapping, and a serde bridge;
//! - [`hash`]: SHA-256 with the one-byte domain tags of 0006 §2;
//! - [`header`]: the ten-field record header the Merkle tree commits to;
//! - [`body`]: the salted parts, their leaves, the commitment, and erasure;
//! - [`record`]: `[header, body]`, and the global revision ID scheme;
//! - [`tree`]: the RFC 6962 Merkle tree over headers and the segment layout;
//! - [`registry`]: the graph registry of `graphs.toml`, and the `key:` and `graph:`
//!   configuration records;
//! - [`store`]: the `LogStore` contract, with [`memory`] and the [`segments`] file
//!   backend, the [`conformance`] suite every backend passes, and [`genesis`], which
//!   starts a `config` partition.
//!
//! Each shape is specified in `docs/api/payloads.md` §1–2 and §4, with test vectors
//! under `docs/api/vectors/`. Checkpoints, proofs and `verify` are `scatter-integrity`'s.

pub mod body;
pub mod cbor;
pub mod conformance;
pub mod genesis;
pub mod hash;
pub mod header;
pub mod memory;
pub mod record;
pub mod registry;
pub mod segments;
pub mod store;
pub mod tree;

pub use body::{Body, BodyError, Part, Salt};
pub use cbor::{CborError, Value};
pub use hash::Hash;
pub use header::{Header, HeaderError};
pub use memory::MemoryStore;
pub use record::{Record, RecordError};
pub use registry::{Graph, GraphRegistry};
pub use segments::SegmentStore;
pub use store::{Draft, LogStore, Slot, StoreError};
pub use tree::{Frontier, Segments};
