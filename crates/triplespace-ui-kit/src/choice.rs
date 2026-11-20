//! Links offered side by side: a choice, a switch of them and a pager (19 §5.10). Every
//! page kind draws these, so they belong to none.

use serde::Serialize;

/// A link in a switch or a pager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// The page.
    pub href: String,
    /// The text.
    pub label: String,
    /// Whether it is the current one.
    pub current: bool,
}

/// The source switch (19 §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Switch {
    /// The accessible name.
    pub label: String,
    /// *All sources*, *Made here*, then each mirror.
    pub items: Vec<Choice>,
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pager {
    /// The accessible name.
    pub label: String,
    /// The first page.
    pub newest: Option<Choice>,
    /// The next page.
    pub older: Option<Choice>,
}
