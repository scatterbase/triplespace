//! Forwarder keys (0057 §10): `private.forwarder_key`. A forwarder key lets a proxy (the
//! edge, the web tier) vouch for the client address it appends to `X-Forwarded-For`,
//! where trusting the proxy by its address is not enough. It acts as no account, reads
//! nothing and grants nothing. A key is `{key ID}.{secret}`, shaped like an API key
//! ([`crate::keys`]), and only the secret's SHA-256 is stored.
//!
//! This module keeps the secret's hash. The `forwarder:{key ID}` record in the instance
//! `config` partition, which puts issuing and revoking in the log, is the caller's: see
//! [`record_content`].

use std::time::SystemTime;

use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use crate::keys::check_label;
use crate::{AccountsError, random_token};

/// The header a proxy appends its key to, one comma-separated entry per hop.
pub const HEADER: &str = "triplespace-forwarder";

/// The config record kind (0015 §3).
pub const KIND: &str = "forwarder";

/// What the operator sees once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    /// The key ID, which names the config record.
    pub key_id: String,
    /// The credential a proxy sends, `{key ID}.{secret}`.
    pub key: String,
}

/// A key as it is listed: never the hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwarderInfo {
    /// The key ID.
    pub key_id: String,
    /// The operator's label.
    pub label: String,
    /// When it was issued.
    pub created: SystemTime,
    /// When it was revoked.
    pub revoked_at: Option<SystemTime>,
}

fn sha256_hex(secret: &str) -> String {
    scatter_log::hash::hex(&Sha256::digest(secret.as_bytes()))
}

/// The content of the `forwarder:{key ID}` config record for a live key: its label and
/// when it was issued, in microseconds since the epoch. Revoking is a null record.
#[must_use]
pub fn record_content(label: &str, issued_micros: u64) -> serde_json::Value {
    serde_json::json!({"kind": KIND, "label": label, "issued": issued_micros})
}

/// Issues a forwarder key. Labels are unique among live keys, so that two keys may share
/// a label only while one of them is being rotated out under a new label.
pub async fn issue<C: GenericClient>(client: &C, label: &str) -> Result<Issued, AccountsError> {
    check_label(label)?;
    let key_id = random_token(16)?;
    let secret = random_token(32)?;
    client
        .execute(
            "INSERT INTO private.forwarder_key (key_id, label, hash) VALUES ($1, $2, $3)",
            &[&key_id, &label, &sha256_hex(&secret)],
        )
        .await
        .map_err(|e| match e.as_db_error() {
            Some(db) if db.constraint() == Some("forwarder_key_live_label") => {
                AccountsError::Invalid(format!("a live forwarder key is labelled `{label}`"))
            }
            _ => AccountsError::Sql(e),
        })?;
    Ok(Issued {
        key: format!("{key_id}.{secret}"),
        key_id,
    })
}

/// Every forwarder key, newest first.
pub async fn list<C: GenericClient>(client: &C) -> Result<Vec<ForwarderInfo>, AccountsError> {
    let rows = client
        .query(
            "SELECT key_id, label, created, revoked_at FROM private.forwarder_key
             ORDER BY created DESC",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| ForwarderInfo {
            key_id: r.get(0),
            label: r.get(1),
            created: r.get(2),
            revoked_at: r.get(3),
        })
        .collect())
}

/// Revokes a key. False if it was not live.
pub async fn revoke<C: GenericClient>(client: &C, key_id: &str) -> Result<bool, AccountsError> {
    let n = client
        .execute(
            "UPDATE private.forwarder_key SET revoked_at = now()
             WHERE key_id = $1 AND revoked_at IS NULL",
            &[&key_id],
        )
        .await?;
    Ok(n == 1)
}

/// Checks a presented key, `{key ID}.{secret}`: the key ID if it is live and the secret
/// matches, else `None`. Malformed input is `None`, never an error.
pub async fn verify<C: GenericClient>(
    client: &C,
    presented: &str,
) -> Result<Option<String>, AccountsError> {
    let Some((key_id, secret)) = presented.trim().split_once('.') else {
        return Ok(None);
    };
    if key_id.is_empty() || secret.is_empty() || key_id.len() > 64 || secret.len() > 128 {
        return Ok(None);
    }
    let row = client
        .query_opt(
            "SELECT hash FROM private.forwarder_key WHERE key_id = $1 AND revoked_at IS NULL",
            &[&key_id],
        )
        .await?;
    let Some(row) = row else { return Ok(None) };
    let stored: String = row.get(0);
    let presented_hash = sha256_hex(secret);
    // Constant-time comparison of the hex digests.
    let same = stored.len() == presented_hash.len()
        && stored
            .bytes()
            .zip(presented_hash.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0;
    Ok(same.then(|| key_id.to_string()))
}
