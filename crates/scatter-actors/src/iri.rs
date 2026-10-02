//! Actor IRIs (0007 §2; 0046 §7).
//!
//! | Actor | IRI |
//! |---|---|
//! | Tenant account (`local:{id}`, or the tenant's slug) | `{tenant base}/user/{id}` |
//! | Farm account | `{farm base}/instance/user/{id}` |
//! | The instance as operator | `{farm base}/instance/operator` |
//! | User of a registered issuer with a template | The template with `{id}` filled in |
//! | Surrogate (0007 §5) | `{tenant base}/actor/{n}` |
//!
//! An IRI never contains a name or an IP address, so it is still valid after either is
//! erased. Issuers whose IDs are never published (`wikimedia-central`, `password`) have no
//! IRI, and a request for one is an error, not a guess.

use oxrdf::NamedNode;

use crate::issuer::{ActorModel, INSTANCE, IssuerRegistry, LOCAL};
use crate::key::ActorKey;

/// The bases IRIs are minted under.
#[derive(Debug, Clone, Copy)]
pub struct IriContext<'a> {
    /// The tenant's slug, which is its issuer code.
    pub tenant_slug: &'a str,
    /// The tenant's base, `https://librarybase.org`.
    pub tenant_base: &'a str,
    /// The farm's slug, the instance's own.
    pub farm_slug: &'a str,
    /// The farm base (0046 §6), which may equal a tenant's base.
    pub farm_base: &'a str,
}

/// Why an actor has no IRI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IriError {
    /// The issuer is not registered.
    #[error("actor `{0}`: unknown issuer")]
    UnknownIssuer(ActorKey),
    /// The issuer publishes no actor IRIs.
    #[error("actor `{0}`: the issuer `{1}` publishes no actor IRIs")]
    NotPublished(ActorKey, String),
    /// The issuer has no individual actors.
    #[error("actor `{0}`: the issuer has no individual actors; attribute to the provider")]
    ProviderOnly(ActorKey),
    /// The result is not an IRI.
    #[error("actor `{0}`: `{1}` is not an IRI")]
    Invalid(ActorKey, String),
}

fn base(s: &str) -> &str {
    s.trim_end_matches('/')
}

/// The IRI of an actor.
pub fn actor_iri(
    key: &ActorKey,
    registry: &IssuerRegistry,
    ctx: IriContext<'_>,
) -> Result<NamedNode, IriError> {
    let iri = if key.issuer() == LOCAL || key.issuer() == ctx.tenant_slug {
        format!("{}/user/{}", base(ctx.tenant_base), key.subject())
    } else if key.issuer() == INSTANCE {
        format!("{}/instance/operator", base(ctx.farm_base))
    } else if key.issuer() == ctx.farm_slug {
        format!("{}/instance/user/{}", base(ctx.farm_base), key.subject())
    } else {
        let issuer = registry
            .by_code(key.issuer())
            .ok_or_else(|| IriError::UnknownIssuer(key.clone()))?;
        if issuer.actor_model == ActorModel::ProviderOnly {
            return Err(IriError::ProviderOnly(key.clone()));
        }
        let template = issuer
            .iri
            .as_ref()
            .ok_or_else(|| IriError::NotPublished(key.clone(), issuer.code.clone()))?;
        template.replace("{id}", key.subject())
    };
    NamedNode::new(&iri).map_err(|_| IriError::Invalid(key.clone(), iri))
}

/// The IRI of a surrogate actor (0007 §5): `{tenant base}/actor/{n}`.
///
/// # Panics
///
/// Never for a base that is itself an IRI.
#[must_use]
pub fn surrogate_iri(tenant_base: &str, n: u64) -> NamedNode {
    NamedNode::new(format!("{}/actor/{n}", base(tenant_base))).expect("an IRI from an IRI base")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTX: IriContext<'static> = IriContext {
        tenant_slug: "librarybase",
        tenant_base: "https://librarybase.org/",
        farm_slug: "scatter",
        farm_base: "https://scatter.red",
    };

    #[test]
    fn iris_by_issuer() {
        let r = IssuerRegistry::default_registry();
        let iri = |k: &str| {
            actor_iri(&ActorKey::parse(k).unwrap(), r, CTX).map(oxrdf::NamedNode::into_string)
        };
        assert_eq!(iri("local:42").unwrap(), "https://librarybase.org/user/42");
        assert_eq!(
            iri("librarybase:42").unwrap(),
            "https://librarybase.org/user/42"
        );
        assert_eq!(
            iri("scatter:7").unwrap(),
            "https://scatter.red/instance/user/7"
        );
        assert_eq!(
            iri("instance:scatter").unwrap(),
            "https://scatter.red/instance/operator"
        );
        assert_eq!(
            iri("wikidatawiki:12345").unwrap(),
            "https://www.wikidata.org/wiki/Special:Redirect/user/12345"
        );
        assert!(matches!(
            iri("wikimedia-central:1"),
            Err(IriError::NotPublished(..))
        ));
        assert!(matches!(iri("password:1"), Err(IriError::NotPublished(..))));
        assert!(matches!(iri("openalex:x"), Err(IriError::ProviderOnly(_))));
        assert!(matches!(iri("nobody:1"), Err(IriError::UnknownIssuer(_))));
        assert_eq!(
            surrogate_iri("https://librarybase.org", 7).as_str(),
            "https://librarybase.org/actor/7"
        );
    }
}
