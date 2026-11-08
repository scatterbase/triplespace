//! An entity's triples in RDF format 1.0.0 (wikibase-compat.md §5), under the flags of
//! Wikibase's `RdfProducer`, which make up `Special:EntityData`'s flavors and the dump
//! flavors of `dumpRdf.php`.
//!
//! The order follows Wikibase's dumps: the document node, the entity's type (with a
//! property's predicate family and declarations), sitelinks, the truthy triples, the
//! statements, the terms, and last the reference and value nodes. Order is not part of the
//! contract; the set of triples is.

use std::collections::{BTreeSet, HashSet};

use oxrdf::{BlankNode, Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, Term, Triple};
use scatter_vocab::{cc, geo, owl, prov, rdf, rdfs, schema, skos, wikibase, xsd};
use scatter_wikibase_model::{
    DataType, DataValue, Entity, EntityId, EntityType, Hasher, PageInfo, Rank, Reference,
    SiteRegistry, Sitelink, Snak, SnakKind, Statement, StatementId, php_float_string,
};
use sha2::{Digest as _, Sha256};

use crate::iri::{Family, IriScheme, PropertyIris, rawurlencode, wf_urlencode};
use crate::time::xsd_date_time;

/// Earth, as every Wikibase names it.
pub const EARTH: &str = "http://www.wikidata.org/entity/Q2";
/// The unit of a unitless quantity, as every Wikibase names it (wikibase-compat.md §7.3).
pub const UNIT_ONE: &str = "http://www.wikidata.org/entity/Q199";
const COMMONS_FILE_PATH: &str = "http://commons.wikimedia.org/wiki/Special:FilePath/";
const COMMONS_DATA: &str = "http://commons.wikimedia.org/data/main/";

/// What to write: Wikibase's `RdfProducer` flags, with its values. Terms, the document
/// node's `schema:about`, `schema:version` and `schema:dateModified`, and a property's own
/// predicate family and declarations are written under every set, as in Wikibase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Produce(u16);

impl Produce {
    /// Best-rank statements as `wdt:` triples.
    pub const TRUTHY_STATEMENTS: Self = Self(1);
    /// Every statement as a node.
    pub const ALL_STATEMENTS: Self = Self(2);
    /// Qualifiers on statement nodes.
    pub const QUALIFIERS: Self = Self(4);
    /// References on statement nodes.
    pub const REFERENCES: Self = Self(8);
    /// Sitelinks and their badges.
    pub const SITELINKS: Self = Self(16);
    /// Stubs for the properties the written snaks use ([`Renderer::mentions`]).
    pub const PROPERTIES: Self = Self(32);
    /// Full value nodes (`psv:`, `pqv:`, `prv:` and `v:`).
    pub const FULL_VALUES: Self = Self(64);
    /// `cc:license` and `schema:softwareVersion` on the document node.
    pub const VERSION_INFO: Self = Self(128);
    /// Stubs for the entities the written snaks' values name ([`Renderer::mentions`]).
    pub const RESOLVED_ENTITIES: Self = Self(256);
    /// Normalized values; none are written yet, since no instance configures unit
    /// conversion or formatter URLs.
    pub const NORMALIZED_VALUES: Self = Self(512);
    /// The document node's statement, sitelink and identifier counts.
    pub const PAGE_PROPS: Self = Self(1024);
    /// Everything.
    pub const ALL: Self = Self(0xFFFF);

    /// What `dumpRdf.php --flavor full-dump` writes for each entity: the `dump` flavor
    /// without the version information, which a dump carries once in its header.
    pub const DUMP_FILE: Self = Self(2 | 1 | 4 | 8 | 16 | 64 | 1024 | 512);
    /// What `dumpRdf.php --flavor truthy-dump` writes for each entity.
    pub const TRUTHY_DUMP_FILE: Self = Self::TRUTHY_STATEMENTS;

    /// Whether every flag of `other` is set.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets of flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for Produce {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// `Special:EntityData`'s RDF flavors (`EntityDataSerializationService::getFlavor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// Truthy statements and sitelinks.
    Simple,
    /// Every statement with its qualifiers and references, and sitelinks; no truthy
    /// triples and no value nodes.
    Long,
    /// What a dump writes for the entity, with the version information.
    Dump,
    /// Everything, with stubs for the properties and entities the statements mention:
    /// what is served when no flavor is asked for.
    Full,
}

impl Flavor {
    /// The names `flavor=` takes, as Wikibase lists them in its error.
    pub const NAMES: [&str; 4] = ["simple", "dump", "long", "full"];

    /// The flavor `flavor=` names, `Full` when it is absent or empty, `None` when it names
    /// none of the four (Wikibase answers that with a 400).
    #[must_use]
    pub fn parse(name: Option<&str>) -> Option<Self> {
        match name.unwrap_or_default() {
            "simple" => Some(Self::Simple),
            "long" => Some(Self::Long),
            "dump" => Some(Self::Dump),
            "full" | "" => Some(Self::Full),
            _ => None,
        }
    }

    /// The flags the flavor sets.
    #[must_use]
    pub const fn produce(self) -> Produce {
        match self {
            Self::Simple => Produce(1 | 16 | 128),
            Self::Long => Produce(2 | 4 | 8 | 16 | 128),
            Self::Dump => Produce(2 | 1 | 4 | 8 | 16 | 64 | 1024 | 512 | 128),
            Self::Full => Produce::ALL,
        }
    }
}

