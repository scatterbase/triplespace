//! The blob store (0039 §3, §4; 03 §8, 12 §2): content-addressed storage over
//! [`object_store`], shared with Scatterbase, whose blobs are the same objects under the
//! same layout (0039 §22).
//!
//! - [`BlobStore`] is the store over one backend. [`BlobStore::filesystem`] is the `fs`
//!   backend, a directory; the `s3` backend is not built yet and will come in through
//!   [`BlobStore::new`], which takes any `object_store` backend.
//! - [`Scope`] and [`Key`] are the storage scopes and the key layout of 0039 §3–4; [`Prefix`]
//!   names the sets a listing walks and a prefix deletion clears.
//! - [`Sha256`], [`Sha1`], [`Digest`] and [`Hasher`] are the hashes: SHA-256 addresses the
//!   bytes, SHA-1 rides along for MediaWiki compatibility, both computed in one streaming
//!   pass.
//! - A write is [`BlobStore::put`] for bytes in memory, or [`BlobStore::stash`] then
//!   [`BlobStore::commit`] for a stream: bytes land under `stash/` while they are hashed and
//!   the size limit is enforced, then are copied to their content address unless an object is
//!   already there. [`BlobStore::put_stream`] does both.
//! - Reads are [`BlobStore::get`], [`BlobStore::get_range`] and the streamed
//!   [`BlobStore::read`]; [`BlobStore::head`] and [`BlobStore::exists`] look without reading.
//! - [`BlobStore::delete`] and [`BlobStore::delete_prefix`] remove; [`BlobStore::list`] is
//!   the walk the orphan sweep and `verify` make, and [`BlobStore::verify`] is the `full`
//!   check of one object (0039 §14).
//!
//! Async, like `scatter-log-postgres` (22 §1.2 rule 2 names the pure crates; this is not one).
//! No `triplespace-*` crate is reachable from here (rule 1).

pub mod error;
pub mod hash;
pub mod key;
pub mod store;

pub use bytes::Bytes;
pub use error::BlobError;
pub use hash::{Digest, Hasher, Sha1, Sha256};
pub use key::{Key, KeyError, Kind, Prefix, Scope};
pub use store::{BlobStore, Entry, Meta, Reader, Staged, Stored, Verified};
