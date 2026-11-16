//! The IRIs Wikibase's RDF names things by (wikibase-compat.md §5.1; 02 §2.3; 05 §1.5).
//!
//! Prefix names are conventional and not the contract; the IRIs are. An [`IriScheme`] holds
//! the instance's bases and the registries that turn a foreign or keyed ID into its concept
//! IRI, and writes every node and predicate IRI the renderer needs.

use std::fmt::Write as _;

use oxrdf::NamedNode;
use scatter_normalize::KeyedRegistry;
use scatter_providers::{Registry as ProviderRegistry, UpstreamRef};
use scatter_wikibase_model::{EntityId, IdForm, StatementId};

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
    /// Bases for a tenant at `base` (`https://example.org/`) with articles at
    /// `{base}wiki/` (0004 §4).
    #[must_use]
    pub fn triplespace(base: &str) -> Self {
        let base = with_slash(base);
        Self {
            article_path: format!("{base}wiki/"),
            base,
        }
    }

    /// Bases given in full, for a stock Wikibase's layout.
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

/// The nodes of a statement named as its upstream names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamNodes {
    /// The statement node.
    pub statement: NamedNode,
    /// The base its reference and value nodes are under: `http://www.wikidata.org/`.
    pub base: String,
    /// The provider's code, whose ID forms the node hashes are taken over.
    pub provider: String,
}

/// The IRIs of one instance (or one tenant), for one rendering.
#[derive(Debug, Clone)]
pub struct IriScheme<'r> {
    ns: Namespaces,
    providers: &'r ProviderRegistry,
    keyed: &'r KeyedRegistry,
}

impl<'r> IriScheme<'r> {
    /// A scheme over `ns`, resolving foreign and keyed IDs through the registries.
    #[must_use]
    pub const fn new(
        ns: Namespaces,
        providers: &'r ProviderRegistry,
        keyed: &'r KeyedRegistry,
    ) -> Self {
        Self {
            ns,
            providers,
            keyed,
        }
    }

