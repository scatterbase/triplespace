//! Entity and statement IDs (ADR 0017 §1–3).
//!
//! Every entity ID has one of three forms, told apart by its first character and by the
//! presence of a colon:
//!
//! | Form | Shape | Examples |
//! |---|---|---|
//! | Local | One uppercase letter and digits | `Q5`, `P12`, `L3` |
//! | Foreign | Two-letter provider code, one-letter type code, and the rest in the type's grammar | `WDQ42`, `MBAb10bbbfc-…` |
//! | Keyed | The keyed type's name, a colon, and the normalized key | `domain:en.wikipedia.org` |
//!
//! No minted ID contains a colon and every keyed ID does. This module classifies and
//! canonicalizes IDs by their *shape*; whether a foreign prefix or a keyed prefix is
//! registered, and whether the rest is valid under its grammar, is checked against the
//! registries by [`EntityId::validate`], which the adapters and the write path call.
//! The model itself stays registry-free so that it can hold IDs from any source in the
//! form that source wrote them.

use std::fmt;

use scatter_normalize::KeyedRegistry;
use scatter_providers::Registry as ProviderRegistry;
use serde::{Deserialize, Serialize};

/// Which of the three forms an ID has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdForm {
    /// Minted by this instance (`Q5`).
    Local,
    /// Minted by a provider (`WDQ42`).
    Foreign,
    /// The thing itself (`domain:example.org`).
    Keyed,
}

/// An entity ID in canonical form.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(String);

/// Why a string is not an entity ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdParseError {
    /// Empty, or not one of the three shapes.
    #[error("`{0}` is not an entity ID")]
    Shape(String),
    /// A local ID with something other than digits after its letter.
    #[error("`{0}` is not a local entity ID: one uppercase letter then digits")]
    Local(String),
    /// A foreign ID the provider registry rejects.
    #[error("{0}")]
    Foreign(#[from] scatter_providers::IdError),
    /// A keyed ID the keyed-type registry rejects.
    #[error("{0}")]
    Keyed(#[from] scatter_normalize::KeyError),
    /// A statement ID without `$`, or whose entity part is not an ID, or whose GUID part
    /// is not 36 hex-and-hyphen characters.
    #[error("`{0}` is not a statement ID (`<EntityId>$<UUID>`)")]
    Statement(String),
}

impl EntityId {
    /// Parses an ID by shape, canonicalizing case: the prefix of a minted ID is uppercased
    /// and a keyed ID's type name is lowercased. The rest is kept as given, because its
    /// canonical form depends on the registries ([`EntityId::validate`]).
    ///
    /// ```
    /// use scatter_wikibase_model::{EntityId, IdForm};
    /// assert_eq!(EntityId::parse("q5").unwrap().as_str(), "Q5");
    /// assert_eq!(EntityId::parse("wdq42").unwrap().form(), IdForm::Foreign);
    /// assert_eq!(EntityId::parse("Domain:example.org").unwrap().as_str(), "domain:example.org");
    /// assert!(EntityId::parse("5").is_err());
    /// assert!(EntityId::parse("Q").is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, IdParseError> {
        if let Some((prefix, key)) = s.split_once(':') {
            if prefix.is_empty()
                || key.is_empty()
                || !prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(IdParseError::Shape(s.to_string()));
            }
            return Ok(Self(format!("{}:{key}", prefix.to_ascii_lowercase())));
        }
        let b = s.as_bytes();
        if b.len() < 2 || !b[0].is_ascii_alphabetic() {
            return Err(IdParseError::Shape(s.to_string()));
        }
        if b.len() >= 4 && b[1].is_ascii_alphabetic() && b[2].is_ascii_alphabetic() {
            // Foreign: three letters, then the rest in the type's grammar.
            let mut out = s[..3].to_ascii_uppercase();
            out.push_str(&s[3..]);
            return Ok(Self(out));
        }
        if !b[1..].iter().all(u8::is_ascii_digit) {
            return Err(IdParseError::Local(s.to_string()));
        }
        let mut out = String::with_capacity(s.len());
        out.push(b[0].to_ascii_uppercase() as char);
        out.push_str(&s[1..]);
        Ok(Self(out))
    }

    /// Checks the ID against the registries and returns it in full canonical form: a
    /// foreign ID's rest canonicalized by its type's grammar, a keyed ID's key normalized.
    /// A local ID has nothing to check beyond its shape.
    pub fn validate(
        &self,
        providers: &ProviderRegistry,
        keyed: &KeyedRegistry,
    ) -> Result<Self, IdParseError> {
        match self.form() {
            IdForm::Local => Ok(self.clone()),
            IdForm::Foreign => Ok(Self(providers.parse_foreign_id(&self.0)?.to_string())),
            IdForm::Keyed => Ok(Self(keyed.parse_id(&self.0)?.to_string())),
        }
    }

    /// The ID as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Which form the ID has.
    #[must_use]
    pub fn form(&self) -> IdForm {
        if self.0.contains(':') {
            IdForm::Keyed
        } else if self.0.len() >= 4 && self.0.as_bytes()[1].is_ascii_alphabetic() {
            IdForm::Foreign
        } else {
            IdForm::Local
        }
    }

    /// The numeric part, for the `numeric-id` Wikibase emits beside `id`: the digits of a
    /// local ID, or of a foreign ID whose rest is digits. `None` for keyed IDs and for
    /// foreign IDs under another grammar (0009 §3, 0017 §2).
    #[must_use]
    pub fn numeric_id(&self) -> Option<u64> {
        let rest = match self.form() {
            IdForm::Local => &self.0[1..],
            IdForm::Foreign => &self.0[3..],
            IdForm::Keyed => return None,
        };
        rest.parse().ok()
    }

    /// For a local ID, its type letter (`Q`, `P`, `L`). For a foreign ID, its type code.
    #[must_use]
    pub fn type_code(&self) -> Option<char> {
        match self.form() {
            IdForm::Local => self.0.chars().next(),
            IdForm::Foreign => self.0.chars().nth(2),
            IdForm::Keyed => None,
        }
    }

    /// For a keyed ID, its type name and key.
    #[must_use]
    pub fn keyed_parts(&self) -> Option<(&str, &str)> {
        self.0.split_once(':')
    }

    /// The Wikibase `entity-type` this ID's own shape implies, where it can: `item` for a
    /// local or foreign `Q`, `property` for `P`, `lexeme` for `L`, the type name for a
    /// keyed ID. Other foreign type codes need the provider registry.
    #[must_use]
    pub fn implied_entity_type(&self) -> Option<&str> {
        if let Some((name, _)) = self.keyed_parts() {
            return Some(name);
        }
        match self.type_code()? {
            'Q' => Some("item"),
            'P' => Some("property"),
            'L' => Some("lexeme"),
            _ => None,
        }
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityId({})", self.0)
    }
}

