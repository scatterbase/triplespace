//! Triplespace's entity IRIs in Wikibase's RDF (0115 §15): the `rdf` feature.
//!
//! `scatter-wikibase-rdf` names every entity under the instance's base unless an
//! [`EntityIris`] names it elsewhere. [`TriplespaceIris`] is Triplespace's: a foreign ID is
//! named by its provider type's template applied to the upstream ID (0002 §4; 05 §1.5), a
//! source entity's likewise (0078 §7; 02 §2.3), and a keyed ID by its type's template
//! (04 §3.5). An ID neither knows is named under the instance's base, which is what an
//! unpublished source's concept IRI is (02 §2.3). [`upstream_nodes`] resolves the nodes a
//! mirrored statement is named by, as its upstream names them (05 §1.5; 0002 A43, A44).

use oxrdf::NamedNode;
use scatter_keyed::KeyedRegistry;
use scatter_wikibase_model::EntityId;
use scatter_wikibase_rdf::EntityIris;

use crate::id::{IdForm, TriplespaceId as _};
use crate::provider::ProviderModel;

/// Triplespace's [`EntityIris`]: foreign and source IDs through a provider model, keyed IDs
/// through the keyed-type registry.
#[derive(Clone, Copy)]
pub struct TriplespaceIris<'r> {
    providers: &'r (dyn ProviderModel + Send + Sync),
    keyed: &'r KeyedRegistry,
}

impl std::fmt::Debug for TriplespaceIris<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriplespaceIris").finish_non_exhaustive()
    }
}

impl<'r> TriplespaceIris<'r> {
    /// Names over `providers` and `keyed`.
    #[must_use]
    pub const fn new(
        providers: &'r (dyn ProviderModel + Send + Sync),
        keyed: &'r KeyedRegistry,
    ) -> Self {
        Self { providers, keyed }
    }

    /// Names over the embedded provider and keyed-type registries.
    #[cfg(feature = "providers")]
    #[must_use]
    pub fn embedded() -> TriplespaceIris<'static> {
        TriplespaceIris::new(
            scatter_providers::Registry::default_registry(),
            KeyedRegistry::default_registry(),
        )
    }
}

impl EntityIris for TriplespaceIris<'_> {
    fn concept(&self, id: &EntityId) -> Option<NamedNode> {
        match id.form() {
            IdForm::Foreign => {
                let (base, upstream) = self.providers.upstream_concept(id.as_str())?;
                NamedNode::new(format!("{base}{upstream}")).ok()
            }
            IdForm::Keyed => self.keyed.parse_id(id.as_str()).ok().map(|k| k.iri()),
            IdForm::Local => None,
        }
    }

    fn property(&self, id: &EntityId) -> Option<(String, String)> {
        if id.form() == IdForm::Foreign {
            self.providers.upstream_concept(id.as_str())
        } else {
            None
        }
    }
}

/// The nodes each statement in `upstream` is named by as its upstream names it
/// (05 §1.5; 0002 A43), for `scatter-wikibase-rdf`'s `Renderer::with_upstream`: the
/// statement node `{concept base}statement/{upstream ID}-{UUID}`, after the entity the
/// [`UpstreamRef`](scatter_providers::UpstreamRef) names (the subject of the statement's ID,
/// or for a key-mapped provider's statement the item mapped onto the keyed subject,
/// 0002 A44), and its reference and value nodes under the provider's base, hashed over the
/// IDs as the provider writes them. A statement whose upstream is not an entity of a
/// Wikibase-shaped provider in `providers` is left out, and so named under the instance's
/// base. A value the adapter rewrote to a keyed ID (`domain:example.org` for the provider's
/// `Q9`) hashes as stored, so a reference that holds one gets a node the provider's RDF does
/// not have.
#[cfg(feature = "providers")]
#[must_use]
pub fn upstream_nodes(
    providers: &scatter_providers::Registry,
    upstream: &std::collections::BTreeMap<
        scatter_wikibase_model::StatementId,
        scatter_providers::UpstreamRef,
    >,
) -> std::collections::BTreeMap<
    scatter_wikibase_model::StatementId,
    scatter_wikibase_rdf::UpstreamNodes,
> {
    let iris = std::sync::Arc::new(crate::ValueIris::new(providers, None));
    upstream
        .iter()
        .filter_map(|(id, u)| {
            let (provider, entity_type) = u.resolve(providers)?;
            let (concept_base, "") = entity_type.iri.split_once("{upstream_id}")? else {
                return None;
            };
            let nodes = scatter_wikibase_rdf::UpstreamNodes {
                statement: NamedNode::new_unchecked(format!(
                    "{concept_base}statement/{}-{}",
                    scatter_wikibase_rdf::escape_iri(&u.id),
                    scatter_wikibase_rdf::escape_iri(id.uuid())
                )),
                base: scatter_wikibase_rdf::predicate_base(concept_base),
                hasher: crate::hasher_for(provider).with_iris(iris.clone()),
            };
            Some((id.clone(), nodes))
        })
        .collect()
}

#[cfg(all(test, feature = "providers"))]
mod tests {
    use super::*;
    use scatter_providers::{Registry, UpstreamRef};
    use scatter_wikibase_model::{Hasher, StatementId};
    use scatter_wikibase_rdf::{Family, IriScheme, Namespaces};
    use std::collections::BTreeMap;