    /// A scheme over `ns` with the embedded registries.
    #[must_use]
    pub fn with_default_registries(ns: Namespaces) -> IriScheme<'static> {
        IriScheme::new(
            ns,
            ProviderRegistry::default_registry(),
            KeyedRegistry::default_registry(),
        )
    }

    /// The bases.
    #[must_use]
    pub const fn namespaces(&self) -> &Namespaces {
        &self.ns
    }

    /// An entity's concept IRI. A local ID is `{base}entity/{id}`; a foreign ID is its
    /// provider type's template applied to the upstream ID (0002 §4; 05 §1.5), and a source
    /// entity's likewise (0078 §7; 02 §2.3); a keyed ID is its type's template (04 §3.5).
    /// An ID no registry knows falls back to `{base}entity/{id}`, which is what an
    /// unpublished source's concept IRI is (02 §2.3).
    #[must_use]
    pub fn concept(&self, id: &EntityId) -> NamedNode {
        match id.form() {
            IdForm::Foreign => {
                if let Ok(f) = self.providers.parse_foreign_id(id.as_str()) {
                    return f.concept_iri();
                }
            }
            IdForm::Keyed => {
                if let Ok(k) = self.keyed.parse_id(id.as_str()) {
                    return k.iri();
                }
            }
            IdForm::Local => {}
        }
        NamedNode::new_unchecked(format!("{}{}", self.ns.entity(), escape_iri(id.as_str())))
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
    /// `prop/…`. A local property's concept base is `{base}entity/`, so its predicates are
    /// `{base}prop/direct/P7`; a foreign property keeps its upstream's,
    /// `http://www.wikidata.org/prop/direct/P31` (05 §1.5; 0002 A42). A foreign property
    /// whose template puts anything after the ID, which no Wikibase can, is named under the
    /// instance's base by its prefixed ID.
    #[must_use]
    pub fn property(&self, id: &EntityId) -> PropertyIris {
        if id.form() == IdForm::Foreign
            && let Ok(f) = self.providers.parse_foreign_id(id.as_str())
            && let Some((concept_base, "")) = f.entity_type().iri.split_once("{upstream_id}")
        {
            return PropertyIris {
                base: predicate_base(concept_base),
                local: escape_iri(&f.upstream_id()),
            };
        }
        PropertyIris {
            base: predicate_base(&self.ns.entity()),
            local: escape_iri(id.as_str()),
        }
    }

    /// Where a mirrored statement's nodes are named when it says exactly what its upstream
    /// says (05 §1.5; 0002 A43): its statement node as its upstream names it,
    /// `{concept base}statement/{upstream ID}-{UUID}`, after the entity `upstream` names
    /// (the subject of the statement's ID, or for a key-mapped provider's statement the
    /// item mapped onto the keyed subject, 0002 A44), and its reference and value nodes
    /// under the upstream's base. `None` when `upstream` is not an entity of a
    /// Wikibase-shaped provider in the registry: the statement is then named under the
    /// instance's base.
    #[must_use]
    pub fn upstream_nodes(
        &self,
        upstream: &UpstreamRef,
        id: &StatementId,
    ) -> Option<UpstreamNodes> {
        let (provider, entity_type) = upstream.resolve(self.providers)?;
        let Some((concept_base, "")) = entity_type.iri.split_once("{upstream_id}") else {
            return None;
        };
        Some(UpstreamNodes {
            statement: NamedNode::new_unchecked(format!(
                "{concept_base}statement/{}-{}",
                escape_iri(&upstream.id),
                escape_iri(id.uuid())
            )),
            base: predicate_base(concept_base),
            provider: provider.code.clone(),
        })
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
/// base without a trailing `entity/`, or the concept base itself.
fn predicate_base(concept_base: &str) -> String {
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
        IriScheme::with_default_registries(Namespaces::triplespace("https://example.org"))
    }

    #[test]
    fn local_foreign_and_keyed_concepts() {
        let s = scheme();
        let iri = |id: &str| s.concept(&EntityId::parse(id).unwrap()).into_string();
        assert_eq!(iri("Q5"), "https://example.org/entity/Q5");
        assert_eq!(iri("WDQ42"), "http://www.wikidata.org/entity/Q42");
        assert_eq!(
            iri("domain:en.wikipedia.org"),
            "https://scatter.red/domain/en.wikipedia.org"
        );
        assert_eq!(
            s.document(&EntityId::parse("WDQ42").unwrap()).as_str(),
            "https://example.org/wiki/Special:EntityData/WDQ42"
        );
    }

    #[test]
    fn foreign_properties_keep_upstream_predicates() {
        let s = scheme();
        let wd = s.property(&EntityId::parse("WDP31").unwrap());
        assert_eq!(
            wd.iri(Family::Direct).as_str(),
            "http://www.wikidata.org/prop/direct/P31"
        );
        let local = s.property(&EntityId::parse("P7").unwrap());
        assert_eq!(
            local.iri(Family::QualifierValue).as_str(),
            "https://example.org/prop/qualifier/value/P7"
        );
    }

    #[test]
    fn predicates_follow_wikibase_rule() {
        assert_eq!(
            predicate_base("http://www.wikidata.org/entity/"),
            "http://www.wikidata.org/"
        );
        // A Wikibase whose concept base does not end in `entity/` writes `prop/` after it.
        assert_eq!(
            predicate_base("https://zed.example/id/"),
            "https://zed.example/id/"
        );
        let registry = ProviderRegistry::parse(
            "version = 1\n[[provider]]\ncode = \"ZZ\"\nslug = \"zed\"\nnumber = 99\nissuer = \"zed\"\nrevision_ids = true\nadapter = \"scatter-adapter-wikibase\"\n[[provider.type]]\ncode = \"P\"\nentity_type = \"property\"\nupstream_prefix = \"P\"\niri = \"https://zed.example/id/{upstream_id}\"\nnamespace = \"Property\"\n",
        )
        .unwrap();
        let s = IriScheme::new(
            Namespaces::triplespace("https://example.org"),
            &registry,
            KeyedRegistry::default_registry(),
        );
        let p = s.property(&EntityId::parse("ZZP5").unwrap());
        assert_eq!(
            p.iri(Family::Direct).as_str(),
            "https://zed.example/id/prop/direct/P5"
        );
    }

    #[test]
    fn upstream_statement_nodes() {
        let s = scheme();
        let id = StatementId::parse("WDQ42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9").unwrap();
        let wd = UpstreamRef::of(&s.providers.parse_foreign_id("WDQ42").unwrap());
        let u = s.upstream_nodes(&wd, &id).unwrap();
        assert_eq!(
            u.statement.as_str(),
            "http://www.wikidata.org/entity/statement/Q42-F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
        );
        assert_eq!(u.base, "http://www.wikidata.org/");
        assert_eq!(u.provider, "WD");
        // A key-mapped provider's statement is stored under the keyed subject with the
        // item's UUID; named after the item it was mapped from (0002 A44).
        let keyed = StatementId::parse("domain:wikipedia.org$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9")
            .unwrap();
        let xd = UpstreamRef::mapped(s.providers.by_code("XD").unwrap(), "Q11").unwrap();
        let u = s.upstream_nodes(&xd, &keyed).unwrap();
        assert_eq!(
            u.statement.as_str(),
            "https://internetdomains.wiki/entity/statement/Q11-F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
        );
        assert_eq!(u.base, "https://internetdomains.wiki/");
        assert_eq!(u.provider, "XD");
        // A provider the registry does not know names nothing.
        let unknown = UpstreamRef {
            provider: "ZZ".into(),
            type_code: 'Q',
            id: "Q1".into(),
        };
        assert!(s.upstream_nodes(&unknown, &id).is_none());
    }

    #[test]
    fn encodings() {
        assert_eq!(rawurlencode("Douglas adams.jpg"), "Douglas%20adams.jpg");
        assert_eq!(wf_urlencode("Data:Paris (x).map"), "Data:Paris%20(x).map");
        assert_eq!(escape_iri("a b<c>"), "a%20b%3Cc%3E");
    }
}