impl Serialize for EntityId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EntityId {
    /// Parses by shape, so an ID is canonical the moment it is in the model.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<str>::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for EntityId {
    type Err = IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// A statement ID, `<EntityId>$<UUID>` (wikibase-compat §2; 0017 §1; 0018 §7).
///
/// The GUID is kept as written. Wikibase compares the UUID part case-insensitively and
/// writes it in uppercase; the resolved view rewrites the entity part to the canonical
/// member's ID and keeps the UUID (0018 §7).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StatementId(String);

impl StatementId {
    /// Parses and checks the shape; the entity part is canonicalized as [`EntityId::parse`]
    /// does, the UUID part kept as given.
    pub fn parse(s: &str) -> Result<Self, IdParseError> {
        let Some((entity, uuid)) = s.rsplit_once('$') else {
            return Err(IdParseError::Statement(s.to_string()));
        };
        let entity = EntityId::parse(entity).map_err(|_| IdParseError::Statement(s.to_string()))?;
        if !is_guid(uuid) {
            return Err(IdParseError::Statement(s.to_string()));
        }
        Ok(Self(format!("{entity}${uuid}")))
    }

    /// The ID as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The entity part.
    ///
    /// # Panics
    ///
    /// Never: the constructor checked the shape.
    #[must_use]
    pub fn entity_id(&self) -> EntityId {
        EntityId(
            self.0
                .rsplit_once('$')
                .expect("checked at parse")
                .0
                .to_string(),
        )
    }