    fn scheme() -> IriScheme<'static> {
        IriScheme::new(
            Namespaces::with_wiki_path("https://example.org"),
            TriplespaceIris::embedded(),
        )
    }

    fn iri(s: &IriScheme<'_>, id: &str) -> String {
        s.concept(&EntityId::parse(id).unwrap()).into_string()
    }

    // 0115 §15: local, foreign and keyed concepts.
    #[test]
    fn local_foreign_and_keyed_concepts() {
        let s = scheme();
        assert_eq!(iri(&s, "Q5"), "https://example.org/entity/Q5");
        assert_eq!(iri(&s, "WDQ42"), "http://www.wikidata.org/entity/Q42");
        assert_eq!(
            iri(&s, "domain:en.wikipedia.org"),
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
    fn a_concept_base_without_entity_keeps_prop_after_it() {
        let registry = Registry::parse(
            "version = 1\n[[provider]]\ncode = \"ZZ\"\nslug = \"zed\"\nnumber = 99\nissuer = \"zed\"\nrevision_ids = true\nadapter = \"scatter-adapter-wikibase\"\n[[provider.type]]\ncode = \"P\"\nentity_type = \"property\"\nupstream_prefix = \"P\"\niri = \"https://zed.example/id/{upstream_id}\"\nnamespace = \"Property\"\n",
        )
        .unwrap();
        let s = IriScheme::new(
            Namespaces::with_wiki_path("https://example.org"),
            TriplespaceIris::new(&registry, KeyedRegistry::default_registry()),
        );
        let p = s.property(&EntityId::parse("ZZP5").unwrap());
        assert_eq!(
            p.iri(Family::Direct).as_str(),
            "https://zed.example/id/prop/direct/P5"
        );
    }

    #[test]
    fn upstream_statement_nodes() {
        let providers = Registry::default_registry();
        let id = StatementId::parse("WDQ42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9").unwrap();
        let keyed = StatementId::parse("domain:wikipedia.org$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9")
            .unwrap();
        let other = StatementId::parse("Q1$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9").unwrap();
        let map: BTreeMap<StatementId, UpstreamRef> = [
            (
                id.clone(),
                UpstreamRef::of(&providers.parse_foreign_id("WDQ42").unwrap()),
            ),
            // A key-mapped provider's statement is stored under the keyed subject with the
            // item's UUID; named after the item it was mapped from (0002 A44).
            (
                keyed.clone(),
                UpstreamRef::mapped(providers.by_code("XD").unwrap(), "Q11").unwrap(),
            ),
            // A provider the registry does not know names nothing.
            (
                other.clone(),
                UpstreamRef {
                    provider: "ZZ".into(),
                    type_code: 'Q',
                    id: "Q1".into(),
                },
            ),
        ]
        .into();
        let nodes = upstream_nodes(providers, &map);
        let iris = std::sync::Arc::new(crate::ValueIris::new(providers, None));
        let u = &nodes[&id];
        assert_eq!(
            u.statement.as_str(),
            "http://www.wikidata.org/entity/statement/Q42-F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
        );
        assert_eq!(u.base, "http://www.wikidata.org/");
        // The provider's own hashes, over value IRIs as it wrote them (0004 A20).
        assert_eq!(
            u.hasher,
            Hasher::mirrored_from("WD").with_iris(iris.clone())
        );
        let u = &nodes[&keyed];
        assert_eq!(
            u.statement.as_str(),
            "https://internetdomains.wiki/entity/statement/Q11-F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
        );
        assert_eq!(u.base, "https://internetdomains.wiki/");
        assert_eq!(u.hasher, Hasher::mirrored_from("XD").with_iris(iris));
        assert!(!nodes.contains_key(&other));
    }

    /// A Domain's document node is `Special:EntityData/domain:en.wikipedia.org` on each
    /// instance's own article path, though its concept IRI is the same everywhere (0009 §6).
    #[test]
    fn keyed_document_node_is_instance_data() {
        let s = scheme();
        let id = EntityId::parse("domain:en.wikipedia.org").unwrap();
        assert_eq!(
            s.document(&id).as_str(),
            "https://example.org/wiki/Special:EntityData/domain:en.wikipedia.org"
        );
        let other = IriScheme::new(
            Namespaces::with_wiki_path("https://elsewhere.example"),
            TriplespaceIris::embedded(),
        );
        assert_eq!(
            other.document(&id).as_str(),
            "https://elsewhere.example/wiki/Special:EntityData/domain:en.wikipedia.org"
        );
        assert_eq!(
            other.concept(&id).as_str(),
            "https://scatter.red/domain/en.wikipedia.org"
        );
    }

    /// The key is used in A-label form, so the IRI is plain ASCII (0009 §6).
    #[test]
    fn keyed_concept_iri_uses_the_a_label() {
        assert_eq!(
            iri(&scheme(), "domain:bücher.example"),
            "https://scatter.red/domain/xn--bcher-kva.example"
        );
    }

    /// Each keyed type's IRI template is registry data (0017 §3).
    #[test]
    fn keyword_and_notation_concepts_follow_their_templates() {
        let s = scheme();
        assert_eq!(
            iri(&s, "keyword:saxophone"),
            "https://scatter.red/keyword/saxophone"
        );
        assert_eq!(
            iri(&s, "notation:osm:amenity=cafe"),
            "https://scatter.red/notation/osm:amenity=cafe"
        );
    }

    /// "A source declared without an `iri` is unpublished. Its concept IRI is then
    /// `{base}/entity/{name}:{upstream ID}` under the tenant's base" (0078 §7).
    #[test]
    fn unpublished_source_falls_back_to_the_tenant_base() {
        let s = scheme();
        assert_eq!(iri(&s, "mhc:Q1"), "https://example.org/entity/mhc:Q1");
        assert_eq!(
            s.document(&EntityId::parse("mhc:Q1").unwrap()).as_str(),
            "https://example.org/wiki/Special:EntityData/mhc:Q1"
        );
    }
}
