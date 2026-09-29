//! Site aliases (0026 §2): the registry embedded from `docs/registry/sites.toml`, and the
//! mapping between `(site ID, title)` and a page URL.
//!
//! The site ID of a sitelink is its host. A site alias gives a host the MediaWiki site ID,
//! article path and language that Wikidata tooling expects, so that `enwiki` + `Douglas
//! Adams` and `https://en.wikipedia.org/wiki/Douglas_Adams` are one sitelink. There is no
//! site table: an alias exists only for compatibility, and a host without one is linked
//! by URL with the host as its site ID.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use percent_encoding_shim::{decode_title, encode_title};
use serde::Deserialize;

use crate::SITES_TOML;
use crate::entity::Sitelink;

/// One site alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The MediaWiki site ID (`enwiki`).
    pub id: String,
    /// The host, as a Domain key.
    pub host: String,
    /// The article path, with `$1` for the title (`/wiki/$1`).
    pub path: String,
    /// The content language.
    pub language: Option<String>,
    /// The MediaWiki site group, informational.
    pub group: Option<String>,
}

impl Site {
    /// The URL of a title on this site: spaces to underscores, then MediaWiki's
    /// URL-encoding of titles, into the article path, under `https`.
    ///
    /// ```
    /// use scatter_wikibase_model::SiteRegistry;
    /// let en = SiteRegistry::default_registry().by_id("enwiki").unwrap();
    /// assert_eq!(en.url_for_title("Douglas Adams"), "https://en.wikipedia.org/wiki/Douglas_Adams");
    /// assert_eq!(en.url_for_title("Café (bar)"), "https://en.wikipedia.org/wiki/Caf%C3%A9_(bar)");
    /// ```
    #[must_use]
    pub fn url_for_title(&self, title: &str) -> String {
        format!(
            "https://{}{}",
            self.host,
            self.path.replace("$1", &encode_title(title))
        )
    }

    /// The title a URL on this site names, if the URL is under the article path.
    #[must_use]
    pub fn title_for_url(&self, url: &str) -> Option<String> {
        let (prefix, suffix) = self.path.split_once("$1")?;
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?
            .strip_prefix(self.host.as_str())?
            .strip_prefix(prefix)?;
        let rest = rest.strip_suffix(suffix).unwrap_or(rest);
        let rest = rest.split(['?', '#']).next()?;
        if rest.is_empty() {
            return None;
        }
        Some(decode_title(rest))
    }
}

/// The parsed site-alias registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRegistry {
    version: u32,
    sites: Vec<Site>,
    by_id: BTreeMap<String, usize>,
    by_host: BTreeMap<String, usize>,
}