/// What a sitelink's site contributes to the RDF (wikibase-compat.md §5.2; 02 §5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteFacts {
    /// The page's URL, the `schema:Article` node.
    pub url: String,
    /// The site's language, where an alias supplies one: `schema:inLanguage`, and the tag
    /// on `schema:name`.
    pub language: Option<String>,
    /// The site's root, `schema:isPartOf`.
    pub part_of: Option<String>,
    /// The site's group, `wikibase:wikiGroup` on the root.
    pub group: Option<String>,
}

/// Where a sitelink's facts come from.
pub trait Sites {
    /// The facts for a sitelink, or `None` to leave it out.
    fn facts(&self, link: &Sitelink) -> Option<SiteFacts>;
}

impl Sites for SiteRegistry {
    /// A sitelink's site is its host, or a site alias for one (0026 §2): the URL is the
    /// link's own or the alias's, the root is the URL's scheme and host, and language and
    /// group come from the alias.
    fn facts(&self, link: &Sitelink) -> Option<SiteFacts> {
        let site = self.by_id(&link.site).or_else(|| self.by_host(&link.site));
        let url = link.url.clone().or_else(|| self.sitelink_url(link))?;
        let part_of = url.split_once("://").map(|(scheme, rest)| {
            let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
            format!("{scheme}://{host}/")
        });
        Some(SiteFacts {
            url,
            language: site.and_then(|s| s.language.clone()),
            part_of,
            group: site.and_then(|s| s.group.clone()),
        })
    }
}

impl<F: Fn(&Sitelink) -> Option<SiteFacts>> Sites for F {
    fn facts(&self, link: &Sitelink) -> Option<SiteFacts> {
        self(link)
    }
}

/// The nodes a dump has already written, so that a shared reference, value or site node
/// is written once (wikibase-compat.md §5.4).
#[derive(Debug, Default, Clone)]
pub struct Seen(HashSet<String>);

impl Seen {
    /// Marks `key` written; true when it was not already.
    fn first(&mut self, key: &str) -> bool {
        self.0.insert(key.to_string())
    }
}

type DatatypeLookup<'a> = &'a (dyn Fn(&EntityId) -> Option<DataType> + Sync);

fn no_lookup(_: &EntityId) -> Option<DataType> {
    None
}

/// Renders entities as RDF.
pub struct Renderer<'a> {
    iris: &'a IriScheme<'a>,
    hasher: &'a Hasher,
    produce: Produce,
    sites: &'a (dyn Sites + Sync),
    datatypes: DatatypeLookup<'a>,
    upstream: Option<&'a BTreeSet<StatementId>>,
    skolem: bool,
}

/// Where one statement's reference and value nodes are named, and how they are hashed.
struct NodeNames {
    base: String,
    hasher: Hasher,
}

impl NodeNames {
    fn reference(&self, hash: &str) -> NamedNode {
        NamedNode::new_unchecked(format!("{}reference/{hash}", self.base))
    }

    fn value(&self, value: &DataValue) -> NamedNode {
        NamedNode::new_unchecked(format!("{}value/{}", self.base, self.hasher.value(value)))
    }
}

impl<'a> Renderer<'a> {
    /// A renderer over `iris` writing what `produce` says, naming value and reference
    /// nodes with `hasher`, with the embedded site aliases and no property-type lookup.
    #[must_use]
    pub fn new(iris: &'a IriScheme<'a>, hasher: &'a Hasher, produce: Produce) -> Self {
        Self {
            iris,
            hasher,
            produce,
            sites: SiteRegistry::default_registry(),
            datatypes: &no_lookup,
            upstream: None,
            skolem: false,
        }
    }

    /// Writes skolem IRIs, `https://scatter.red/genid/{hash}`, where Wikibase writes blank
    /// nodes: the form of a dump the update stream continues from (02 §4.3).
    #[must_use]
    pub const fn with_skolem(mut self, skolem: bool) -> Self {
        self.skolem = skolem;
        self
    }

    /// Names the statements in `upstream` as their upstream names them: the statement node
    /// under the provider's concept base, and its reference and value nodes under the
    /// provider's base, hashed over the IDs as the provider writes them, so that each node
    /// is the one the provider's own RDF has (05 §1.5; 0002 A43). Which statements qualify
    /// is the caller's: those whose every member is a mirror's and that no local correction
    /// touches. A statement whose ID is not a Wikibase-shaped provider's is named under the
    /// instance's base all the same.
    #[must_use]
    pub fn with_upstream(mut self, upstream: &'a BTreeSet<StatementId>) -> Self {
        self.upstream = Some(upstream);
        self
    }

    /// Takes sitelink facts from `sites`.
    #[must_use]
    pub fn with_sites(mut self, sites: &'a (dyn Sites + Sync)) -> Self {
        self.sites = sites;
        self
    }

    /// Types a snak that carries no `datatype` by its property's, from `lookup`.
    #[must_use]
    pub fn with_datatypes(mut self, lookup: DatatypeLookup<'a>) -> Self {
        self.datatypes = lookup;
        self
    }

    /// An entity's triples, standing alone, as `Special:EntityData` serves them.
    #[must_use]
    pub fn entity(&self, entity: &Entity, page: Option<&PageInfo>) -> Vec<Triple> {
        let mut out = Vec::new();
        self.entity_into(entity, page, &mut Seen::default(), &mut out);
        out
    }

