//! Thread records: the `scatter:v0/thread` payload type (ADR 0019; architecture 14).
//!
//! A thread is a page in the `Thread` namespace, and each post is one record keyed by the
//! thread's page ID. A record declares four parts:
//!
//! | Part | Holds |
//! |---|---|
//! | 0 `content` | The operation: [`ThreadOp`]. Never the post's text |
//! | 1 `comment` | The summary, for every operation but `create` and `post` |
//! | 2 `attestation` | Actor and signature, and what the server minted: [`Minted`] |
//! | 3 `text` | The post's text, for `create`, `post` and `edit` |
//!
//! The structure and the text are separate parts so that erasing a post's text leaves the
//! tree intact (14 §1.5).
//!
//! [`Thread`] is the fold of a thread's records: its posts and their tree, subject and title,
//! home and listings with their pins, status, and the ranges of posts that stand collapsed.
//! [`Thread::apply`] checks each record against that state as it folds it in, with what only
//! the store knows behind [`Pages`]. [`Statuses`] is the thread-status registry, and
//! [`normalize_subject`] and [`mint_title`] make thread titles.
//!
//! Pure; no I/O. The content part is encoded as JSON here, and the write path turns it into
//! the record's canonical CBOR, as `scatter-pages` does. Proposals (14 §5) and boards' own
//! content model (14 §3.3) are not here yet.

#![forbid(unsafe_code)]

mod fold;
mod op;
mod status;
mod title;

pub use fold::{
    AnyPages, Attachment, Check, Invalid, Limits, Minted, Pages, Post, Record, Thread, Writer,
};
pub use op::{AddedBy, ImportedFrom, MARKDOWN, Range, Target, ThreadOp, WIKITEXT};
pub use status::{Category, OPEN, Status, Statuses, StatusesError, THREAD_STATUSES_TOML};
pub use title::{SubjectError, mint_title, normalize_subject};

/// The payload type.
pub const PAYLOAD_THREAD: &str = "scatter:v0/thread";

/// The index of the `text` part.
pub const TEXT_PART: usize = 3;

/// The `Thread` namespace (14 §1.4).
pub const NS_THREAD: i32 = 214;