/// Why a registry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SiteRegistryError {
    /// The TOML did not parse or had the wrong shape.
    #[error("sites.toml: {0}")]
    Toml(String),
    /// The file's `version` is one this crate does not read.
    #[error("sites.toml version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// Two aliases share a site ID or a host.
    #[error("sites `{a}` and `{b}` share the {what} `{value}`")]
    Duplicate {
        /// The first site ID.
        a: String,
        /// The second site ID.
        b: String,
        /// `id` or `host`.
        what: &'static str,
        /// The shared value.
        value: String,
    },
    /// The host is not a Domain key.
    #[error("site `{id}`: host `{host}` is not a domain key: {reason}")]
    Host {
        /// The site ID.
        id: String,
        /// The host.
        host: String,
        /// Why.
        reason: String,
    },
    /// The article path has no `$1`.
    #[error("site `{id}`: path `{path}` has no `$1`")]
    Path {
        /// The site ID.
        id: String,
        /// The path.
        path: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[allow(dead_code)]
    generated_from: Option<String>,
    #[allow(dead_code)]
    generated_at: Option<String>,
    #[serde(default, rename = "site")]
    sites: Vec<RawSite>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSite {
    id: String,
    host: String,
    path: String,
    language: Option<String>,
    group: Option<String>,
}

impl SiteRegistry {
    /// Parses and validates a registry in the format of `docs/registry/sites.toml`.
    pub fn parse(text: &str) -> Result<Self, SiteRegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| SiteRegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(SiteRegistryError::Version(raw.version));
        }
        let mut sites = Vec::with_capacity(raw.sites.len());
        let mut by_id = BTreeMap::new();
        let mut by_host = BTreeMap::new();
        for s in raw.sites {
            if !scatter_normalize::domain::is_key(&s.host) {
                let reason = scatter_normalize::domain::normalize(&s.host)
                    .map_or_else(|e| e.to_string(), |k| format!("not canonical; `{k}` is"));
                return Err(SiteRegistryError::Host {
                    id: s.id,
                    host: s.host,
                    reason,
                });
            }
            if !s.path.contains("$1") {
                return Err(SiteRegistryError::Path {
                    id: s.id,
                    path: s.path,
                });
            }
            let i = sites.len();
            let dup =
                |what, value: &str, j: usize, sites: &Vec<Site>| SiteRegistryError::Duplicate {
                    a: sites[j].id.clone(),
                    b: s.id.clone(),
                    what,
                    value: value.to_string(),
                };
            if let Some(j) = by_id.insert(s.id.clone(), i) {
                return Err(dup("id", &s.id, j, &sites));
            }
            if let Some(j) = by_host.insert(s.host.clone(), i) {
                return Err(dup("host", &s.host, j, &sites));
            }
            sites.push(Site {
                id: s.id,
                host: s.host,
                path: s.path,
                language: s.language,
                group: s.group,
            });
        }
        Ok(Self {
            version: raw.version,
            sites,
            by_id,
            by_host,
        })
    }

    /// The embedded default registry, parsed once.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: the default file is validated by a test.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: LazyLock<SiteRegistry> = LazyLock::new(|| {
            SiteRegistry::parse(SITES_TOML).expect("docs/registry/sites.toml is valid")
        });
        &DEFAULT
    }

    /// The file format version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every alias, in registry order.
    #[must_use]
    pub fn sites(&self) -> &[Site] {
        &self.sites
    }

    /// The alias with this MediaWiki site ID.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Site> {
        self.by_id.get(id).map(|&i| &self.sites[i])
    }

    /// The alias for this host.
    #[must_use]
    pub fn by_host(&self, host: &str) -> Option<&Site> {
        self.by_host.get(host).map(|&i| &self.sites[i])
    }

    /// The URL a sitelink names, before normalization: the URL the source wrote, or, for
    /// an aliased site ID, the title on that site. `None` when the source gave no URL and
    /// the site ID has no alias, which is the case 0026 §4 leaves to the provider's own
    /// site table.
    #[must_use]
    pub fn sitelink_url(&self, link: &Sitelink) -> Option<String> {
        if let Some(url) = &link.url {
            return Some(url.clone());
        }
        if let Some(site) = self.by_id(&link.site) {
            return Some(site.url_for_title(&link.title));
        }
        // Keyed by host with the title as path, query and fragment (0026 §2).
        if scatter_normalize::domain::is_key(&link.site) && link.title.starts_with('/') {
            return Some(format!("https://{}{}", link.site, link.title));
        }
        None
    }
}

/// MediaWiki's title ↔ URL encoding, kept local to this module.
mod percent_encoding_shim {
    use std::fmt::Write as _;

    /// Spaces become underscores, then everything outside MediaWiki's unencoded set
    /// (`wfUrlencode`: unreserved characters plus `;:@$!*(),/`) is percent-encoded.
    pub fn encode_title(title: &str) -> String {
        let mut out = String::with_capacity(title.len());
        for b in title.replace(' ', "_").bytes() {
            if b.is_ascii_alphanumeric() || b";:@$!*(),/-_.~".contains(&b) {
                out.push(b as char);
            } else {
                let _ = write!(out, "%{b:02X}");
            }
        }
        out
    }

