//! Document page records: the `scatter:v0/page` payload type (0008 §4; 10 §3.2).
//!
//! A page record is keyed by its page ID and declares four parts: the three standard
//! ones (payloads.md §2), and a fourth, **`text`**, the page's complete text after the
//! operation, kept apart from the operation so that hiding or erasing an old revision's
//! text touches nothing else, as `scatter:v0/upstream-revision` keeps its text (0015 §4).
//!
//! | Part | Holds |
//! |---|---|
//! | 0 `content` | The operation: [`PageOp`] |
//! | 1 `comment` | The summary |
//! | 2 `attestation` | Actor, job, tags |
//! | 3 `text` | The full text, for `create` and `edit`; absent from `move`, and from a revision whose source withheld it |
//!
//! The content part names the title by namespace number and title, since a namespace's
//! local name can change (Project follows the site's name, 0008 A37). It stores no size
//! or hash: both derive from the text. An adopted revision carries the source's revision
//! IDs and timestamp, as an adopted entity revision does (0035 A12–A14).
//!
//! The crate is pure and builds for `wasm32`. It encodes the content part as JSON; the
//! write path turns it into the record's canonical CBOR.

#![forbid(unsafe_code)]

pub mod diff;
mod models;
mod title;

pub use models::{ContentModel, ContentModels, ModelSource};
pub use title::{MAX_TITLE_BYTES, TitleError, check_title};

use serde::{Deserialize, Serialize};

/// The payload type.
pub const PAYLOAD_PAGE: &str = "scatter:v0/page";

/// The index of the `text` part.
pub const TEXT_PART: usize = 3;

/// Part names an adopted revision's source may have withheld (0035 A14).
pub mod withheld {
    /// The text part.
    pub const TEXT: &str = "text";
    /// The comment part.
    pub const COMMENT: &str = "comment";
    /// The attestation's actor.
    pub const ATTESTATION: &str = "attestation";
}

/// What an adopted revision keeps of its source (0035 A12–A14), beside the source page ID
/// the record's header carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The source's revision ID of this revision, which the record's header also carries.
    pub source_revid: u64,
    /// The revision it followed on the source; absent for the first one adopted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_parentid: Option<u64>,
    /// The source's timestamp, as its own string.
    pub source_time: String,
    /// The parts the source hid, which the record therefore lacks: `text`, `comment`,
    /// `attestation`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withheld: Vec<String>,
}

/// One page operation: the content part of a `scatter:v0/page` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum PageOp {
    /// Mints the page ID, sets the title and content model, and stores the first text.
    Create {
        /// The namespace number.
        ns: i32,
        /// The title within the namespace, in canonical form ([`check_title`]).
        title: String,
        /// The content model.
        model: String,
        /// The source's minor-edit flag, or the editor's.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        minor: bool,
        /// For an adopted revision, what it keeps of its source.
        #[serde(default, skip_serializing_if = "Option::is_none", flatten)]
        source: Option<Source>,
    },
    /// Stores the page's complete new text, and a new model when it changes.
    Edit {
        /// The new content model, when it changes (`action=changecontentmodel`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// The minor-edit flag.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        minor: bool,
        /// For an adopted revision, what it keeps of its source.
        #[serde(default, skip_serializing_if = "Option::is_none", flatten)]
        source: Option<Source>,
    },
    /// Sets a new title.
    Move {
        /// The new namespace number.
        ns: i32,
        /// The new title within it.
        title: String,
        /// The page ID of the redirect the move left at the old title, when it left one
        /// (10 §2.5): minted before the move's record is appended, so that the move's log
        /// event can name it. Absent on a move that left none, and on every record
        /// written before the field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        redirect: Option<u64>,
    },
}