    /// The UUID part, as written.
    #[must_use]
    pub fn uuid(&self) -> &str {
        self.0.rsplit_once('$').expect("checked at parse").1
    }

    /// The same statement under another entity ID, for the canonical-ID rewrite of
    /// 0018 §7.
    #[must_use]
    pub fn with_entity(&self, entity: &EntityId) -> Self {
        Self(format!("{entity}${}", self.uuid()))
    }
}

fn is_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

impl fmt::Display for StatementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for StatementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StatementId({})", self.0)
    }
}

impl Serialize for StatementId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for StatementId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<str>::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for StatementId {
    type Err = IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_forms_by_shape() {
        let q = EntityId::parse("q5").unwrap();
        assert_eq!(
            (q.as_str(), q.form(), q.numeric_id()),
            ("Q5", IdForm::Local, Some(5))
        );
        assert_eq!(q.implied_entity_type(), Some("item"));
        let wd = EntityId::parse("wdp31").unwrap();
        assert_eq!(
            (wd.as_str(), wd.form(), wd.numeric_id()),
            ("WDP31", IdForm::Foreign, Some(31))
        );
        assert_eq!(wd.implied_entity_type(), Some("property"));
        let mb = EntityId::parse("MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d").unwrap();
        assert_eq!(mb.form(), IdForm::Foreign);
        assert_eq!(mb.numeric_id(), None);
        assert_eq!(mb.implied_entity_type(), None);
        let d = EntityId::parse("Domain:en.wikipedia.org").unwrap();
        assert_eq!(
            (d.as_str(), d.form(), d.numeric_id()),
            ("domain:en.wikipedia.org", IdForm::Keyed, None)
        );
        assert_eq!(d.implied_entity_type(), Some("domain"));
        // CAMEO codes keep their zeros (0017 §2, as amended).
        assert_eq!(EntityId::parse("GDC0311").unwrap().numeric_id(), Some(311));
    }

    #[test]
    fn rejections_by_shape() {
        for s in ["", "5", "Q", "Q5a", "QQ", ":x", "domain:", "a b:c", "L1-F1"] {
            assert!(EntityId::parse(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn validation_against_registries() {
        let p = ProviderRegistry::default_registry();
        let k = KeyedRegistry::default_registry();
        let v = |s: &str| EntityId::parse(s).unwrap().validate(p, k);
        assert_eq!(v("Q5").unwrap().as_str(), "Q5");
        assert_eq!(
            v("mbab10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D")
                .unwrap()
                .as_str(),
            "MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
        );
        assert_eq!(
            v("domain:Bücher.example").unwrap().as_str(),
            "domain:xn--bcher-kva.example"
        );
        assert!(matches!(v("ZZQ1"), Err(IdParseError::Foreign(_))));
        assert!(matches!(v("isbn:123"), Err(IdParseError::Keyed(_))));
        assert!(matches!(
            v("OAKmachine-learning"),
            Err(IdParseError::Foreign(_))
        ));
    }

    #[test]
    fn statement_ids() {
        let s = StatementId::parse("q8$C13E7A23-11E7-4C91-A799-3D1806B65444").unwrap();
        assert_eq!(s.as_str(), "Q8$C13E7A23-11E7-4C91-A799-3D1806B65444");
        assert_eq!(s.entity_id().as_str(), "Q8");
        assert_eq!(s.uuid(), "C13E7A23-11E7-4C91-A799-3D1806B65444");
        let k = StatementId::parse("domain:en.wikipedia.org$c13e7a23-11e7-4c91-a799-3d1806b65444")
            .unwrap();
        assert_eq!(k.entity_id().as_str(), "domain:en.wikipedia.org");
        let rewritten = s.with_entity(&EntityId::parse("LBQ8").unwrap());
        assert_eq!(
            rewritten.as_str(),
            "LBQ8$C13E7A23-11E7-4C91-A799-3D1806B65444"
        );
        for bad in [
            "Q8",
            "Q8$",
            "Q8$abc",
            "$C13E7A23-11E7-4C91-A799-3D1806B65444",
            "Q8$C13E7A23-11E7-4C91-A799-3D1806B6544G",
        ] {
            assert!(StatementId::parse(bad).is_err(), "{bad}");
        }
    }
}
