//! What the parser and renderer need to know about a site: its namespaces, interwiki
//! prefixes, URL protocols, extension tags, behaviour switches and interface messages.
//!
//! The crate does not read the registries itself (22 §2.1 lists no dependency for it);
//! the caller builds a [`ConfigSource`] from `namespaces.toml`, `wikitext-functions.toml`
//! and the tenant's settings, and [`Config::new`] turns it into the parser's
//! configuration (0033 §9.1).

/// One namespace, with the names a link may use for it.
#[derive(Clone, Debug)]
pub struct Namespace {
    /// Its number.
    pub id: i32,
    /// The local name, as titles are written (`Project` or the site's name); empty for
    /// the main namespace.
    pub name: String,
    /// The canonical name, where it differs from the local one.
    pub canonical: Option<String>,
    /// Other names that resolve to it.
    pub aliases: Vec<String>,
    /// Whether the first letter of a title keeps its case. MediaWiki's default, and
    /// `namespaces.toml`'s `first-letter` normalizer, is `false`: it is uppercased.
    pub case_sensitive: bool,
    /// Whether `[[/Sub]]` links are relative to the page.
    pub subpages: bool,
}

impl Namespace {
    /// A namespace whose first letter is uppercased and that has subpages unless it is
    /// the main namespace.
    #[must_use]
    pub fn new(id: i32, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            canonical: None,
            aliases: Vec::new(),
            case_sensitive: false,
            subpages: id != 0,
        }
    }

    /// Adds a canonical name.
    #[must_use]
    pub fn canonical(mut self, canonical: &str) -> Self {
        self.canonical = Some(canonical.to_string());
        self
    }

    /// Adds an alias.
    #[must_use]
    pub fn alias(mut self, alias: &str) -> Self {
        self.aliases.push(alias.to_string());
        self
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str())
            .chain(self.canonical.as_deref())
            .chain(self.aliases.iter().map(String::as_str))
            .filter(|n| !n.is_empty())
    }
}

/// An interwiki prefix and the URL its titles go to, with `$1` for the title.
#[derive(Clone, Debug)]
pub struct Interwiki {
    /// The prefix, matched without regard to case.
    pub prefix: String,
    /// The URL, with `$1` where the encoded title goes.
    pub url: String,
}

/// Interface text the renderer writes into page output.
#[derive(Clone, Debug)]
pub struct Messages {
    /// The content language, written on the TOC title as MediaWiki does.
    pub lang: String,
    /// The content language's direction.
    pub dir: String,
    /// The TOC's title (`toc`: "Contents").
    pub toc: String,
    /// The line above a redirect's target (`redirectto`: "Redirect to:").
    pub redirect_to: String,
    /// A red link's tooltip, with `$1` for the title (`red-link-title`).
    pub missing_page: String,
    /// The tooltip of the chip a template call renders as while it is not expanded (0010 §4).
    pub chip_template: String,
    /// The tooltip of the chip any other construct outside the subset renders as.
    pub chip_unsupported: String,
}

impl Default for Messages {
    fn default() -> Self {
        Self {
            lang: "en".into(),
            dir: "ltr".into(),
            toc: "Contents".into(),
            redirect_to: "Redirect to:".into(),
            missing_page: "$1 (page does not exist)".into(),
            chip_template: "Template not rendered".into(),
            chip_unsupported: "Not rendered".into(),
        }
    }
}

/// Everything [`Config::new`] needs.
#[derive(Clone, Debug)]
pub struct ConfigSource {
    /// The site's namespaces.
    pub namespaces: Vec<Namespace>,
    /// Interwiki prefixes.
    pub interwiki: Vec<Interwiki>,
    /// URL protocols external links and bare URLs may use (`$wgUrlProtocols`).
    pub protocols: Vec<String>,
    /// Extension tags the parser recognises. Their content is not wiki text where the
    /// renderer says so (`nowiki`, `pre`, `syntaxhighlight`, `source`); the rest render as
    /// chips unless [`crate::render`] implements them.
    pub extension_tags: Vec<String>,
    /// Behaviour switches, the words between `__` and `__`.
    pub switches: Vec<String>,
    /// Words that start a redirect line (`#REDIRECT`), without the `#`.
    pub redirect_words: Vec<String>,
    /// The letters a link's trail may hold (`[[cat]]s`).
    pub link_trail: String,
    /// Interface text.
    pub messages: Messages,
}

