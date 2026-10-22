//! The content part of a thread record: one operation (14 §1.2, §1.5).

use serde::{Deserialize, Serialize};

/// The media type of a post's text when none is given.
pub const MARKDOWN: &str = "text/markdown";

/// The media type of a post an import writes in wikitext (0054 §5).
pub const WIKITEXT: &str = "text/x-wiki";

/// What a thread is attached to (14 §1.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Target {
    /// A page ID: a subject's talk page, or a board, which is its own talk page.
    Page(u64),
    /// A local actor key, for `User talk` (0007 §1). Home only.
    Actor(String),
}

/// The upstream revision that added an imported post's text, and its actor (0090 §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedBy {
    /// The upstream revision ID.
    pub revision: u64,
    /// The upstream actor, as the source names actors.
    pub actor: String,
}

/// Where an imported thread or post came from: `imported_from` (0054 §5, 0069 §12, 0090 §8).
///
/// On `create`, the page fields name the thread's source; on every imported post, `create`
/// included, the comment fields describe the comment. Every field is optional, since each
/// importer knows a different subset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedFrom {
    /// The page repository or wiki the talk page came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The talk page's upstream title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The upstream revision the thread was converted from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// The archive subpage, when the thread came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    /// The hash of the section's DiscussionTools name (0069 §12), hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_hash: Option<String>,
    /// The comment's DiscussionTools name (0090 §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The comment's DiscussionTools ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The user its signature names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_as: Option<String>,
    /// The upstream revision that added it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<AddedBy>,
    /// Its original time, ISO 8601.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Why the post holds a whole section or page (0090 §4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// A wrapper's opening and closing calls, on a closing or collapsing post (0090 §3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<[String; 2]>,
}

/// A range of a thread's posts, by revision ID, that a post collapses (14 §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// The first post collapsed.
    pub first: u64,
    /// The last post collapsed; not before `first` in the thread's order.
    pub last: u64,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// One thread operation: the content part of a `scatter:v0/thread` record (14 §1.5).
///
/// Proposals (`propose`, `submit`, `withdraw`, 14 §5) are not here yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum ThreadOp {
    /// Mints the thread, attaches it to its home, and is the opening post.
    Create {
        /// The subject, as typed.
        name: String,
        /// The home.
        target: Target,
        /// Further targets listed at creation.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        also: Vec<Target>,
        /// A status the thread arrives with, as an import's does.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        /// Pins the thread on its home from the start.
        #[serde(default, skip_serializing_if = "is_false")]
        pinned: bool,
        /// The text part's media type, when not markdown.
        #[serde(default, rename = "mediaType", skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
        /// Where an imported thread came from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        imported_from: Option<ImportedFrom>,
    },
    /// A post, top-level or in reply to another.
    Post {
        /// The parent post's revision ID, or none for a top-level post.
        #[serde(default, rename = "inReplyTo")]
        in_reply_to: Option<u64>,
        /// A status this post sets.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        /// The text part's media type, when not markdown.
        #[serde(default, rename = "mediaType", skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
        /// 3 to 6: a heading post (0090 §3). Imports only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<u8>,
        /// A range of posts this post collapses beneath its text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        collapse: Option<Range>,
        /// A collapsing post whose collapse this post undoes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uncollapse: Option<u64>,
        /// Where an imported post came from. Imports only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        imported_from: Option<ImportedFrom>,
    },
    /// Replaces the complete text of one post.
    Edit {
        /// The post's revision ID.
        object: u64,
        /// The text part's media type, when not markdown; it must be the post's own.
        #[serde(default, rename = "mediaType", skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
    /// Changes the subject, and so the title.
    Rename {
        /// The new subject.
        name: String,
    },
    /// Moves the thread to a new home.
    Move {
        /// The new home.
        target: Target,
        /// Keeps the old home as a listing, with its pin.
        #[serde(default, skip_serializing_if = "is_false")]
        keep: bool,
    },
    /// Lists the thread on a further page.
    Attach {
        /// The page.
        target: Target,
    },
    /// Removes a listing.
    Detach {
        /// The page.
        target: Target,
    },
    /// Pins the thread on one of its attachments.
    Pin {
        /// The attachment.
        target: Target,
    },
    /// Unpins it there.
    Unpin {
        /// The attachment.
        target: Target,
    },
}

impl ThreadOp {
    /// The operation's name, as written in the content part.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ThreadOp::Create { .. } => "create",
            ThreadOp::Post { .. } => "post",
            ThreadOp::Edit { .. } => "edit",
            ThreadOp::Rename { .. } => "rename",
            ThreadOp::Move { .. } => "move",
            ThreadOp::Attach { .. } => "attach",
            ThreadOp::Detach { .. } => "detach",
            ThreadOp::Pin { .. } => "pin",
            ThreadOp::Unpin { .. } => "unpin",
        }
    }

    /// Whether the record carries a text part: `create`, `post` and `edit`.
    #[must_use]
    pub fn has_text(&self) -> bool {
        matches!(
            self,
            ThreadOp::Create { .. } | ThreadOp::Post { .. } | ThreadOp::Edit { .. }
        )
    }

    /// Whether the record carries a summary in its comment part, and needs a base offset:
    /// every operation but `create` and `post` (14 §1.2).
    #[must_use]
    pub fn needs_base(&self) -> bool {
        !matches!(self, ThreadOp::Create { .. } | ThreadOp::Post { .. })
    }

    /// The content part as JSON.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Reads a content part.
    pub fn from_json(value: serde_json::Value) -> Result<Self, String> {
        serde_json::from_value(value).map_err(|e| e.to_string())
    }
}
