//! Resolver namespaces read (06 §3.3): which resolver and key a string names, and which
//! entities hold the key in the viewer's tenant's resolved view.
//!
//! The resolvers that serve are those bound to a property ([`bound`]): the role-bound
//! defaults of `resolvers.toml`, bound to Wikidata's property for the role in its mirrored
//! form, as the statement UI reads its roles. `view.identifier` keys a bound property's
//! values with the resolver's normalizer (`crate::entities`), so the lookup and the index
//! agree by construction (06 §3.1).

pub use scatter_normalize::Resolver;
use scatter_normalize::{ResolverRegistry, SchemeRegistry};
use scatter_wikibase_model::key::Bound;

use crate::acl_read::Hidden;

/// The most entities a lookup reports.
pub const MAX_CANDIDATES: i64 = 50;

/// The resolvers bound to properties (06 §3.1), from the embedded registries: what
/// `view.identifier` keys a value by, and so what the lookup and a match key read it by.
#[must_use]
pub fn bound() -> &'static Bound {
    Bound::embedded()
}

/// The resolvers that serve: those bound to at least one property.
#[must_use]
pub fn serving() -> Vec<&'static Resolver> {
    ResolverRegistry::default_registry()
        .resolvers()
        .iter()
        .filter(|r| bound().properties(&r.name).next().is_some())
        .collect()
}

/// A resolver's key, as a string names it.
#[derive(Debug, PartialEq, Eq)]
pub enum Keyed<'a> {
    /// A valid key of the resolver.
    Key(&'a Resolver, String),
    /// Prefixed with the resolver's name, but not a valid key: why.
    Invalid(&'a Resolver, String),
}

/// The role-bound resolver whose name or namespace prefixes `q`, and the key after it
/// (06 §3.2): `doi:10.1000/x` and `DOI:10.1000/x` alike.
fn prefixed<'a>(q: &str, resolvers: &[&'a Resolver]) -> Option<Keyed<'a>> {
    let (prefix, rest) = q.split_once(':')?;
    let r = resolvers.iter().find(|r| {
        r.name.eq_ignore_ascii_case(prefix.trim())
            || r.namespace.eq_ignore_ascii_case(prefix.trim())
    })?;
    Some(match r.normalize(rest) {
        Ok(key) => Keyed::Key(r, key),
        Err(e) => Keyed::Invalid(r, e.to_string()),
    })
}

/// The one resolver of which `q` as written is a valid key, and the key; `None` where it
/// is a key of none or of several.
fn bare<'a>(q: &str, resolvers: &[&'a Resolver]) -> Option<Keyed<'a>> {
    let mut keys = resolvers
        .iter()
        .filter_map(|r| r.normalize(q).ok().map(|k| Keyed::Key(r, k)));
    let first = keys.next()?;
    keys.next().is_none().then_some(first)
}

/// The resolver's key for `q`: prefixed, else bare.
pub fn keyed<'a>(q: &str, resolvers: &[&'a Resolver]) -> Option<Keyed<'a>> {
    prefixed(q, resolvers).or_else(|| bare(q, resolvers))
}

/// Whether a string is a notation key of a registered scheme (`osm:amenity=cafe`; 0048
/// §2): the one bare key that resolves (0029 A9).
#[must_use]
pub fn is_notation_key(q: &str) -> bool {
    scatter_normalize::notation::split(q)
        .is_some_and(|(scheme, _)| SchemeRegistry::default_registry().by_name(scheme).is_some())
}

/// One entity a key names: its canonical member, as `(id, type)`, and its values for the
/// resolver's properties.
pub struct Found {
    /// The canonical member's ID.
    pub id: String,
    /// Its entity type.
    pub entity_type: String,
    /// Its values for the resolver's properties, as keys.
    pub values: Vec<String>,
}

