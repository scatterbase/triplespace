//! What the site offers: a feature shows only where this build of the site has built it
//! and the API says it serves it (0057 §9; 0012 §1.4). Links, tabs, menus and special
//! pages read [`Features`], so a page never leads to something that is not there.
//!
//! The API's half is `siprop=triplespace`'s `capabilities` list. The site's half is the
//! [`BUILT`] list below, which grows as the phases of the web frontend plan land.

use triplespace_client::SiteInfo;

/// A feature, by the API capability it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `Special:Search` and the header's box (`wbsearchentities`).
    Search,
    /// Logging in and out (`clientlogin`).
    Login,
    /// Editing entities (`wbeditentity`).
    Edit,
    /// Creating entities: **New item**, **New property**.
    Create,
    /// Entity and page histories.
    History,
    /// `Special:RecentChanges`.
    RecentChanges,
    /// `Special:Jobs` and job pages.
    Jobs,
    /// `Special:Account`.
    Account,
    /// Document pages: user pages, project pages, **New page**.
    Pages,
}

impl Feature {
    /// The API capability the feature needs.
    #[must_use]
    pub fn capability(self) -> &'static str {
        match self {
            Self::Search => "wbsearchentities",
            Self::Login => "clientlogin",
            Self::Edit | Self::Create => "wbeditentity",
            Self::History => "history",
            Self::RecentChanges => "recentchanges",
            Self::Jobs => "jobs",
            Self::Account => "account",
            Self::Pages => "pages",
        }
    }
}

/// The features this build of the site has built.
pub const BUILT: &[Feature] = &[Feature::Search];

/// What the site offers on one request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features {
    on: Vec<Feature>,
}

impl Features {
    /// The features both built here and served by the API `site` describes.
    #[must_use]
    pub fn of(site: &SiteInfo) -> Self {
        Self::from_capabilities(&site.capabilities)
    }

    /// The features both built here and among `capabilities`.
    #[must_use]
    pub fn from_capabilities(capabilities: &[String]) -> Self {
        Self {
            on: BUILT
                .iter()
                .copied()
                .filter(|f| capabilities.iter().any(|c| c == f.capability()))
                .collect(),
        }
    }

    /// Every feature this build has built, as if the API served them all: for tests of
    /// the markup.
    #[must_use]
    pub fn all_built() -> Self {
        Self { on: BUILT.to_vec() }
    }

    /// Whether a feature is offered.
    #[must_use]
    pub fn has(&self, f: Feature) -> bool {
        self.on.contains(&f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_needs_both_halves() {
        let f = Features::from_capabilities(&["wbsearchentities".into(), "history".into()]);
        assert!(f.has(Feature::Search));
        assert!(
            !f.has(Feature::History),
            "the API serves it; the site has not built it"
        );
        assert!(!Features::from_capabilities(&[]).has(Feature::Search));
    }
}