    /// An entity's triples, appended to `out`, leaving out the shared nodes `seen` says
    /// were written already: the form a dump writes.
    pub fn entity_into(
        &self,
        entity: &Entity,
        page: Option<&PageInfo>,
        seen: &mut Seen,
        out: &mut Vec<Triple>,
    ) {
        let subject = self.iris.concept(&entity.id);
        let mut tail = Vec::new();

        self.document(entity, page, &subject, out);
        match entity.entity_type {
            EntityType::Item => push(out, &subject, rdf::TYPE, wikibase::ITEM),
            EntityType::Property => self.property(entity, &subject, out),
            // Lexemes (0066 §6), MediaInfo (0065), and the keyed types (0048 §5) add their
            // own node types; not written yet.
            _ => {}
        }
        if self.produce.has(Produce::SITELINKS) {
            self.sitelinks(entity, &subject, seen, out);
        }

        if self.produce.has(Produce::TRUTHY_STATEMENTS) {
            for (property, group) in &entity.statements {
                let best = best_rank(group);
                let piris = self.iris.property(property);
                for statement in group {
                    if best.is_some_and(|b| statement.rank == b) {
                        self.truthy(statement, &subject, &piris, out);
                    }
                }
            }
        }
        if self.produce.has(Produce::ALL_STATEMENTS) {
            for (property, group) in &entity.statements {
                let best = best_rank(group);
                let piris = self.iris.property(property);
                for statement in group {
                    let is_best = best.is_some_and(|b| statement.rank == b);
                    self.statement(statement, is_best, &subject, &piris, seen, out, &mut tail);
                }
            }
        }
        Self::terms(entity, &subject, out);
        out.append(&mut tail);
    }

    /// The properties and entities an entity's written snaks mention, in order of first
    /// mention, without the entity itself: what Wikibase writes stubs for after the entity
    /// under [`Produce::PROPERTIES`] (every snak's property) and
    /// [`Produce::RESOLVED_ENTITIES`] (every entity-valued value). The snaks are those
    /// the flags write: best-rank main snaks for the truthy triples, every main snak, and
    /// qualifiers and reference snaks where those are written. Units, globes, calendar
    /// models and badges are not mentions, as in Wikibase.
    #[must_use]
    pub fn mentions(&self, entity: &Entity) -> Vec<EntityId> {
        let properties = self.produce.has(Produce::PROPERTIES);
        let values = self.produce.has(Produce::RESOLVED_ENTITIES);
        let mut out: Vec<EntityId> = Vec::new();
        if !properties && !values {
            return out;
        }
        let mut seen: HashSet<EntityId> = HashSet::from([entity.id.clone()]);
        let mut mention = |snak: &Snak| {
            if properties && seen.insert(snak.property.clone()) {
                out.push(snak.property.clone());
            }
            if values
                && let SnakKind::Value(DataValue::EntityId(e)) = &snak.kind
                && seen.insert(e.id.clone())
            {
                out.push(e.id.clone());
            }
        };
        for group in entity.statements.values() {
            let best = best_rank(group);
            for statement in group {
                let is_best = best.is_some_and(|b| statement.rank == b);
                if (is_best && self.produce.has(Produce::TRUTHY_STATEMENTS))
                    || self.produce.has(Produce::ALL_STATEMENTS)
                {
                    mention(&statement.mainsnak);
                }
                if !self.produce.has(Produce::ALL_STATEMENTS) {
                    continue;
                }
                if self.produce.has(Produce::QUALIFIERS) {
                    statement
                        .qualifiers
                        .values()
                        .flatten()
                        .for_each(&mut mention);
                }
                if self.produce.has(Produce::REFERENCES) {
                    statement
                        .references
                        .iter()
                        .flat_map(|r| r.snaks.values().flatten())
                        .for_each(&mut mention);
                }
            }
        }
        out
    }

    /// A mentioned entity's stub, as Wikibase's `ItemStubRdfBuilder` and
    /// `PropertyStubRdfBuilder` write it: the entity's type, its labels and descriptions in
    /// `languages` (no aliases, statements or sitelinks), and for a property its type,
    /// predicate family and declarations.
    #[must_use]
    pub fn stub(&self, entity: &Entity, languages: &[String]) -> Vec<Triple> {
        let subject = self.iris.concept(&entity.id);
        let mut out = Vec::new();
        match entity.entity_type {
            EntityType::Item => push(&mut out, &subject, rdf::TYPE, wikibase::ITEM),
            EntityType::Property => self.property(entity, &subject, &mut out),
            _ => {}
        }
        for lang in languages {
            if let Some(label) = entity.labels.get(lang) {
                for p in [rdfs::LABEL, skos::PREF_LABEL, schema::NAME] {
                    push_lit(&mut out, &subject, p, text(label, Some(lang)));
                }
            }
            if let Some(description) = entity.descriptions.get(lang) {
                push_lit(
                    &mut out,
                    &subject,
                    schema::DESCRIPTION,
                    text(description, Some(lang)),
                );
            }
        }
        out
    }

    /// A mentioned ID that redirects: `owl:sameAs` from it to its target, which is then
    /// stubbed in its place (Wikibase's `addEntityRedirect`).
    #[must_use]
    pub fn redirect(&self, from: &EntityId, to: &EntityId) -> Triple {
        Triple::new(self.iris.concept(from), owl::SAME_AS, self.iris.concept(to))
    }

    // --- the document and the entity -------------------------------------------------

