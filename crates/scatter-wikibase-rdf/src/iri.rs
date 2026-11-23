//! The IRIs Wikibase's RDF names things by (wikibase-compat.md §5.1; 02 §2.3; 05 §1.5).
//!
//! Prefix names are conventional and not the contract; the IRIs are. An [`IriScheme`] holds
//! the instance's bases and an [`EntityIris`], which names the entities that are not named
//! under those bases, and writes every node and predicate IRI the renderer needs. A stock
//! Wikibase names every entity under its own base ([`OwnBase`]); Triplespace's
//! implementation, for its foreign and keyed IDs, is in `scatter-wikibase-ids` (0115 §15).

use std::fmt::Write as _;

use std::sync::Arc;

use oxrdf::NamedNode;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::{EntityId, StatementId};

/// The bases an instance writes its own nodes under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespaces {
    /// The concept base without `entity/`, ending in `/`: concept IRIs are
    /// `{base}entity/Q1` and predicates `{base}prop/direct/P1`.
    pub base: String,
    /// The article path, ending in `/`, under which `Special:EntityData/` and
    /// `Special:FilePath/` live: `{base}wiki/` on Triplespace, `{script}/index.php/` on a
    /// stock Wikibase.
    pub article_path: String,
}

impl Namespaces {
    /// Bases for an instance at `base` (`https://example.org/`) with articles at
    /// `{base}wiki/`, as Wikidata and Triplespace lay them out (0004 §4).
    #[must_use]
    pub fn with_wiki_path(base: &str) -> Self {
        let base = with_slash(base);
        Self {
            article_path: format!("{base}wiki/"),
            base,
        }
    }

    /// Bases given in full, for any other layout: a stock Wikibase's
    /// `{script}/index.php/`, say.
    #[must_use]
    pub fn new(base: &str, article_path: &str) -> Self {
        Self {
            base: with_slash(base),
            article_path: with_slash(article_path),
        }
    }

    /// `{base}entity/`.
    #[must_use]
    pub fn entity(&self) -> String {
        format!("{}entity/", self.base)
    }

    /// `{article path}Special:EntityData/`, the document prefix.
    #[must_use]
    pub fn data(&self) -> String {
        format!("{}Special:EntityData/", self.article_path)
    }

    /// `{base}entity/statement/`.
    #[must_use]
    pub fn statement(&self) -> String {
        format!("{}entity/statement/", self.base)
    }

    /// `{base}reference/`.
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{}reference/", self.base)
    }

    /// `{base}value/`.
    #[must_use]
    pub fn value(&self) -> String {
        format!("{}value/", self.base)
    }

    /// `{article path}Special:FilePath/`, where `localMedia` values point (0039 §12).
    #[must_use]
    pub fn file_path(&self) -> String {
        format!("{}Special:FilePath/", self.article_path)
    }
}

fn with_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}

/// One of the thirteen predicate families a property has (wikibase-compat.md §5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// `wdt:`, entity → simple value, best rank only.
    Direct,
    /// `wdtn:`.
    DirectNormalized,
    /// `p:`, entity → statement.
    Claim,
    /// `ps:`.
    Statement,
    /// `psv:`.
    StatementValue,
    /// `psn:`.
    StatementValueNormalized,
    /// `pq:`.
    Qualifier,
    /// `pqv:`.
    QualifierValue,
    /// `pqn:`.
    QualifierValueNormalized,
    /// `pr:`.
    Reference,
    /// `prv:`.
    ReferenceValue,
    /// `prn:`.
    ReferenceValueNormalized,
    /// `wdno:`, the class of things with no value for the property.
    NoValue,
}

impl Family {
    /// Every family, in prefix-declaration order.
    pub const ALL: [Self; 13] = [
        Self::Direct,
        Self::DirectNormalized,
        Self::Claim,
        Self::Statement,
        Self::StatementValue,
        Self::StatementValueNormalized,
        Self::Qualifier,
        Self::QualifierValue,
        Self::QualifierValueNormalized,
        Self::Reference,
        Self::ReferenceValue,
        Self::ReferenceValueNormalized,
        Self::NoValue,
    ];

