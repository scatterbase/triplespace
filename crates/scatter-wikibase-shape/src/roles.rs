//! The role map (0003 §7): property IDs differ between Wikibases, so shape detection names
//! the properties it cares about by role, and each instance maps its own properties onto
//! the roles.

use std::collections::{BTreeMap, BTreeSet};

/// A role the statement UI uses (0003 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// A point in time: the Series shape and the sort order.
    TimePoint,
    /// When something started: the Timeline shape.
    TimeStart,
    /// When something ended: the Timeline shape.
    TimeEnd,
    /// Why a value is deprecated: the deprecated fold's labels.
    DeprecationReason,
    /// The work a reference cites: footnote titles.
    ReferenceWork,
    /// A position in a series: a Table with only this column is a numbered list.
    SeriesOrdinal,
}

impl Role {
    /// Every role, in table order.
    pub const ALL: [Role; 6] = [
        Role::TimePoint,
        Role::TimeStart,
        Role::TimeEnd,
        Role::DeprecationReason,
        Role::ReferenceWork,
        Role::SeriesOrdinal,
    ];

    /// The role's name, as the ADRs and configuration write it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Role::TimePoint => "time-point",
            Role::TimeStart => "time-start",
            Role::TimeEnd => "time-end",
            Role::DeprecationReason => "deprecation-reason",
            Role::ReferenceWork => "reference-work",
            Role::SeriesOrdinal => "series-ordinal",
        }
    }

    /// The role by its name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }

    /// Wikidata's property for the role (0003 §7).
    #[must_use]
    pub fn wikidata(self) -> &'static str {
        match self {
            Role::TimePoint => "P585",
            Role::TimeStart => "P580",
            Role::TimeEnd => "P582",
            Role::DeprecationReason => "P2241",
            Role::ReferenceWork => "P248",
            Role::SeriesOrdinal => "P1545",
        }
    }
}

/// Which properties play which role. One role may have several properties: a local one
/// and a mirrored one, say (`P12` and `WDP585` both `time-point`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roles {
    map: BTreeMap<Role, BTreeSet<String>>,
}

impl Roles {
    /// No roles: shape detection then never finds a Timeline or a Series.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Wikidata's own properties, for reading Wikidata or a Wikibase that copied its
    /// property IDs.
    #[must_use]
    pub fn wikidata() -> Self {
        let mut r = Self::empty();
        for role in Role::ALL {
            r.add(role, role.wikidata());
        }
        r
    }

    /// Wikidata's properties as an instance mirroring Wikidata under `code` holds them
    /// (`WD` → `WDP585`), beside any roles already mapped.
    #[must_use]
    pub fn with_mirror(mut self, code: &str) -> Self {
        for role in Role::ALL {
            self.add(role, &format!("{code}{}", role.wikidata()));
        }
        self
    }

    /// Maps a property onto a role.
    pub fn add(&mut self, role: Role, property: &str) {
        self.map
            .entry(role)
            .or_default()
            .insert(property.to_string());
    }

    /// Whether a property plays a role.
    #[must_use]
    pub fn is(&self, property: &str, role: Role) -> bool {
        self.map.get(&role).is_some_and(|s| s.contains(property))
    }

    /// The properties of a role.
    pub fn properties(&self, role: Role) -> impl Iterator<Item = &str> {
        self.map
            .get(&role)
            .into_iter()
            .flatten()
            .map(String::as_str)
    }

    /// The roles a property plays.
    pub fn of(&self, property: &str) -> impl Iterator<Item = Role> + '_ {
        let property = property.to_string();
        Role::ALL
            .into_iter()
            .filter(move |r| self.is(&property, *r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikidata_and_mirrored_forms() {
        let r = Roles::wikidata().with_mirror("WD");
        assert!(r.is("P585", Role::TimePoint));
        assert!(r.is("WDP585", Role::TimePoint));
        assert!(r.is("WDP580", Role::TimeStart));
        assert!(!r.is("P580", Role::TimePoint));
        assert_eq!(
            r.of("WDP2241").collect::<Vec<_>>(),
            vec![Role::DeprecationReason]
        );
        assert_eq!(Role::parse("time-end"), Some(Role::TimeEnd));
        assert!(!Roles::empty().is("P585", Role::TimePoint));
        let mut local = Roles::empty();
        local.add(Role::TimePoint, "P12");
        assert_eq!(
            local.properties(Role::TimePoint).collect::<Vec<_>>(),
            vec!["P12"]
        );
    }
}
