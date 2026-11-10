//! Name tombstones (0010 §10; 19 §2.3): every name a local account has released, by
//! rename or vanish, stays reserved, so that `[[User:OldName]]` in old text can never come
//! to name a different person. A vanished account's names are erased from the log, so the
//! tombstone is not a projection: `private.name_tombstone` holds a keyed hash of the
//! tenant and the lower-cased name under the instance secret, no actor key and no
//! plaintext, and reveals nothing but that a candidate name is reserved, which the "name
//! taken" check reveals anyway.
//!
//! **Under strict names a tombstone yields to the issuer that named it** (0102 §3; 07
//! §2.10): a row remembers the OAuth issuer whose username the released name was derived
//! from, and [`held`] reports it, so that the same issuer attesting the name for another
//! subject takes it, since the provider has already made the choice the tombstone exists
//! to prevent. A tombstone of any other origin holds.

use std::fmt::Write as _;

use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;
use tokio_postgres::GenericClient;

use crate::AccountsError;
use crate::requests::name_key;
use crate::token::Secret;

/// A tombstone, as the name check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tombstone {
    /// The OAuth issuer whose username the released name was derived from under strict
    /// names, which may take the name back; `None` for a name of any other origin.
    pub issuer: Option<String>,
}

/// The keyed hash a name is stored under: HMAC-SHA256 over the tenant and the lower-cased
/// name, under a subkey of the instance secret, hex. The tenant is part of the input
/// rather than a column, so that the row says nothing about where the name was held.
#[must_use]
pub fn key(secret: &Secret, tenant: &str, name: &str) -> String {
    let k = secret.subkey("triplespace-accounts:name-tombstone:v1:");
    let mut mac = Hmac::<Sha256>::new_from_slice(&k).expect("any key length");
    mac.update(tenant.as_bytes());
    mac.update(b"\0");
    mac.update(name_key(name).as_bytes());
    let out = mac.finalize().into_bytes();
    out.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Reserves a released name. `issuer` is the OAuth issuer that named it under strict
/// names, or `None`. A name released twice, which only a yielded tombstone allows, keeps
/// one row with the later release.
///
/// # Errors
///
/// If the store fails.
pub async fn reserve<C: GenericClient>(
    client: &C,
    secret: &Secret,
    tenant: &str,
    name: &str,
    issuer: Option<&str>,
) -> Result<(), AccountsError> {
    client
        .execute(
            "INSERT INTO private.name_tombstone (name_hmac, issuer) VALUES ($1, $2)
             ON CONFLICT (name_hmac) DO UPDATE SET released = now(), issuer = EXCLUDED.issuer",
            &[&key(secret, tenant, name), &issuer],
        )
        .await?;
    Ok(())
}

/// The tombstone on a name, if one holds.
///
/// # Errors
///
/// If the store fails.
pub async fn held<C: GenericClient>(
    client: &C,
    secret: &Secret,
    tenant: &str,
    name: &str,
) -> Result<Option<Tombstone>, AccountsError> {
    let row = client
        .query_opt(
            "SELECT issuer FROM private.name_tombstone WHERE name_hmac = $1",
            &[&key(secret, tenant, name)],
        )
        .await?;
    Ok(row.map(|r| Tombstone { issuer: r.get(0) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_case_insensitive_and_per_tenant() {
        let s = Secret::from_bytes([3u8; 32]);
        assert_eq!(key(&s, "lb", "Alice B"), key(&s, "lb", "alice_b"));
        assert_ne!(key(&s, "lb", "Alice"), key(&s, "other", "Alice"));
        assert_ne!(
            key(&s, "lb", "Alice"),
            key(&Secret::from_bytes([4u8; 32]), "lb", "Alice")
        );
        assert_eq!(key(&s, "lb", "Alice").len(), 64);
    }
}