    /// The path under the vocabulary base.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Direct => "prop/direct/",
            Self::DirectNormalized => "prop/direct-normalized/",
            Self::Claim => "prop/",
            Self::Statement => "prop/statement/",
            Self::StatementValue => "prop/statement/value/",
            Self::StatementValueNormalized => "prop/statement/value-normalized/",
            Self::Qualifier => "prop/qualifier/",
            Self::QualifierValue => "prop/qualifier/value/",
            Self::QualifierValueNormalized => "prop/qualifier/value-normalized/",
            Self::Reference => "prop/reference/",
            Self::ReferenceValue => "prop/reference/value/",
            Self::ReferenceValueNormalized => "prop/reference/value-normalized/",
            Self::NoValue => "prop/novalue/",
        }
    }

    /// The conventional prefix name.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Direct => "wdt",
            Self::DirectNormalized => "wdtn",
            Self::Claim => "p",
            Self::Statement => "ps",
            Self::StatementValue => "psv",
            Self::StatementValueNormalized => "psn",
            Self::Qualifier => "pq",
            Self::QualifierValue => "pqv",
            Self::QualifierValueNormalized => "pqn",
            Self::Reference => "pr",
            Self::ReferenceValue => "prv",
            Self::ReferenceValueNormalized => "prn",
            Self::NoValue => "wdno",
        }
    }
}

/// Where a property's predicates live: a vocabulary base and the property's local name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyIris {
    base: String,
    local: String,
}

impl PropertyIris {
    /// The predicate of `family` for this property.
    #[must_use]
    pub fn iri(&self, family: Family) -> NamedNode {
        NamedNode::new_unchecked(format!("{}{}{}", self.base, family.path(), self.local))
    }
}

/// Names the entities an instance does not name under its own base (0115 §15).
///
/// Every method has a default that names the entity under the instance's base, which is
/// what a stock Wikibase does with every ID: implement only what differs.
pub trait EntityIris: Send + Sync {
    /// The concept IRI of an entity named elsewhere: a mirrored entity's upstream IRI, say.
    /// `None` names it `{base}entity/{id}`.
    fn concept(&self, _id: &EntityId) -> Option<NamedNode> {
        None
    }

    /// For a property named elsewhere, its concept base and its local name there, as
    /// `("http://www.wikidata.org/entity/", "P31")`: its predicates are named by Wikibase's
    /// rule from that base ([`predicate_base`]). `None` names them under the instance's
    /// base, by the property's ID.
    fn property(&self, _id: &EntityId) -> Option<(String, String)> {
        None
    }
}

/// Every entity named under the instance's own base, as a stock Wikibase names them.
#[derive(Debug, Clone, Copy, Default)]
pub struct OwnBase;

impl EntityIris for OwnBase {}

/// The nodes of a statement named as its upstream names them (05 §1.5; 0002 A43), resolved
/// by the caller and given to [`crate::Renderer::with_upstream`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamNodes {
    /// The statement node.
    pub statement: NamedNode,
    /// The base its reference and value nodes are under: `http://www.wikidata.org/`.
    pub base: String,
    /// The provider's hasher, whose ID forms the node hashes are taken over.
    pub hasher: Hasher,
}

/// The IRIs of one instance (or one tenant), for one rendering.
#[derive(Clone)]
pub struct IriScheme<'r> {
    ns: Namespaces,
    entities: Arc<dyn EntityIris + 'r>,
}

impl std::fmt::Debug for IriScheme<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IriScheme")
            .field("ns", &self.ns)
            .finish_non_exhaustive()
    }
}

impl<'r> IriScheme<'r> {
    /// A scheme over `ns`, naming entities outside its bases through `entities`.
    #[must_use]
    pub fn new(ns: Namespaces, entities: impl EntityIris + 'r) -> Self {
        Self {
            ns,
            entities: Arc::new(entities),
        }
    }

