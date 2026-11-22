//! Badges as the skin draws them (0114 §4; 19 §1.6): one component for every badge, on an
//! entity, a property, a value, a sitelink or a statement.
//!
//! The icons are Codex's (`@wikimedia/codex-icons` 2.8.0), by the names the badge registry
//! gives (`check`, `star`, …): their SVG paths are copied here under that package's MIT
//! licence, Copyright (c) 2011-2022 Wikimedia Design & OOUI team and other contributors.

use serde::Serialize;

/// The icons a badge may name, as the inside of a 20×20 SVG.
const ICONS: &[(&str, &str)] = &[
    (
        "check",
        "<path d=\"M18.154 3.837 8 16.8H6.65l-4.8-3.6 1.2-1.6 4.02 3.015 9.517-12.02z\"/>",
    ),
    (
        "cancel",
        "<path d=\"M10 1a9 9 0 110 18 9 9 0 010-18M4.394 5.806A6.97 6.97 0 003 10a7 7 0 0011.193 5.605l-9.8-9.8ZM10 3a6.97 6.97 0 00-4.191 1.392l9.797 9.798A7 7 0 0010 3\"/>",
    ),
    (
        "star",
        "<path d=\"M12.9 7H19v1.588l-4.593 3.589L16.5 19h-2L10 15.5 5.5 19h-2l2.092-6.823L1 8.587V7h6.1l1.945-5.995h1.91zM8.6 9H4.5l3.415 2.435-1.289 4.17L10 13l3.373 2.605-1.288-4.17L15.5 9h-4.05L10 4.687z\"/>",
    ),
    (
        "heart",
        "<path d=\"M11.039 3.656c2.158-2.332 5.937-2.179 7.893.318a4.99 4.99 0 01-.272 6.487L10.757 19H9.243L1.34 10.461a4.99 4.99 0 01-.272-6.487c1.956-2.497 5.735-2.65 7.893-.318L10 4.776z\"/>",
    ),
    (
        "checkAll",
        "<path d=\"M14 17h-4v-2h4zm2.404-13.163L6.22 16.8H4.9L.1 13.2l1.2-1.6 4.02 3.015 9.517-12.02zM17 13h-4v-2h4zm3-4h-4V7h4z\"/>",
    ),
    (
        "alert",
        "<path d=\"M9.5 1h1L19 17.2V19H1v-1.8zM9 7v6h2V7zm0 8v2h2v-2z\"/>",
    ),
    (
        "article",
        "<path d=\"M2 0h16v20H2zm2 2v16h12V2zm2 2h3v1H6zm0 2h3v1H6zm0 2h3v1H6zm0 3h8v1H6zm0 2h8v1H6zm0 2h8v1H6zm4-11h4v5h-4z\"/>",
    ),
    (
        "articleRedirect",
        "<path d=\"M18 20H2V0h16zM4 2c0 2.393.483 4.507 1.661 6.007 1.002 1.274 2.605 2.229 5.188 2.445L8.896 8.5l1.414-1.414 3.708 3.707v1.414l-3.708 3.707L8.896 14.5l2.038-2.038c-3.16-.211-5.396-1.374-6.845-3.219L4 9.123V18h12V2z\"/>",
    ),
    (
        "tag",
        "<path d=\"m19.914 11.503-8.413 8.414L.004 8.414.003 1l1-1h7.415zM2.003 7.587l9.497 9.502 5.585-5.587-9.496-9.501L2.003 2z\"/><circle cx=\"5.5\" cy=\"5.5\" r=\"1.5\"/>",
    ),
];

/// The inside of a 20×20 SVG for a Codex icon the badges use, by its name; `None` for one
/// this list does not carry.
#[must_use]
pub fn icon(name: &str) -> Option<&'static str> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, svg)| *svg)
}

/// A badge as the skin's `codex.badge` macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BadgeModel {
    /// The `class` attribute.
    pub classes: String,
    /// The text: the badge's message, else its item's label, else its item's ID.
    pub text: String,
    /// The tooltip: the message's tip, else the item's description.
    pub tip: Option<String>,
    /// The item's page, which the badge links to.
    pub href: Option<String>,
    /// The icon, the inside of a 20×20 SVG (`_html`: drawn by the binary, 0097 A4).
    pub icon_html: Option<String>,
}

impl BadgeModel {
    /// A badge; `best` marks the "Best value" badge, which keeps its colours.
    #[must_use]
    pub fn new(
        text: String,
        tip: Option<String>,
        href: Option<String>,
        icon_name: &str,
        best: bool,
    ) -> Self {
        Self {
            classes: if best {
                "ts-chip ts-chip--badge ts-chip--best".into()
            } else {
                "ts-chip ts-chip--badge".into()
            },
            text,
            tip,
            href,
            icon_html: icon(icon_name).map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0114 §4: every icon the default badge registry names is carried.
    #[test]
    fn the_registry_icons_are_carried() {
        for b in scatter_wikibase_fusion::badges::BadgeRegistry::default_registry().badges() {
            assert!(icon(&b.icon).is_some(), "{}", b.icon);
        }
        assert!(icon("tag").is_some(), "the default for a tenant's badge");
        assert!(icon("nope").is_none());
    }
}
