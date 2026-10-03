//! The `Adapter` trait (0002 §8.4): one implementation per provider type, rewriting the
//! provider's IDs to the prefixed form (`Q42` → `WDQ42`), supplying canonical IRIs, and
//! optionally mapping upstream items onto a keyed type by an identity property (0009
//! §9). [`WikibaseAdapter`] is the adapter for any Wikibase provider, including the
//! tenant's own source wiki in adoption mode (0035 §3), where IDs stay in local form.
//!
//! Statement IDs for a source that has none are derived from the entity, property and
//! value as a name-based UUID ([`deterministic_statement_id`]), so that they are stable
//! across re-imports and a local override stays attached (0002 §8.4).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use scatter_providers::{IdError, Provider, Registry};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, IdForm, IdParseError, StatementId};
use scatter_wikibase_model::statement::{Reference, Snak, SnakGroups, Statement};
use scatter_wikibase_model::value::DataValue;
use sha1::Digest as _;

/// Why an adapter could not rewrite something.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AdapterError {
    /// The provider's ID did not fit any of its types' grammars.
    #[error("{0}")]
    Id(#[from] IdError),
    /// The text is not an entity ID at all.
    #[error("{0}")]
    Parse(#[from] IdParseError),
    /// An entity-source prefix the adapter's source table does not know (wikibase-compat
    /// §5.1).
    #[error("unknown entity source `{0}`")]
    UnknownSource(String),
    /// The identity value does not normalize to a valid key (0009 §9).
    #[error("`{value}` is not a valid {keyed_type} key")]
    InvalidKey {
        /// The keyed type.
        keyed_type: String,
        /// The value.
        value: String,
    },
    /// More than one identity value (0009 §9): reported, not mapped.
    #[error("{count} identity values; an item with more than one is not mapped")]
    MultipleKeys {
        /// How many.
        count: usize,
    },
}

/// Maps upstream items onto a keyed type (0009 §9): the identity property whose value is
/// the key, and the normalization of that value.
pub trait KeyMap {
    /// The keyed type mapped onto: `domain`, `keyword`, `notation`.
    fn keyed_type(&self) -> &str;
    /// The upstream identity property, in the provider's own form (`P1`).
    fn identity_property(&self) -> &EntityId;
    /// The key an identity value normalizes to, or `None` when it is not valid.
    fn key_of(&self, value: &str) -> Option<String>;

    /// The keyed ID an upstream entity maps to, if it has exactly one valid identity
    /// value: `Ok(None)` for an entity without one, which stays an ordinary foreign item.
    fn map(&self, entity: &Entity) -> Result<Option<EntityId>, AdapterError> {
        let values: Vec<&str> = entity
            .statements
            .get(self.identity_property())
            .into_iter()
            .flatten()
            .filter_map(|s| match s.mainsnak.data_value() {
                Some(DataValue::String(v)) => Some(v.as_str()),
                _ => None,
            })
            .collect();
        match values.as_slice() {
            [] => Ok(None),
            [v] => {
                let key = self.key_of(v).ok_or_else(|| AdapterError::InvalidKey {
                    keyed_type: self.keyed_type().to_string(),
                    value: (*v).to_string(),
                })?;
                Ok(Some(EntityId::parse(&format!(
                    "{}:{key}",
                    self.keyed_type()
                ))?))
            }
            many => Err(AdapterError::MultipleKeys { count: many.len() }),
        }
    }
}

/// A provider adapter.
pub trait Adapter {
    /// The provider.
    fn provider(&self) -> &Provider;

    /// Rewrites an ID as the provider wrote it to the form it is stored in.
    fn rewrite_id(&self, upstream: &str) -> Result<EntityId, AdapterError>;

    /// The canonical IRI of a stored ID (0002 §4; 0009 §9): the provider's own for a
    /// foreign ID, the key's for a keyed one; a local ID has none here.
    fn canonical_iri(&self, id: &EntityId) -> Option<String>;

    /// The key map, for a key-mapped provider.
    fn key_map(&self) -> Option<&dyn KeyMap> {
        None
    }

    /// Rewrites every ID in an entity in place: its own, its statements' GUID prefixes,
    /// property IDs, entity values and badges. IDs already in a stored form are kept.
    fn rewrite_entity(&self, entity: &mut Entity) -> Result<(), AdapterError> {
        let id = self.rewrite_id(entity.id.as_str())?;
        entity.id = id;
        let mut statements = std::mem::take(&mut entity.statements);
        let mut rewritten = scatter_wikibase_model::entity::StatementGroups::new();
        for (property, mut group) in statements.drain(..) {
            let property = self.rewrite_id(property.as_str())?;
            for s in &mut group {
                self.rewrite_statement(s)?;
                if let Some(sid) = &s.id {
                    s.id = Some(sid.with_entity(&entity.id));
                }
            }
            rewritten.entry(property).or_default().extend(group);
        }
        entity.statements = rewritten;
        for link in entity.sitelinks.values_mut() {
            for badge in &mut link.badges {
                *badge = self.rewrite_id(badge.as_str())?;
            }
        }
        Ok(())
    }

    /// Rewrites the IDs inside one statement: snak properties and entity values.
    fn rewrite_statement(&self, statement: &mut Statement) -> Result<(), AdapterError> {
        self.rewrite_snak(&mut statement.mainsnak)?;
        statement.qualifiers = self.rewrite_groups(std::mem::take(&mut statement.qualifiers))?;
        for r in &mut statement.references {
            let snaks = std::mem::take(&mut r.snaks);
            *r = Reference {
                snaks: self.rewrite_groups(snaks)?,
                hash: r.hash.take(),
            };
        }
        Ok(())
    }

    /// Rewrites a snak's property and entity value.
    fn rewrite_snak(&self, snak: &mut Snak) -> Result<(), AdapterError> {
        snak.property = self.rewrite_id(snak.property.as_str())?;
        if let scatter_wikibase_model::statement::SnakKind::Value(DataValue::EntityId(v)) =
            &mut snak.kind
        {
            v.id = self.rewrite_id(v.id.as_str())?;
        }
        Ok(())
    }

    /// Rewrites snak groups, keys included.
    fn rewrite_groups(&self, groups: SnakGroups) -> Result<SnakGroups, AdapterError> {
        let mut out = SnakGroups::new();
        for (property, mut snaks) in groups {
            let property = self.rewrite_id(property.as_str())?;
            for s in &mut snaks {
                self.rewrite_snak(s)?;
            }
            out.entry(property).or_default().extend(snaks);
        }
        Ok(out)
    }
}

/// The adapter for a Wikibase provider.
#[derive(Debug, Clone)]
pub struct WikibaseAdapter {
    provider: Provider,
    registry: &'static Registry,
    /// Adoption mode (0035 §3): the source's own IDs are the tenant's and stay local.
    home: bool,
    /// The source's entity-source table (wikibase-compat §5.1): prefix → provider code.
    sources: BTreeMap<String, String>,
}

impl WikibaseAdapter {
    /// An adapter for a registered provider; `None` if the slug is not registered.
    #[must_use]
    pub fn new(slug: &str) -> Option<Self> {
        Self::with_registry(slug, Registry::default_registry())
    }

    /// An adapter for a provider of the given registry.
    #[must_use]
    pub fn with_registry(slug: &str, registry: &'static Registry) -> Option<Self> {
        Some(Self {
            provider: registry.by_slug(slug)?.clone(),
            registry,
            home: false,
            sources: BTreeMap::new(),
        })
    }

    /// Adoption mode: IDs the source minted stay in local form; only IDs under an
    /// entity-source prefix are rewritten, through the source table.
    #[must_use]
    pub fn home(mut self) -> Self {
        self.home = true;
        self
    }

    /// Adds an entity-source prefix (`wikidata`) and the provider code it stands for.
    #[must_use]
    pub fn with_source(mut self, prefix: &str, code: &str) -> Self {
        self.sources
            .insert(prefix.to_string(), code.to_ascii_uppercase());
        self
    }

    /// The registry.
    #[must_use]
    pub fn registry(&self) -> &'static Registry {
        self.registry
    }
}