    /// A scheme over `ns` naming every entity under it, as a stock Wikibase does
    /// ([`OwnBase`]).
    #[must_use]
    pub fn own(ns: Namespaces) -> IriScheme<'static> {
        IriScheme::new(ns, OwnBase)
    }

    /// The bases.
    #[must_use]
    pub const fn namespaces(&self) -> &Namespaces {
        &self.ns
    }

    /// An entity's concept IRI: the one [`EntityIris::concept`] gives, or else
    /// `{base}entity/{id}`.
    #[must_use]
    pub fn concept(&self, id: &EntityId) -> NamedNode {
        self.entities.concept(id).unwrap_or_else(|| {
            NamedNode::new_unchecked(format!("{}{}", self.ns.entity(), escape_iri(id.as_str())))
        })
    }

    /// An entity's document node, `{article path}Special:EntityData/{id}`, with the ID as
    /// the tenant writes it (02 §2.3; 04 §3.5).
    #[must_use]
    pub fn document(&self, id: &EntityId) -> NamedNode {
        NamedNode::new_unchecked(format!("{}{}", self.ns.data(), escape_iri(id.as_str())))
    }

    /// Where a property's predicates live, by Wikibase's own rule
    /// (`RdfVocabulary::getConceptUriBase`): take the concept base, the part of the
    /// concept IRI before the ID, drop a trailing `entity/` if there is one, and append
    /// `prop/…`. A property named under the instance's base has the concept base
    /// `{base}entity/`, so its predicates are `{base}prop/direct/P7`; one
    /// [`EntityIris::property`] names elsewhere keeps that base's,
    /// `http://www.wikidata.org/prop/direct/P31` (05 §1.5; 0002 A42).
    #[must_use]
    pub fn property(&self, id: &EntityId) -> PropertyIris {
        if let Some((concept_base, local)) = self.entities.property(id) {
            return PropertyIris {
                base: predicate_base(&concept_base),
                local: escape_iri(&local),
            };
        }
        PropertyIris {
            base: predicate_base(&self.ns.entity()),
            local: escape_iri(id.as_str()),
        }
    }

    /// A statement node, `{base}entity/statement/{ID with $ as -}`.
    #[must_use]
    pub fn statement(&self, id: &StatementId) -> NamedNode {
        NamedNode::new_unchecked(format!(
            "{}{}",
            self.ns.statement(),
            escape_iri(&id.as_str().replace('$', "-"))
        ))
    }

    /// A reference node, `{base}reference/{hash}`.
    #[must_use]
    pub fn reference(&self, hash: &str) -> NamedNode {
        NamedNode::new_unchecked(format!("{}{hash}", self.ns.reference()))
    }

    /// A full value node, `{base}value/{hash}`.
    #[must_use]
    pub fn value(&self, hash: &str) -> NamedNode {
        NamedNode::new_unchecked(format!("{}{hash}", self.ns.value()))
    }

    /// An IRI a data value carries as a string: a quantity's unit, a globe, a calendar
    /// model. An absolute IRI is kept; an entity ID, which is how a client may write one,
    /// becomes its concept IRI.
    #[must_use]
    pub fn value_iri(&self, s: &str) -> NamedNode {
        if s.contains("://")
            && let Ok(n) = NamedNode::new(s)
        {
            return n;
        }
        if let Ok(id) = EntityId::parse(s) {
            return self.concept(&id);
        }
        NamedNode::new_unchecked(escape_iri(s))
    }

    /// The prefixes a Turtle rendering declares, Wikibase's names for this scheme's
    /// bases (wikibase-compat.md §5.1).
    #[must_use]
    pub fn prefixes(&self) -> Vec<(String, String)> {
        use scatter_vocab::{cc, geo, owl, prov, rdf, rdfs, schema, skos, wikibase, xsd};
        let mut out: Vec<(String, String)> = [
            (rdf::PREFIX, rdf::NS),
            (xsd::PREFIX, xsd::NS),
            (rdfs::PREFIX, rdfs::NS),
            (owl::PREFIX, owl::NS),
            (wikibase::PREFIX, wikibase::NS),
            (skos::PREFIX, skos::NS),
            (schema::PREFIX, schema::NS),
            (cc::PREFIX, cc::NS),
            (geo::PREFIX, geo::NS),
            (prov::PREFIX, prov::NS),
        ]
        .into_iter()
        .map(|(p, n)| (p.to_string(), n.to_string()))
        .collect();
        out.push(("wd".into(), self.ns.entity()));
        out.push(("data".into(), self.ns.data()));
        out.push(("s".into(), self.ns.statement()));
        out.push(("ref".into(), self.ns.reference()));
        out.push(("v".into(), self.ns.value()));
        for f in Family::ALL {
            out.push((f.prefix().into(), format!("{}{}", self.ns.base, f.path())));
        }
        out
    }
}

