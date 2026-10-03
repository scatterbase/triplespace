//! Integrity over the log (0006 §6–9): signed checkpoints, inclusion and consistency
//! proofs, the key chain, attestation signatures, export bundles and `verify`.
//!
//! - [`note`]: C2SP signed notes;
//! - [`checkpoint`]: C2SP tlog-checkpoints and segment manifests, signed by the instance key;
//! - [`proof`]: RFC 9162 inclusion and consistency proofs;
//! - [`attest`]: client signatures and instance attestations in the attestation part;
//! - [`keys`]: the key chain of a `config` partition;
//! - [`checkpoints`]: where checkpoints and manifests are kept, and how they are made;
//! - [`mod@verify`]: levels 1 and 2 of 0006 §9 over a store and its checkpoints;
//! - [`bundle`]: export bundles, written from any store and verified on their own.

pub mod attest;
pub mod bundle;
pub mod checkpoint;
pub mod checkpoints;
pub mod keys;
pub mod note;
pub mod proof;
pub mod verify;

pub use attest::{AttestError, Attestation};
pub use bundle::{Bundle, BundleError, BundlePartition};
pub use checkpoint::{Checkpoint, CheckpointError, SignedCheckpoint};
pub use checkpoints::{CheckpointStore, CheckpointStoreError, FileCheckpoints, MemoryCheckpoints};
pub use keys::{ChainError, KeyChain};
pub use note::{Note, NoteError, NoteSigner, NoteVerifier};
pub use proof::ProofError;
pub use verify::{Level, Problem, Report, Verify, verify};