/// The entities holding `key` for one of `properties` (06 §3.3 step 2), in the viewer's
/// tenant's resolved view: a shared row stands only where the tenant has not overlaid its
/// entity (03 §7), and a deleted or hidden entity does not count. Each is reported as the
/// canonical member of its cluster, with every value it holds for the properties.
///
/// # Errors
/// The database's.
pub async fn lookup(
    db: &tokio_postgres::Client,
    tenant: &str,
    hidden: &Hidden,
    properties: &[&str],
    key: &str,
) -> Result<Vec<Found>, tokio_postgres::Error> {
    if hidden.every_entity() || properties.is_empty() {
        return Ok(Vec::new());
    }
    let value_key = format!("external-id\u{1F}{key}");
    let rows = db
        .query(
            "WITH hit AS (
               SELECT DISTINCT i.entity_id FROM view.identifier i
               WHERE i.tenant IN ($1, '') AND i.property = ANY($2) AND i.value_key = $3
                 AND (i.tenant = $1 OR NOT EXISTS (
                   SELECT 1 FROM view.entity o WHERE o.tenant = $1 AND o.id = i.entity_id))
             ), row AS (
               SELECT DISTINCT ON (h.entity_id) h.entity_id, e.canonical_id, e.deleted, e.type
               FROM hit h JOIN view.entity e ON e.id = h.entity_id AND e.tenant IN ($1, '')
               ORDER BY h.entity_id, (e.tenant = $1) DESC
             ), live AS (
               SELECT entity_id, canonical_id FROM row
               WHERE NOT deleted AND NOT (entity_id = ANY($4)) AND NOT (type = ANY($5))
             )
             SELECT DISTINCT ON (r.id) r.id, r.type, l.entity_id
             FROM live l
             JOIN LATERAL (
               SELECT x.id, x.type FROM view.entity x
               WHERE x.tenant IN ($1, '') AND x.id IN (l.canonical_id, l.entity_id) AND NOT x.deleted
                 AND NOT (x.id = ANY($4)) AND NOT (x.type = ANY($5))
               ORDER BY (x.id = l.canonical_id) DESC, (x.tenant = $1) DESC
               LIMIT 1) r ON true
             ORDER BY r.id
             LIMIT $6",
            &[
                &tenant,
                &properties,
                &value_key,
                &hidden.entity_ids(),
                &hidden.entity_types(),
                &MAX_CANDIDATES,
            ],
        )
        .await?;
    let mut found = Vec::with_capacity(rows.len());
    for row in rows {
        let member: String = row.get(2);
        let values = db
            .query(
                "SELECT DISTINCT substr(i.value_key, 13) FROM view.identifier i
                 WHERE i.tenant IN ($1, '') AND i.entity_id = $2 AND i.property = ANY($3)
                   AND (i.tenant = $1 OR NOT EXISTS (
                     SELECT 1 FROM view.entity o WHERE o.tenant = $1 AND o.id = i.entity_id))
                 ORDER BY 1",
                &[&tenant, &member, &properties],
            )
            .await?
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect();
        found.push(Found {
            id: row.get(0),
            entity_type: row.get(1),
            values,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doi() -> Vec<&'static Resolver> {
        vec![ResolverRegistry::default_registry().by_name("doi").unwrap()]
    }

    // 06 §3.2, §3.3: a resolver's name or namespace prefixes its key, in any case; a bare
    // DOI is a key of the one resolver; a prefixed string that is no DOI is invalid.
    #[test]
    fn keys_are_read_prefixed_or_bare() {
        let rs = doi();
        let key = |q: &str| match keyed(q, &rs) {
            Some(Keyed::Key(r, k)) => Some((r.name.clone(), k)),
            _ => None,
        };
        let want = Some(("doi".to_string(), "10.1000/xyz".to_string()));
        assert_eq!(key("doi:10.1000/XYZ"), want);
        assert_eq!(key("DOI:10.1000/xyz"), want);
        assert_eq!(key("10.1000/XYZ"), want);
        assert_eq!(key("https://doi.org/10.1000/xyz"), want);
        assert_eq!(key("Q42"), None);
        assert!(matches!(keyed("DOI:11.1/x", &rs), Some(Keyed::Invalid(..))));
        assert_eq!(keyed("Douglas Adams", &rs), None);
        assert_eq!(
            serving()
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["doi"]
        );
        assert!(is_notation_key("osm:amenity=cafe"));
        assert!(!is_notation_key("doi:10.1000/x"));
    }
}