/// The base a Wikibase writes its predicates under, from its concept base: the concept
/// base without a trailing `entity/`, or the concept base itself
/// (`RdfVocabulary::getConceptUriBase`).
#[must_use]
pub fn predicate_base(concept_base: &str) -> String {
    concept_base
        .strip_suffix("entity/")
        .unwrap_or(concept_base)
        .to_string()
}

/// Percent-encodes the characters an IRI may not hold (RFC 3987: spaces, controls, and
/// `<>"{}|\^` and backtick), leaving everything else, non-ASCII included, as it is.
#[must_use]
pub fn escape_iri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control()
            || matches!(
                c,
                ' ' | '<' | '>' | '"' | '{' | '}' | '|' | '\\' | '^' | '`'
            )
        {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "%{b:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// PHP's `rawurlencode`: every byte but `A–Z a–z 0–9 - _ . ~` percent-encoded.
#[must_use]
pub fn rawurlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// MediaWiki's `wfUrlencode`: `rawurlencode` that leaves `; @ $ ! * ( ) , / ~ :` as they
/// are, as Wikibase writes Commons data-page IRIs.
#[must_use]
pub fn wf_urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_'
                    | b'.'
                    | b'~'
                    | b';'
                    | b'@'
                    | b'$'
                    | b'!'
                    | b'*'
                    | b'('
                    | b')'
                    | b','
                    | b'/'
                    | b':'
            )
        {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheme() -> IriScheme<'static> {
        IriScheme::own(Namespaces::with_wiki_path("https://example.org"))
    }

    #[test]
    fn own_base_names_everything_locally() {
        let s = scheme();
        let iri = |id: &str| s.concept(&EntityId::parse(id).unwrap()).into_string();
        assert_eq!(iri("Q5"), "https://example.org/entity/Q5");
        assert_eq!(iri("WDQ42"), "https://example.org/entity/WDQ42");
        assert_eq!(
            s.document(&EntityId::parse("Q5").unwrap()).as_str(),
            "https://example.org/wiki/Special:EntityData/Q5"
        );
        let local = s.property(&EntityId::parse("P7").unwrap());
        assert_eq!(
            local.iri(Family::QualifierValue).as_str(),
            "https://example.org/prop/qualifier/value/P7"
        );
    }

    /// An `EntityIris` names entities and properties elsewhere (0115 §15).
    #[test]
    fn entities_named_elsewhere() {
        struct Zed;
        impl EntityIris for Zed {
            fn concept(&self, id: &EntityId) -> Option<NamedNode> {
                let rest = id.as_str().strip_prefix("ZZ")?;
                Some(NamedNode::new_unchecked(format!(
                    "https://zed.example/id/{rest}"
                )))
            }
            fn property(&self, id: &EntityId) -> Option<(String, String)> {
                let rest = id.as_str().strip_prefix("ZZ")?;
                Some(("https://zed.example/id/".into(), rest.into()))
            }
        }
        let s = IriScheme::new(Namespaces::with_wiki_path("https://example.org"), Zed);
        assert_eq!(
            s.concept(&EntityId::parse("ZZQ5").unwrap()).as_str(),
            "https://zed.example/id/Q5"
        );
        // A concept base that does not end in `entity/` has `prop/` written after it.
        assert_eq!(
            s.property(&EntityId::parse("ZZP5").unwrap())
                .iri(Family::Direct)
                .as_str(),
            "https://zed.example/id/prop/direct/P5"
        );
        assert_eq!(
            s.concept(&EntityId::parse("Q5").unwrap()).as_str(),
            "https://example.org/entity/Q5"
        );
    }

    #[test]
    fn predicates_follow_wikibase_rule() {
        assert_eq!(
            predicate_base("http://www.wikidata.org/entity/"),
            "http://www.wikidata.org/"
        );
        assert_eq!(
            predicate_base("https://zed.example/id/"),
            "https://zed.example/id/"
        );
    }

    #[test]
    fn encodings() {
        assert_eq!(rawurlencode("Douglas adams.jpg"), "Douglas%20adams.jpg");
        assert_eq!(wf_urlencode("Data:Paris (x).map"), "Data:Paris%20(x).map");
        assert_eq!(escape_iri("a b<c>"), "a%20b%3Cc%3E");
    }
}
