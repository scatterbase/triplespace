//! What a wiki contributes to the conversion: its content language, its zone, and the names
//! a signature's user link may use. All of it comes from `meta=siteinfo` or a dump's
//! `<siteinfo>`; a wiki configures nothing else (0090 §5).

use crate::time::Zone;

/// The wiki-specific inputs of the splitter.
#[derive(Debug, Clone)]
pub struct Site {
    /// The content language, which selects the registry's month names.
    pub lang: String,
    /// How the wiki's signature timestamps map to UTC.
    pub zone: Zone,
    /// The names and aliases of the User namespace (2), as siteinfo reports them.
    pub user: Vec<String>,
    /// The names and aliases of the User talk namespace (3).
    pub user_talk: Vec<String>,
    /// The names and aliases of the Special namespace (-1).
    pub special: Vec<String>,
    /// The names and aliases of `Special:Contributions`.
    pub contributions: Vec<String>,
}

impl Site {
    /// English Wikipedia's names, signing in UTC: the default for tests and English wikis.
    pub fn english() -> Site {
        Site {
            lang: "en".into(),
            zone: Zone::utc(),
            user: vec!["User".into()],
            user_talk: vec!["User talk".into()],
            special: vec!["Special".into()],
            contributions: vec!["Contributions".into(), "Contribs".into()],
        }
    }
}

/// Namespace and page names compare case-insensitively, with underscores as spaces.
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> String {
        s.split(|c: char| c == '_' || c.is_whitespace())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    norm(a) == norm(b)
}