impl Adapter for WikibaseAdapter {
    fn provider(&self) -> &Provider {
        &self.provider
    }

    fn rewrite_id(&self, upstream: &str) -> Result<EntityId, AdapterError> {
        // `wikidata:Q42` from a federated install: through the source table.
        if let Some((prefix, rest)) = upstream.split_once(':') {
            if let Some(code) = self.sources.get(prefix) {
                let provider = self
                    .registry
                    .by_code(code)
                    .ok_or_else(|| AdapterError::UnknownSource(prefix.to_string()))?;
                return Ok(EntityId::parse(
                    &self.registry.from_upstream(provider, rest)?.to_string(),
                )?);
            }
            // A keyed ID (`domain:example.org`) is already in its stored form.
            return Ok(EntityId::parse(upstream)?);
        }
        let id = EntityId::parse(upstream)?;
        if self.home {
            return Ok(id);
        }
        match id.form() {
            IdForm::Local => Ok(EntityId::parse(
                &self
                    .registry
                    .from_upstream(&self.provider, upstream)?
                    .to_string(),
            )?),
            // Already prefixed, or keyed: kept.
            IdForm::Foreign | IdForm::Keyed => Ok(id),
        }
    }

    fn canonical_iri(&self, id: &EntityId) -> Option<String> {
        match id.form() {
            IdForm::Foreign => Some(
                self.registry
                    .parse_foreign_id(id.as_str())
                    .ok()?
                    .concept_iri()
                    .as_str()
                    .to_string(),
            ),
            IdForm::Keyed => {
                let (t, key) = id.keyed_parts()?;
                Some(format!("https://scatter.red/{t}/{key}"))
            }
            IdForm::Local => None,
        }
    }
}

