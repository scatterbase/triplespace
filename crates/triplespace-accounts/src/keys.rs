//! Subsidiary API keys (0024 §4, §10): `private.api_key`. A key is `{key ID}.{secret}`;
//! the key ID is 16 random bytes and the secret 32, both URL-safe base64 without padding,
//! and only the secret's SHA-256 is stored. The secret has 256 bits of entropy, so a fast
//! hash is the right one: there is nothing to guess slowly. Grants are checked against the
//! registry at issue; a key never adds a permission its account lacks (evaluation does the
//! intersection, [`scatter_actors::evaluate::evaluate`]).

use std::collections::BTreeSet;
use std::time::SystemTime;

use scatter_actors::grant::{BASIC, GrantRegistry};
use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use crate::{AccountsError, random_token};

/// What an operator sees once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    /// The key ID, which sessions and revocation name.
    pub key_id: String,
    /// The bearer credential, `{key ID}.{secret}`.
    pub bearer: String,
}

/// A key as it is listed: never the hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// The key ID.
    pub key_id: String,
    /// The operator's label.
    pub label: String,
    /// The grants.
    pub grants: Vec<String>,
    /// When it was made.
    pub created: SystemTime,
    /// When it stops working, if ever.
    pub expires: Option<SystemTime>,
    /// When it was last used.
    pub last_used: Option<SystemTime>,
    /// When it was revoked.
    pub revoked_at: Option<SystemTime>,
}

/// A key that authenticated a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAuth {
    /// The subsidiary's actor key.
    pub actor_key: String,
    /// The key ID.
    pub key_id: String,
    /// The key's grants.
    pub grants: BTreeSet<String>,
}

fn sha256_hex(secret: &str) -> String {
    scatter_log::hash::hex(&Sha256::digest(secret.as_bytes()))
}

/// A label: 1–64 characters, letters, digits, `-`, `_`, `.`; no `@`, which separates it
/// from the account name in a bot-password login (0024 §4).
pub fn check_label(label: &str) -> Result<(), AccountsError> {
    let ok = !label.is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok {
        Ok(())
    } else {
        Err(AccountsError::Invalid(format!(
            "label `{label}`: 1–64 letters, digits, `-`, `_` or `.`"
        )))
    }
}

/// Issues a key for the account. `basic` is always included (grants.toml); every other
/// grant must be registered.
pub async fn issue<C: GenericClient>(
    client: &C,
    actor_key: &str,
    label: &str,
    grants: &[String],
    expires: Option<SystemTime>,
) -> Result<Issued, AccountsError> {
    check_label(label)?;
    let registry = GrantRegistry::default_registry();
    let mut names: BTreeSet<String> = grants.iter().cloned().collect();
    names.insert(BASIC.to_string());
    for g in &names {
        if registry.by_name(g).is_none() {
            return Err(AccountsError::Invalid(format!("unknown grant `{g}`")));
        }
    }
    let key_id = random_token(16)?;
    let secret = random_token(32)?;
    let grants: Vec<String> = names.into_iter().collect();
    client
        .execute(
            "INSERT INTO private.api_key (actor_key, key_id, label, hash, grants, expires)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &actor_key,
                &key_id,
                &label,
                &sha256_hex(&secret),
                &grants,
                &expires,
            ],
        )
        .await
        .map_err(|e| match e.as_db_error() {
            Some(db) if db.constraint() == Some("api_key_actor_key_label_key") => {
                AccountsError::Invalid(format!(
                    "`{actor_key}` already has a key labelled `{label}`"
                ))
            }
            _ => AccountsError::Sql(e),
        })?;
    Ok(Issued {
        bearer: format!("{key_id}.{secret}"),
        key_id,
    })
}

