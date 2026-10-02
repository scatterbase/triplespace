//! The append-only log (0006; 0015; 0033): the bytes every other crate agrees on.
//!
//! This crate holds, in build order:
//!
//! - [`cbor`]: core-deterministic CBOR (RFC 8949 §4.2.1) with a strict decoder, a
//!   JSON mapping, and a serde bridge;
//! - [`hash`]: SHA-256 with the one-byte domain tags of 0006 §2;
//! - [`header`]: the ten-field record header the Merkle tree commits to;
//! - [`body`]: the salted parts, their leaves, the commitment, and erasure;
//! - [`record`]: `[header, body]`, and the global revision ID scheme.
//!
//! Each shape is specified in `docs/api/payloads.md` §1–2, with test vectors under
//! `docs/api/vectors/`. The Merkle tree, checkpoints and the `LogStore` trait follow.

pub mod body;
pub mod cbor;
pub mod hash;
pub mod header;
pub mod record;

pub use body::{Body, BodyError, Part, Salt};
pub use cbor::{CborError, Value};
pub use hash::Hash;
pub use header::{Header, HeaderError};
pub use record::{Record, RecordError};