/// The UUID namespace for derived statement IDs: `uuid5(DNS, "scatter.red")`,
/// `d4e715d9-4b64-5ace-b731-1f68d6664f8a`.
const NAMESPACE: [u8; 16] = [
    0xd4, 0xe7, 0x15, 0xd9, 0x4b, 0x64, 0x5a, 0xce, 0xb7, 0x31, 0x1f, 0x68, 0xd6, 0x66, 0x4f, 0x8a,
];

/// A statement ID for a statement its source gave none (0002 §8.4): a name-based UUID
/// (version 5, SHA-1) over the subject, the property and the main snak's serialization
/// under `hasher`, written in uppercase as Wikibase writes GUIDs. The same statement on
/// the same subject always gets the same ID.
#[must_use]
pub fn deterministic_statement_id(
    subject: &EntityId,
    statement: &Statement,
    hasher: &Hasher,
) -> StatementId {
    let mut h = sha1::Sha1::new();
    h.update(NAMESPACE);
    h.update(subject.as_str().as_bytes());
    h.update([0]);
    h.update(hasher.snak_serialization(&statement.mainsnak).as_bytes());
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex = b.iter().fold(String::with_capacity(32), |mut acc, x| {
        let _ = write!(acc, "{x:02X}");
        acc
    });
    let guid = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    StatementId::parse(&format!("{subject}${guid}")).expect("a well-formed GUID")
}

/// Gives every statement without an ID a deterministic one.
pub fn assign_statement_ids(entity: &mut Entity, hasher: &Hasher) {
    let subject = entity.id.clone();
    for s in entity.statements.values_mut().flatten() {
        if s.id.is_none() {
            s.id = Some(deterministic_statement_id(&subject, s, hasher));
        }
    }
}