    fn document(
        &self,
        entity: &Entity,
        page: Option<&PageInfo>,
        subject: &NamedNode,
        out: &mut Vec<Triple>,
    ) {
        let doc = self.iris.document(&entity.id);
        push(out, &doc, rdf::TYPE, schema::DATASET);
        push(out, &doc, schema::ABOUT, subject.as_ref());
        if self.produce.has(Produce::VERSION_INFO) {
            push(
                out,
                &doc,
                cc::LICENSE,
                NamedNode::new_unchecked(crate::write::CC0).as_ref(),
            );
            push_lit(
                out,
                &doc,
                schema::SOFTWARE_VERSION,
                Literal::new_simple_literal(crate::write::FORMAT_VERSION),
            );
        }
        if let Some(rev) = page.and_then(|p| p.lastrevid) {
            push_lit(
                out,
                &doc,
                schema::VERSION,
                typed(&rev.to_string(), xsd::INTEGER),
            );
        }
        if let Some(modified) = page.and_then(|p| p.modified.as_deref()) {
            push_lit(
                out,
                &doc,
                schema::DATE_MODIFIED,
                typed(modified, xsd::DATE_TIME),
            );
        }
        if self.produce.has(Produce::PAGE_PROPS) {
            let identifiers = entity
                .all_statements()
                .filter(|s| self.datatype(&s.mainsnak) == Some(DataType::ExternalId))
                .count();
            for (p, n) in [
                (wikibase::STATEMENTS, entity.statement_count()),
                (wikibase::SITELINKS, entity.sitelinks.len()),
                (wikibase::IDENTIFIERS, identifiers),
            ] {
                push_lit(out, &doc, p, typed(&n.to_string(), xsd::INTEGER));
            }
        }
    }

    fn property(&self, entity: &Entity, subject: &NamedNode, out: &mut Vec<Triple>) {
        push(out, subject, rdf::TYPE, wikibase::PROPERTY);
        let datatype = entity.datatype.clone().unwrap_or(DataType::String);
        let type_iri =
            NamedNode::new_unchecked(format!("{}{}", wikibase::NS, property_type_name(&datatype)));
        push(out, subject, wikibase::PROPERTY_TYPE, type_iri.as_ref());
        let piris = self.iris.property(&entity.id);
        let normalized = normalization(&datatype);
        let mut links = vec![
            (wikibase::DIRECT_CLAIM, Family::Direct),
            (wikibase::CLAIM, Family::Claim),
            (wikibase::STATEMENT_PROPERTY, Family::Statement),
            (wikibase::STATEMENT_VALUE, Family::StatementValue),
            (wikibase::QUALIFIER, Family::Qualifier),
            (wikibase::QUALIFIER_VALUE, Family::QualifierValue),
            (wikibase::REFERENCE_PROPERTY, Family::Reference),
            (wikibase::REFERENCE_VALUE, Family::ReferenceValue),
            (wikibase::NOVALUE, Family::NoValue),
        ];
        if normalized.is_some() {
            links.extend([
                (wikibase::DIRECT_CLAIM_NORMALIZED, Family::DirectNormalized),
                (
                    wikibase::STATEMENT_VALUE_NORMALIZED,
                    Family::StatementValueNormalized,
                ),
                (
                    wikibase::QUALIFIER_VALUE_NORMALIZED,
                    Family::QualifierValueNormalized,
                ),
                (
                    wikibase::REFERENCE_VALUE_NORMALIZED,
                    Family::ReferenceValueNormalized,
                ),
            ]);
        }
        for (link, family) in &links {
            push(out, subject, *link, piris.iri(*family).as_ref());
        }

        // Declarations: the statement and value-node links are object properties, and the
        // simple-value predicates are object properties exactly when values are IRIs.
        let simple = if is_iri_valued(&datatype) {
            owl::OBJECT_PROPERTY
        } else {
            owl::DATATYPE_PROPERTY
        };
        for family in [
            Family::Claim,
            Family::StatementValue,
            Family::QualifierValue,
            Family::ReferenceValue,
        ] {
            push(out, &piris.iri(family), rdf::TYPE, owl::OBJECT_PROPERTY);
        }
        for family in [
            Family::Direct,
            Family::Statement,
            Family::Qualifier,
            Family::Reference,
        ] {
            push(out, &piris.iri(family), rdf::TYPE, simple);
        }
        if let Some(direct_normalized) = normalized {
            for family in [
                Family::StatementValueNormalized,
                Family::QualifierValueNormalized,
                Family::ReferenceValueNormalized,
            ] {
                push(out, &piris.iri(family), rdf::TYPE, owl::OBJECT_PROPERTY);
            }
            push(
                out,
                &piris.iri(Family::DirectNormalized),
                rdf::TYPE,
                direct_normalized,
            );
        }

        // wdno:P a owl:Class ; owl:complementOf [ a owl:Restriction ; owl:onProperty wdt:P ;
        //   owl:someValuesFrom owl:Thing ] .
        let novalue = piris.iri(Family::NoValue);
        let direct = piris.iri(Family::Direct);
        let restriction = self.genid(&format!(
            "novalue|{}",
            self.iris.concept(&entity.id).as_str()
        ));
        push(out, &novalue, rdf::TYPE, owl::CLASS);
        out.push(Triple::new(
            novalue.clone(),
            owl::COMPLEMENT_OF,
            restriction.clone(),
        ));
        out.push(Triple::new(
            restriction.clone(),
            rdf::TYPE,
            owl::RESTRICTION,
        ));
        out.push(Triple::new(restriction.clone(), owl::ON_PROPERTY, direct));
        out.push(Triple::new(restriction, owl::SOME_VALUES_FROM, owl::THING));
    }