impl Default for ConfigSource {
    /// MediaWiki's defaults for an English wiki: the core namespaces, no interwiki
    /// prefixes, `$wgUrlProtocols`, the extension tags and switches of
    /// `wikitext-functions.toml`, and English messages.
    #[allow(clippy::too_many_lines)] // the lists are data
    fn default() -> Self {
        let ns = |id, name: &str| Namespace::new(id, name);
        let mut file = ns(6, "File").alias("Image");
        file.subpages = false;
        let mut special = ns(-1, "Special");
        special.subpages = false;
        let mut media = ns(-2, "Media");
        media.subpages = false;
        Self {
            namespaces: vec![
                media,
                special,
                ns(0, ""),
                ns(1, "Talk"),
                ns(2, "User"),
                ns(3, "User talk"),
                ns(4, "Project"),
                ns(5, "Project talk"),
                file,
                ns(7, "File talk").alias("Image talk"),
                ns(8, "MediaWiki"),
                ns(9, "MediaWiki talk"),
                ns(10, "Template"),
                ns(11, "Template talk"),
                ns(12, "Help"),
                ns(13, "Help talk"),
                ns(14, "Category"),
                ns(15, "Category talk"),
            ],
            interwiki: Vec::new(),
            protocols: [
                "bitcoin:",
                "ftp://",
                "ftps://",
                "geo:",
                "git://",
                "gopher://",
                "http://",
                "https://",
                "irc://",
                "ircs://",
                "magnet:",
                "mailto:",
                "matrix:",
                "mms://",
                "news:",
                "nntp://",
                "redis://",
                "sftp://",
                "sip:",
                "sips:",
                "sms:",
                "ssh://",
                "svn://",
                "tel:",
                "telnet://",
                "urn:",
                "worldwind://",
                "xmpp:",
                "//",
            ]
            .map(String::from)
            .to_vec(),
            extension_tags: [
                "nowiki",
                "pre",
                "gallery",
                "indicator",
                "ref",
                "references",
                "poem",
                "syntaxhighlight",
                "source",
                "templatestyles",
                "templatedata",
                "math",
                "chem",
                "ce",
                "score",
                "timeline",
                "graph",
                "mapframe",
                "maplink",
                "categorytree",
                "inputbox",
                "imagemap",
                "hiero",
                "charinsert",
                "section",
                "includeonly",
                "noinclude",
                "onlyinclude",
            ]
            .map(String::from)
            .to_vec(),
            switches: [
                "NOTOC",
                "TOC",
                "FORCETOC",
                "HIDDENCAT",
                "NOEDITSECTION",
                "NEWSECTIONLINK",
                "NONEWSECTIONLINK",
                "NOGALLERY",
                "INDEX",
                "NOINDEX",
                "STATICREDIRECT",
                "EXPECTUNUSEDCATEGORY",
                "NOCONTENTCONVERT",
                "NOCC",
                "NOTITLECONVERT",
                "NOTC",
                "DISAMBIG",
                "EXPECTED_UNCONNECTED_PAGE",
            ]
            .map(String::from)
            .to_vec(),
            redirect_words: vec!["REDIRECT".into()],
            link_trail: "abcdefghijklmnopqrstuvwxyz".into(),
            messages: Messages::default(),
        }
    }
}

/// A site's parsing and rendering configuration.
pub struct Config {
    /// The source it was built from.
    pub source: ConfigSource,
    pub(crate) parser: parse_wiki_text_2::Configuration,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// The main namespace's number.
pub const MAIN: i32 = 0;
/// The File namespace's number.
pub const FILE: i32 = 6;
/// The Category namespace's number.
pub const CATEGORY: i32 = 14;

impl Config {
    /// Builds the parser's configuration from `source`.
    #[must_use]
    pub fn new(source: ConfigSource) -> Self {
        let names = |id: i32| -> Vec<String> {
            source
                .namespaces
                .iter()
                .filter(|n| n.id == id)
                .flat_map(|n| n.names().map(str::to_lowercase).collect::<Vec<_>>())
                .flat_map(|n| {
                    let underscored = n.replace(' ', "_");
                    if underscored == n {
                        vec![n]
                    } else {
                        vec![n, underscored]
                    }
                })
                .collect()
        };
        let category = names(CATEGORY);
        let file = names(FILE);
        let parser =
            parse_wiki_text_2::Configuration::new(&parse_wiki_text_2::ConfigurationSource {
                category_namespaces: &as_str(&category),
                extension_tags: &as_str(&source.extension_tags),
                file_namespaces: &as_str(&file),
                link_trail: &source.link_trail,
                magic_words: &as_str(&source.switches),
                protocols: &as_str(&source.protocols),
                redirect_magic_words: &as_str(&source.redirect_words),
            });
        Self { source, parser }
    }

    /// The namespace with this number.
    #[must_use]
    pub fn namespace(&self, id: i32) -> Option<&Namespace> {
        self.source.namespaces.iter().find(|n| n.id == id)
    }

    /// The namespace a prefix names, ignoring case and treating `_` as a space.
    #[must_use]
    pub fn namespace_by_name(&self, prefix: &str) -> Option<&Namespace> {
        let want = normalize_name(prefix);
        self.source
            .namespaces
            .iter()
            .find(|n| n.names().any(|name| normalize_name(name) == want))
    }

    /// The interwiki entry a prefix names, ignoring case.
    #[must_use]
    pub fn interwiki(&self, prefix: &str) -> Option<&Interwiki> {
        let want = prefix.trim().to_lowercase();
        self.source
            .interwiki
            .iter()
            .find(|i| i.prefix.to_lowercase() == want)
    }

    /// Interface text.
    #[must_use]
    pub fn messages(&self) -> &Messages {
        &self.source.messages
    }
}

fn as_str(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn normalize_name(s: &str) -> String {
    s.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
