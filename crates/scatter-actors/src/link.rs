//! Account links (0007 §7; 0028 §2): the `scatter:v0/link-account` payload and its rules.
//!
//! A link says that one person holds two accounts, so that attribution carries across
//! sources. Nothing links accounts except the holder's own request, proved through an
//! identity provider; the one exception is a tenant account's link to the farm account it
//! was created from, by the consent given once at farm signup. A link is removed by
//! erasure, never by a strike, since the history would disclose the same thing.
//!
//! The proof of control (the OAuth round trip and the `centralids` query) is
//! `triplespace-accounts`'s; this module holds what is true of the link itself.

use serde::{Deserialize, Serialize};

use crate::issuer::{ActorModel, IssuerRegistry, LOCAL};
use crate::key::ActorKey;

/// The content part of a `scatter:v0/link-account` record, keyed by the local actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLink {
    /// The foreign account, under its own issuer.
    pub foreign: ActorKey,
}

/// Who asked for the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRequest {
    /// The holder of the local account, having proved control of the foreign one.
    Holder,
    /// Account creation from a farm account, under the consent of farm signup (0028 §2).
    FarmSignup,
    /// Anyone else: an administrator, an adapter, an inference from data.
    Other,
}

/// Why a link may not be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LinkError {
    /// Only the holder, or farm signup, creates a link.
    #[error("only the account holder can link an account")]
    NotHolder,
    /// The foreign account is local, or is the account itself.
    #[error("`{0}` is not a foreign account")]
    NotForeign(ActorKey),
    /// The foreign issuer is not registered, or has no individual actors.
    #[error("`{0}`: no accounts can be linked under this issuer")]
    Issuer(ActorKey),
    /// The foreign account is already linked to another local account.
    #[error("`{foreign}` is already linked to `{holder}`")]
    AlreadyLinked {
        /// The foreign account.
        foreign: ActorKey,
        /// The local account it is linked to.
        holder: ActorKey,
    },
    /// A farm-signup link whose foreign side is not the farm.
    #[error("a farm-signup link must name a farm account, not `{0}`")]
    NotFarm(ActorKey),
}

/// Checks whether `local` may be linked to `foreign` (0007 §7, 0028 §2). `linked_to` is
/// the local account `foreign` is already linked to, if any, from `view.account_link`;
/// `farm_slug` is the farm's issuer code where the policy gives the farm identity.
pub fn check_link(
    local: &ActorKey,
    foreign: &ActorKey,
    request: LinkRequest,
    registry: &IssuerRegistry,
    farm_slug: Option<&str>,
    linked_to: Option<&ActorKey>,
) -> Result<(), LinkError> {
    if foreign.issuer() == LOCAL || foreign == local {
        return Err(LinkError::NotForeign(foreign.clone()));
    }
    match request {
        LinkRequest::Holder => {}
        LinkRequest::FarmSignup => {
            if farm_slug.is_none_or(|f| f != foreign.issuer()) {
                return Err(LinkError::NotFarm(foreign.clone()));
            }
        }
        LinkRequest::Other => return Err(LinkError::NotHolder),
    }
    let is_farm = farm_slug.is_some_and(|f| f == foreign.issuer());
    if !is_farm {
        let issuer = registry
            .by_code(foreign.issuer())
            .ok_or_else(|| LinkError::Issuer(foreign.clone()))?;
        if issuer.actor_model != ActorModel::Numeric {
            return Err(LinkError::Issuer(foreign.clone()));
        }
    }
    if let Some(holder) = linked_to
        && holder != local
    {
        return Err(LinkError::AlreadyLinked {
            foreign: foreign.clone(),
            holder: holder.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_rules() {
        let r = IssuerRegistry::default_registry();
        let me = ActorKey::local(42);
        let wd = ActorKey::parse("wikidatawiki:12345").unwrap();
        assert!(check_link(&me, &wd, LinkRequest::Holder, r, None, None).is_ok());
        // Re-asserting one's own link is fine; someone else's is not.
        assert!(check_link(&me, &wd, LinkRequest::Holder, r, None, Some(&me)).is_ok());
        assert!(matches!(
            check_link(
                &me,
                &wd,
                LinkRequest::Holder,
                r,
                None,
                Some(&ActorKey::local(1))
            ),
            Err(LinkError::AlreadyLinked { .. })
        ));
        assert!(matches!(
            check_link(&me, &wd, LinkRequest::Other, r, None, None),
            Err(LinkError::NotHolder)
        ));
        assert!(matches!(
            check_link(&me, &ActorKey::local(7), LinkRequest::Holder, r, None, None),
            Err(LinkError::NotForeign(_))
        ));
        assert!(matches!(
            check_link(
                &me,
                &ActorKey::parse("openalex:x").unwrap(),
                LinkRequest::Holder,
                r,
                None,
                None
            ),
            Err(LinkError::Issuer(_))
        ));
        assert!(matches!(
            check_link(
                &me,
                &ActorKey::parse("nobody:1").unwrap(),
                LinkRequest::Holder,
                r,
                None,
                None
            ),
            Err(LinkError::Issuer(_))
        ));
        // Farm signup links to the farm account and nothing else.
        let farm = ActorKey::parse("scatter:9").unwrap();
        assert!(
            check_link(
                &me,
                &farm,
                LinkRequest::FarmSignup,
                r,
                Some("scatter"),
                None
            )
            .is_ok()
        );
        assert!(matches!(
            check_link(&me, &wd, LinkRequest::FarmSignup, r, Some("scatter"), None),
            Err(LinkError::NotFarm(_))
        ));
        assert!(matches!(
            check_link(&me, &farm, LinkRequest::FarmSignup, r, None, None),
            Err(LinkError::NotFarm(_))
        ));
        let link = AccountLink { foreign: wd };
        assert_eq!(
            serde_json::to_string(&link).unwrap(),
            r#"{"foreign":"wikidatawiki:12345"}"#
        );
    }
}