    fn sitelinks(
        &self,
        entity: &Entity,
        subject: &NamedNode,
        seen: &mut Seen,
        out: &mut Vec<Triple>,
    ) {
        for link in entity.sitelinks.values() {
            let Some(facts) = self.sites.facts(link) else {
                continue;
            };
            let article = NamedNode::new_unchecked(crate::iri::escape_iri(&facts.url));
            push(out, &article, rdf::TYPE, schema::ARTICLE);
            push(out, &article, schema::ABOUT, subject.as_ref());
            if let Some(lang) = &facts.language {
                push_lit(
                    out,
                    &article,
                    schema::IN_LANGUAGE,
                    Literal::new_simple_literal(lang),
                );
            }
            if let Some(root) = &facts.part_of {
                push(
                    out,
                    &article,
                    schema::IS_PART_OF,
                    NamedNode::new_unchecked(root).as_ref(),
                );
            }
            push_lit(
                out,
                &article,
                schema::NAME,
                text(&link.title, facts.language.as_deref()),
            );
            for badge in &link.badges {
                push(
                    out,
                    &article,
                    wikibase::BADGE,
                    self.iris.concept(badge).as_ref(),
                );
            }
            if let (Some(root), Some(group)) = (&facts.part_of, &facts.group)
                && seen.first(&format!("site {root}"))
            {
                push_lit(
                    out,
                    &NamedNode::new_unchecked(root),
                    wikibase::WIKI_GROUP,
                    Literal::new_simple_literal(group),
                );
            }
        }
    }

    fn terms(entity: &Entity, subject: &NamedNode, out: &mut Vec<Triple>) {
        for (lang, label) in &entity.labels {
            for p in [rdfs::LABEL, skos::PREF_LABEL, schema::NAME] {
                push_lit(out, subject, p, text(label, Some(lang)));
            }
        }
        for (lang, description) in &entity.descriptions {
            push_lit(
                out,
                subject,
                schema::DESCRIPTION,
                text(description, Some(lang)),
            );
        }
        for (lang, aliases) in &entity.aliases {
            for alias in aliases {
                push_lit(out, subject, skos::ALT_LABEL, text(alias, Some(lang)));
            }
        }
    }

    // --- statements --------------------------------------------------------------------