    /// The reverse: percent-decode, then underscores become spaces.
    pub fn decode_title(path: &str) -> String {
        let bytes: Vec<u8> = percent_decode(path.as_bytes());
        String::from_utf8_lossy(&bytes).replace('_', " ")
    }

    fn percent_decode(b: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && let (Some(h), Some(l)) = (
                    b.get(i + 1).copied().and_then(hex),
                    b.get(i + 2).copied().and_then(hex),
                )
            {
                out.push(h << 4 | l);
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry() {
        let r = SiteRegistry::default_registry();
        assert_eq!(r.version(), 1);
        let en = r.by_id("enwiki").unwrap();
        assert_eq!(en.host, "en.wikipedia.org");
        assert!(std::ptr::eq(r.by_host("en.wikipedia.org").unwrap(), en));
        assert!(r.by_id("testwiki").is_none());
    }

    #[test]
    fn titles_and_urls() {
        let en = SiteRegistry::default_registry().by_id("enwiki").unwrap();
        assert_eq!(
            en.url_for_title("Douglas Adams"),
            "https://en.wikipedia.org/wiki/Douglas_Adams"
        );
        assert_eq!(
            en.title_for_url("https://en.wikipedia.org/wiki/Douglas_Adams"),
            Some("Douglas Adams".into())
        );
        assert_eq!(
            en.title_for_url("https://en.wikipedia.org/wiki/Caf%C3%A9_(bar)#History"),
            Some("Café (bar)".into())
        );
        assert_eq!(en.title_for_url("https://de.wikipedia.org/wiki/X"), None);
        assert_eq!(
            en.title_for_url("https://en.wikipedia.org/w/index.php?title=X"),
            None
        );
        assert_eq!(
            en.url_for_title("100% sure?"),
            "https://en.wikipedia.org/wiki/100%25_sure%3F"
        );
    }

    #[test]
    fn sitelink_urls() {
        let r = SiteRegistry::default_registry();
        let mk = |site: &str, title: &str, url: Option<&str>| Sitelink {
            site: site.into(),
            title: title.into(),
            badges: vec![],
            url: url.map(String::from),
        };
        assert_eq!(
            r.sitelink_url(&mk("enwiki", "Douglas Adams", None))
                .as_deref(),
            Some("https://en.wikipedia.org/wiki/Douglas_Adams")
        );
        assert_eq!(
            r.sitelink_url(&mk(
                "testwiki",
                "Douglas Adams",
                Some("http://127.0.0.1:8080/index.php/Douglas_Adams")
            ))
            .as_deref(),
            Some("http://127.0.0.1:8080/index.php/Douglas_Adams"),
            "a given URL wins"
        );
        assert_eq!(
            r.sitelink_url(&mk("collections.example.museum", "/objects/1?v=2", None))
                .as_deref(),
            Some("https://collections.example.museum/objects/1?v=2")
        );
        assert_eq!(r.sitelink_url(&mk("testwiki", "Douglas Adams", None)), None);
    }

    #[test]
    fn rejects_bad_registries() {
        let one = |host: &str, path: &str| {
            format!("version = 1\n[[site]]\nid = \"xwiki\"\nhost = \"{host}\"\npath = \"{path}\"\n")
        };
        assert!(SiteRegistry::parse(&one("x.example.org", "/wiki/$1")).is_ok());
        assert!(matches!(
            SiteRegistry::parse(&one("X.Example.org", "/wiki/$1")),
            Err(SiteRegistryError::Host { .. })
        ));
        assert!(matches!(
            SiteRegistry::parse(&one("x.example.org", "/wiki/")),
            Err(SiteRegistryError::Path { .. })
        ));
        let two = one("x.example.org", "/wiki/$1")
            + &one("y.example.org", "/wiki/$1").replace("version = 1\n", "");
        assert!(matches!(
            SiteRegistry::parse(&two),
            Err(SiteRegistryError::Duplicate { what: "id", .. })
        ));
    }
}