/// The account's keys, newest first.
pub async fn list<C: GenericClient>(
    client: &C,
    actor_key: &str,
) -> Result<Vec<KeyInfo>, AccountsError> {
    let rows = client
        .query(
            "SELECT key_id, label, grants, created, expires, last_used, revoked_at
             FROM private.api_key WHERE actor_key = $1 ORDER BY created DESC",
            &[&actor_key],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| KeyInfo {
            key_id: r.get(0),
            label: r.get(1),
            grants: r.get(2),
            created: r.get(3),
            expires: r.get(4),
            last_used: r.get(5),
            revoked_at: r.get(6),
        })
        .collect())
}

/// Revokes a key, and with it every session it opened (0024 §4).
pub async fn revoke<C: GenericClient>(
    client: &C,
    actor_key: &str,
    key_id: &str,
) -> Result<bool, AccountsError> {
    let n = client
        .execute(
            "UPDATE private.api_key SET revoked_at = now()
             WHERE actor_key = $1 AND key_id = $2 AND revoked_at IS NULL",
            &[&actor_key, &key_id],
        )
        .await?;
    client
        .execute("DELETE FROM private.session WHERE key_id = $1", &[&key_id])
        .await?;
    Ok(n > 0)
}

/// A live key row, by key ID.
async fn live<C: GenericClient>(
    client: &C,
    key_id: &str,
) -> Result<Option<(String, String, Vec<String>)>, AccountsError> {
    let row = client
        .query_opt(
            "SELECT actor_key, hash, grants FROM private.api_key
             WHERE key_id = $1 AND revoked_at IS NULL AND (expires IS NULL OR expires > now())",
            &[&key_id],
        )
        .await?;
    Ok(row.map(|r| (r.get(0), r.get(1), r.get(2))))
}

/// Notes a use, at most once a minute (0024 §4).
async fn touch<C: GenericClient>(client: &C, key_id: &str) -> Result<(), AccountsError> {
    client
        .execute(
            "UPDATE private.api_key SET last_used = now()
             WHERE key_id = $1 AND (last_used IS NULL OR last_used < now() - interval '1 minute')",
            &[&key_id],
        )
        .await?;
    Ok(())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Authenticates a bearer credential, `{key ID}.{secret}` (0024 §4).
pub async fn authenticate_bearer<C: GenericClient>(
    client: &C,
    bearer: &str,
) -> Result<Option<KeyAuth>, AccountsError> {
    let Some((key_id, secret)) = bearer.split_once('.') else {
        return Ok(None);
    };
    let Some((actor_key, hash, grants)) = live(client, key_id).await? else {
        return Ok(None);
    };
    if !constant_time_eq(&hash, &sha256_hex(secret)) {
        return Ok(None);
    }
    touch(client, key_id).await?;
    Ok(Some(KeyAuth {
        actor_key,
        key_id: key_id.to_string(),
        grants: grants.into_iter().collect(),
    }))
}

/// Authenticates a bot-password login: the account's key with this label, and the secret.
pub async fn authenticate_label<C: GenericClient>(
    client: &C,
    actor_key: &str,
    label: &str,
    secret: &str,
) -> Result<Option<KeyAuth>, AccountsError> {
    let row = client
        .query_opt(
            "SELECT key_id, hash, grants FROM private.api_key
             WHERE actor_key = $1 AND label = $2 AND revoked_at IS NULL
               AND (expires IS NULL OR expires > now())",
            &[&actor_key, &label],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let (key_id, hash, grants): (String, String, Vec<String>) =
        (row.get(0), row.get(1), row.get(2));
    if !constant_time_eq(&hash, &sha256_hex(secret)) {
        return Ok(None);
    }
    touch(client, &key_id).await?;
    Ok(Some(KeyAuth {
        actor_key: actor_key.to_string(),
        key_id,
        grants: grants.into_iter().collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert!(check_label("toolforge").is_ok());
        assert!(check_label("my-laptop_2.0").is_ok());
        assert!(check_label("").is_err());
        assert!(check_label("has@sign").is_err());
        assert!(check_label("has space").is_err());
    }

    #[test]
    fn equality_is_length_and_bytes() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