impl PageOp {
    /// The operation's name, as written in the content part.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            PageOp::Create { .. } => "create",
            PageOp::Edit { .. } => "edit",
            PageOp::Move { .. } => "move",
        }
    }

    /// What an adopted revision keeps of its source.
    #[must_use]
    pub fn source(&self) -> Option<&Source> {
        match self {
            PageOp::Create { source, .. } | PageOp::Edit { source, .. } => source.as_ref(),
            PageOp::Move { .. } => None,
        }
    }

    /// The minor-edit flag.
    #[must_use]
    pub fn minor(&self) -> bool {
        match self {
            PageOp::Create { minor, .. } | PageOp::Edit { minor, .. } => *minor,
            PageOp::Move { .. } => false,
        }
    }

    /// Whether the record carries a text part: `create` and `edit`, unless the source
    /// withheld it.
    #[must_use]
    pub fn has_text(&self) -> bool {
        !matches!(self, PageOp::Move { .. })
            && !self
                .source()
                .is_some_and(|s| s.withheld.iter().any(|w| w == withheld::TEXT))
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

/// Why an operation is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// The title is not a valid title.
    Title(TitleError),
    /// The model is not a registered, implemented text model.
    Model(String),
    /// The text is missing from an operation that needs it, or present on one that may
    /// not carry it.
    Text,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Title(e) => write!(f, "{e}"),
            Invalid::Model(m) => write!(f, "`{m}` is not a text content model"),
            Invalid::Text => f.write_str("the text part does not match the operation"),
        }
    }
}

/// Checks an operation and its text: a canonical title, a model that stores text, and a
/// text part exactly where the operation carries one. Whether the namespace allows the
/// model is the caller's check, since the namespace registry is not this crate's.
pub fn validate(op: &PageOp, text: Option<&str>) -> Result<(), Invalid> {
    let models = ContentModels::embedded();
    let text_model = |m: &str| {
        models
            .get(m)
            .is_some_and(|m| m.source == Some(ModelSource::Text) && m.implemented)
    };
    match op {
        PageOp::Create {
            title, model, ns, ..
        } => {
            check_title(*ns, title).map_err(Invalid::Title)?;
            if !text_model(model) {
                return Err(Invalid::Model(model.clone()));
            }
        }
        PageOp::Edit { model, .. } => {
            if let Some(m) = model
                && !text_model(m)
            {
                return Err(Invalid::Model(m.clone()));
            }
        }
        PageOp::Move { ns, title, .. } => check_title(*ns, title).map_err(Invalid::Title)?,
    }
    if op.has_text() != text.is_some() {
        return Err(Invalid::Text);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn content_parts() {
        let create = PageOp::Create {
            ns: 4,
            title: "Scope".into(),
            model: "wikitext".into(),
            minor: false,
            source: None,
        };
        assert_eq!(
            create.to_json(),
            json!({"op": "create", "ns": 4, "title": "Scope", "model": "wikitext"})
        );
        let edit = PageOp::Edit {
            model: None,
            minor: true,
            source: None,
        };
        assert_eq!(edit.to_json(), json!({"op": "edit", "minor": true}));
        let adopted = PageOp::Edit {
            model: None,
            minor: false,
            source: Some(Source {
                source_revid: 248_229,
                source_parentid: Some(248_228),
                source_time: "2026-06-29T04:02:35Z".into(),
                withheld: vec!["text".into()],
            }),
        };
        let j = adopted.to_json();
        assert_eq!(
            j,
            json!({"op": "edit", "source_revid": 248_229, "source_parentid": 248_228,
                   "source_time": "2026-06-29T04:02:35Z", "withheld": ["text"]})
        );
        assert_eq!(PageOp::from_json(j).unwrap(), adopted);
        assert!(!adopted.has_text());
        let mv = PageOp::from_json(json!({"op": "move", "ns": 4, "title": "New"})).unwrap();
        assert_eq!(mv.name(), "move");
        assert!(PageOp::from_json(json!({"op": "delete"})).is_err());
    }

    #[test]
    fn validation() {
        let create = |title: &str, model: &str| PageOp::Create {
            ns: 4,
            title: title.into(),
            model: model.into(),
            minor: false,
            source: None,
        };
        assert_eq!(validate(&create("Scope", "wikitext"), Some("x")), Ok(()));
        assert_eq!(
            validate(&create("Scope", "wikibase-item"), Some("x")),
            Err(Invalid::Model("wikibase-item".into()))
        );
        assert!(matches!(
            validate(&create("a|b", "wikitext"), Some("x")),
            Err(Invalid::Title(_))
        ));
        assert_eq!(
            validate(&create("Scope", "wikitext"), None),
            Err(Invalid::Text)
        );
        let mv = PageOp::Move {
            ns: 4,
            title: "New".into(),
            redirect: None,
        };
        assert_eq!(validate(&mv, None), Ok(()));
        assert_eq!(validate(&mv, Some("x")), Err(Invalid::Text));
    }
}