    /// The truthy triples of a best-rank statement: its main snak on the entity.
    fn truthy(
        &self,
        statement: &Statement,
        subject: &NamedNode,
        piris: &PropertyIris,
        out: &mut Vec<Triple>,
    ) {
        let snak = &statement.mainsnak;
        match &snak.kind {
            SnakKind::Value(value) => {
                for object in self.simple_values(snak, value) {
                    out.push(Triple::new(
                        subject.clone(),
                        piris.iri(Family::Direct),
                        object,
                    ));
                }
            }
            SnakKind::SomeValue => {
                let node = self.some_value_node(statement, &Role::Main, &snak.property, 0);
                out.push(Triple::new(
                    subject.clone(),
                    piris.iri(Family::Direct),
                    node,
                ));
            }
            SnakKind::NoValue => push(out, subject, rdf::TYPE, piris.iri(Family::NoValue).as_ref()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn statement(
        &self,
        statement: &Statement,
        is_best: bool,
        subject: &NamedNode,
        piris: &PropertyIris,
        seen: &mut Seen,
        out: &mut Vec<Triple>,
        tail: &mut Vec<Triple>,
    ) {
        let Some(id) = &statement.id else {
            // A statement without an ID has no node to hang on; Wikibase never writes one.
            return;
        };
        let upstream = self
            .upstream
            .filter(|u| u.contains(id))
            .and_then(|_| self.iris.upstream_nodes(id));
        let (node, names) = match upstream {
            Some(u) => (
                u.statement,
                NodeNames {
                    base: u.base,
                    hasher: Hasher::mirrored_from(&u.provider),
                },
            ),
            None => (
                self.iris.statement(id),
                NodeNames {
                    base: self.iris.namespaces().base.clone(),
                    hasher: self.hasher.clone(),
                },
            ),
        };
        let names = &names;
        push(
            out,
            subject,
            piris.iri(Family::Claim).as_ref(),
            node.as_ref(),
        );
        push(out, &node, rdf::TYPE, wikibase::STATEMENT);
        if is_best {
            push(out, &node, rdf::TYPE, wikibase::BEST_RANK);
        }
        let rank = match statement.rank {
            Rank::Preferred => wikibase::PREFERRED_RANK,
            Rank::Normal => wikibase::NORMAL_RANK,
            Rank::Deprecated => wikibase::DEPRECATED_RANK,
        };
        push(out, &node, wikibase::RANK, rank);

        let subject_node: NamedOrBlankNode = node.clone().into();
        self.snak(
            &statement.mainsnak,
            &subject_node,
            (Family::Statement, Family::StatementValue),
            self.some_value_node(statement, &Role::Main, &statement.mainsnak.property, 0),
            names,
            seen,
            out,
            tail,
        );
        for (property, snaks) in statement
            .qualifiers
            .iter()
            .filter(|_| self.produce.has(Produce::QUALIFIERS))
        {
            let qiris = self.iris.property(property);
            for (i, snak) in snaks.iter().enumerate() {
                self.snak_with(
                    snak,
                    &qiris,
                    &subject_node,
                    (Family::Qualifier, Family::QualifierValue),
                    self.some_value_node(statement, &Role::Qualifier, property, i),
                    names,
                    seen,
                    out,
                    tail,
                );
            }
        }
        for reference in statement
            .references
            .iter()
            .filter(|_| self.produce.has(Produce::REFERENCES))
        {
            let hash = reference
                .hash
                .clone()
                .unwrap_or_else(|| names.hasher.reference(reference));
            let ref_node = names.reference(&hash);
            push(out, &node, prov::WAS_DERIVED_FROM, ref_node.as_ref());
            if seen.first(ref_node.as_str()) {
                self.reference(reference, &hash, &ref_node, statement, names, seen, tail);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reference(
        &self,
        reference: &Reference,
        hash: &str,
        node: &NamedNode,
        statement: &Statement,
        names: &NodeNames,
        seen: &mut Seen,
        tail: &mut Vec<Triple>,
    ) {
        let mut body = Vec::new();
        let mut values = Vec::new();
        push(&mut body, node, rdf::TYPE, wikibase::REFERENCE);
        let subject: NamedOrBlankNode = node.clone().into();
        for (property, snaks) in &reference.snaks {
            let riris = self.iris.property(property);
            for (i, snak) in snaks.iter().enumerate() {
                let blank = self.some_value_node(statement, &Role::Reference(hash), property, i);
                self.snak_with(
                    snak,
                    &riris,
                    &subject,
                    (Family::Reference, Family::ReferenceValue),
                    blank,
                    names,
                    seen,
                    &mut body,
                    &mut values,
                );
            }
        }
        tail.append(&mut body);
        tail.append(&mut values);
    }

    #[allow(clippy::too_many_arguments)]
    fn snak(
        &self,
        snak: &Snak,
        subject: &NamedOrBlankNode,
        families: (Family, Family),
        blank: NamedOrBlankNode,
        names: &NodeNames,
        seen: &mut Seen,
        out: &mut Vec<Triple>,
        tail: &mut Vec<Triple>,
    ) {
        let piris = self.iris.property(&snak.property);
        self.snak_with(
            snak, &piris, subject, families, blank, names, seen, out, tail,
        );
    }

    /// One snak on a statement or reference node: the simple value on the first family,
    /// the full value node on the second, `a wdno:P` for no value.
    #[allow(clippy::too_many_arguments)]
    fn snak_with(
        &self,
        snak: &Snak,
        piris: &PropertyIris,
        subject: &NamedOrBlankNode,
        (simple, full): (Family, Family),
        blank: NamedOrBlankNode,
        names: &NodeNames,
        seen: &mut Seen,
        out: &mut Vec<Triple>,
        tail: &mut Vec<Triple>,
    ) {
        match &snak.kind {
            SnakKind::Value(value) => {
                for object in self.simple_values(snak, value) {
                    out.push(Triple::new(subject.clone(), piris.iri(simple), object));
                }
                if self.produce.has(Produce::FULL_VALUES)
                    && let Some(hash_node) = self.value_node(value, names, seen, tail)
                {
                    out.push(Triple::new(subject.clone(), piris.iri(full), hash_node));
                }
            }
            SnakKind::SomeValue => {
                out.push(Triple::new(subject.clone(), piris.iri(simple), blank));
            }
            SnakKind::NoValue => {
                out.push(Triple::new(
                    subject.clone(),
                    rdf::TYPE,
                    piris.iri(Family::NoValue),
                ));
            }
        }
    }

    // --- blank nodes -------------------------------------------------------------------

    /// The node Wikibase writes as a blank node, named by its preimage: a blank node
    /// labelled with the preimage's hash, or under [`Renderer::with_skolem`] the skolem IRI
    /// `https://scatter.red/genid/{hash}` (02 §4.3; wikibase-compat.md §5.7). The label
    /// and the IRI carry the same hash, so the two forms of a dump map one to one.
    fn genid(&self, preimage: &str) -> NamedOrBlankNode {
        let hash = genid_hash(preimage);
        if self.skolem {
            NamedNode::new_unchecked(format!("{GENID}{hash}")).into()
        } else {
            BlankNode::new_unchecked(hash).into()
        }
    }

    /// A somevalue snak's node. A main snak or qualifier is named by its statement's UUID,
    /// its role, its property's concept IRI and its index among that property's snaks in
    /// the role; a reference snak by its reference's hash in place of the UUID, so that a
    /// reference shared by many statements has one node for each of its snaks, as it has
    /// one `ref:` node (wikibase-compat.md §5.7).
    fn some_value_node(
        &self,
        statement: &Statement,
        role: &Role<'_>,
        property: &EntityId,
        index: usize,
    ) -> NamedOrBlankNode {
        let uuid = statement
            .id
            .as_ref()
            .map_or(String::new(), |id| id.uuid().to_ascii_uppercase());
        let property = self.iris.concept(property);
        let preimage = match role {
            Role::Main => format!("main|{uuid}|{}|{index}", property.as_str()),
            Role::Qualifier => format!("qualifier|{uuid}|{}|{index}", property.as_str()),
            Role::Reference(hash) => format!("reference|{hash}|{}|{index}", property.as_str()),
        };
        self.genid(&preimage)
    }

    // --- values ------------------------------------------------------------------------

    fn datatype(&self, snak: &Snak) -> Option<DataType> {
        snak.datatype
            .clone()
            .or_else(|| (self.datatypes)(&snak.property))
    }

    /// The simple value(s) of a value snak: what `wdt:`, `ps:`, `pq:` and `pr:` point at.
    fn simple_values(&self, snak: &Snak, value: &DataValue) -> Vec<Term> {
        let datatype = self.datatype(snak);
        let one = |t: Term| vec![t];
        match value {
            DataValue::String(s) => match datatype {
                Some(DataType::Url) => {
                    one(NamedNode::new_unchecked(crate::iri::escape_iri(s)).into())
                }
                Some(DataType::CommonsMedia) => one(NamedNode::new_unchecked(format!(
                    "{COMMONS_FILE_PATH}{}",
                    rawurlencode(s)
                ))
                .into()),
                Some(DataType::LocalMedia) => one(NamedNode::new_unchecked(format!(
                    "{}{}",
                    self.iris.namespaces().file_path(),
                    rawurlencode(&s.replace(' ', "_"))
                ))
                .into()),
                Some(DataType::GeoShape | DataType::TabularData) => one(NamedNode::new_unchecked(
                    format!("{COMMONS_DATA}{}", wf_urlencode(&s.replace(' ', "_"))),
                )
                .into()),
                // The raw literal only; the xsd:dateTime values WikibaseEDTF derives are
                // left out until their defects are decided (0001 A19).
                Some(DataType::Edtf) => one(typed(s, xsd::EDTF).into()),
                _ => one(Literal::new_simple_literal(s).into()),
            },
            DataValue::MonolingualText(m) => one(text(&m.text, Some(&m.language)).into()),
            DataValue::EntityId(e) => one(self.iris.concept(&e.id).into()),
            DataValue::Time(t) => xsd_date_time(&t.time, t.precision, &t.calendarmodel)
                .map(|v| one(typed(&v, xsd::DATE_TIME).into()))
                .unwrap_or_default(),
            DataValue::Quantity(q) => one(typed(&q.amount, xsd::DECIMAL).into()),
            DataValue::GlobeCoordinate(g) => {
                let point = format!(
                    "Point({} {})",
                    php_float_string(g.longitude),
                    php_float_string(g.latitude)
                );
                let wkt = if g.globe == EARTH {
                    point
                } else {
                    format!("<{}> {point}", self.iris.value_iri(&g.globe).as_str())
                };
                one(typed(&wkt, geo::WKT_LITERAL).into())
            }
            _ => Vec::new(),
        }
    }

    /// The full value node of a time, quantity or coordinate, written to `tail` the first
    /// time `seen` meets it; `None` for value types that have none.
    fn value_node(
        &self,
        value: &DataValue,
        names: &NodeNames,
        seen: &mut Seen,
        tail: &mut Vec<Triple>,
    ) -> Option<NamedNode> {
        if !matches!(
            value,
            DataValue::Time(_) | DataValue::Quantity(_) | DataValue::GlobeCoordinate(_)
        ) {
            return None;
        }
        let node = names.value(value);
        if !seen.first(node.as_str()) {
            return Some(node);
        }
        let mut node_triples = Vec::new();
        let n = &node;
        match value {
            DataValue::Time(t) => {
                node_triples.push(Triple::new(n.clone(), rdf::TYPE, wikibase::TIME_VALUE));
                if let Some(v) = xsd_date_time(&t.time, t.precision, &t.calendarmodel) {
                    node_triples.push(lit(n, wikibase::TIME_VALUE_PROPERTY, &v, xsd::DATE_TIME));
                }
                node_triples.extend([
                    lit(
                        n,
                        wikibase::TIME_PRECISION,
                        &t.precision.to_string(),
                        xsd::INTEGER,
                    ),
                    lit(
                        n,
                        wikibase::TIME_TIMEZONE,
                        &t.timezone.to_string(),
                        xsd::INTEGER,
                    ),
                    Triple::new(
                        n.clone(),
                        wikibase::TIME_CALENDAR_MODEL,
                        self.iris.value_iri(&t.calendarmodel),
                    ),
                ]);
            }
            DataValue::Quantity(q) => {
                node_triples.push(Triple::new(n.clone(), rdf::TYPE, wikibase::QUANTITY_VALUE));
                node_triples.push(lit(n, wikibase::QUANTITY_AMOUNT, &q.amount, xsd::DECIMAL));
                if let Some(u) = &q.upper_bound {
                    node_triples.push(lit(n, wikibase::QUANTITY_UPPER_BOUND, u, xsd::DECIMAL));
                }
                if let Some(l) = &q.lower_bound {
                    node_triples.push(lit(n, wikibase::QUANTITY_LOWER_BOUND, l, xsd::DECIMAL));
                }
                let unit = if q.unit == "1" {
                    NamedNode::new_unchecked(UNIT_ONE)
                } else {
                    self.iris.value_iri(&q.unit)
                };
                node_triples.push(Triple::new(n.clone(), wikibase::QUANTITY_UNIT, unit));
            }
            DataValue::GlobeCoordinate(g) => {
                node_triples.extend([
                    Triple::new(n.clone(), rdf::TYPE, wikibase::GLOBECOORDINATE_VALUE),
                    lit(
                        n,
                        wikibase::GEO_LATITUDE,
                        &php_float_string(g.latitude),
                        xsd::DOUBLE,
                    ),
                    lit(
                        n,
                        wikibase::GEO_LONGITUDE,
                        &php_float_string(g.longitude),
                        xsd::DOUBLE,
                    ),
                ]);
                if let Some(p) = g.precision {
                    node_triples.push(lit(
                        n,
                        wikibase::GEO_PRECISION,
                        &php_float_string(p),
                        xsd::DOUBLE,
                    ));
                }
                node_triples.push(Triple::new(
                    n.clone(),
                    wikibase::GEO_GLOBE,
                    self.iris.value_iri(&g.globe),
                ));
            }
            _ => {}
        }
        tail.append(&mut node_triples);
        Some(node)
    }
}

// --- helpers ---------------------------------------------------------------------------

/// The best rank present in a statement group: preferred if any statement has it, else
/// normal if any has it, else none (deprecated statements are never best).
fn best_rank(group: &[Statement]) -> Option<Rank> {
    if group.iter().any(|s| s.rank == Rank::Preferred) {
        Some(Rank::Preferred)
    } else if group.iter().any(|s| s.rank == Rank::Normal) {
        Some(Rank::Normal)
    } else {
        None
    }
}

/// Where a somevalue snak sits on its statement, for naming its blank node.
enum Role<'h> {
    Main,
    Qualifier,
    Reference(&'h str),
}

/// The lowercase hex SHA-256 of a skolem preimage (wikibase-compat.md §5.7).
#[must_use]
pub fn genid_hash(preimage: &str) -> String {
    Sha256::digest(preimage.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut out, b| {
            use std::fmt::Write as _;
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// The base of skolem IRIs, `https://scatter.red/genid/` (02 §4.3).
pub const GENID: &str = "https://scatter.red/genid/";

/// The local name of a data type's `wikibase:propertyType` (wikibase-compat.md §4.2, §4.5):
/// Wikibase's own names, and for any other type the camel-cased ID, as
/// `RdfVocabulary::getDataTypeURI` falls back to (`edtf` → `Edtf`).
#[must_use]
pub fn property_type_name(datatype: &DataType) -> String {
    match datatype {
        DataType::GlobeCoordinate => "GlobeCoordinate".into(),
        DataType::MonolingualText => "Monolingualtext".into(),
        DataType::CommonsMedia => "CommonsMedia".into(),
        DataType::LocalMedia => "LocalMedia".into(),
        DataType::Math => "Math".into(),
        DataType::MusicalNotation => "MusicalNotation".into(),
        other => other
            .id()
            .split('-')
            .map(|part| {
                let mut c = part.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect::<String>())
                    .unwrap_or_default()
            })
            .collect(),
    }
}

/// Whether a data type's simple values are IRIs, which makes its `wdt:`, `ps:`, `pq:` and
/// `pr:` predicates object properties.
fn is_iri_valued(datatype: &DataType) -> bool {
    matches!(
        datatype,
        DataType::Url
            | DataType::CommonsMedia
            | DataType::LocalMedia
            | DataType::GeoShape
            | DataType::TabularData
    ) || datatype.entity_type().is_some()
}

/// For a type with normalized values (`quantity`, `external-id`), the class of its `wdtn:`
/// predicate; `None` for the rest (wikibase-compat.md §5.3).
fn normalization(datatype: &DataType) -> Option<NamedNodeRef<'static>> {
    match datatype {
        DataType::Quantity => Some(owl::DATATYPE_PROPERTY),
        DataType::ExternalId => Some(owl::OBJECT_PROPERTY),
        _ => None,
    }
}

fn typed(value: &str, datatype: NamedNodeRef<'_>) -> Literal {
    Literal::new_typed_literal(value, datatype)
}

/// A language-tagged literal, or a plain one without a language. Wikibase writes its
/// language codes as tags as they are; a code that is not a well-formed tag is written
/// the same way rather than dropped.
fn text(value: &str, language: Option<&str>) -> Literal {
    match language {
        Some(lang) => Literal::new_language_tagged_literal(value, lang).unwrap_or_else(|_| {
            Literal::new_language_tagged_literal_unchecked(value, lang.to_ascii_lowercase())
        }),
        None => Literal::new_simple_literal(value),
    }
}

fn push(out: &mut Vec<Triple>, s: &NamedNode, p: NamedNodeRef<'_>, o: NamedNodeRef<'_>) {
    out.push(Triple::new(s.clone(), p, o));
}

/// A triple with a typed literal object.
fn lit(s: &NamedNode, p: NamedNodeRef<'_>, value: &str, datatype: NamedNodeRef<'_>) -> Triple {
    Triple::new(s.clone(), p, typed(value, datatype))
}

fn push_lit(out: &mut Vec<Triple>, s: &NamedNode, p: NamedNodeRef<'_>, o: Literal) {
    out.push(Triple::new(s.clone(), p, o));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_type_names() {
        assert_eq!(property_type_name(&DataType::Edtf), "Edtf");
        assert_eq!(property_type_name(&DataType::ExternalId), "ExternalId");
        assert_eq!(property_type_name(&DataType::WikibaseItem), "WikibaseItem");
        assert_eq!(property_type_name(&DataType::GeoShape), "GeoShape");
        assert_eq!(
            property_type_name(&DataType::GlobeCoordinate),
            "GlobeCoordinate"
        );
        assert_eq!(
            property_type_name(&DataType::MonolingualText),
            "Monolingualtext"
        );
    }
}
