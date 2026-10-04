//! The built-in `password` issuer (0007 §1 decision #1, §3 as amended; 0016 §3): a hash
//! in `private.password` and a `scatter:v0/binding` record in the tenant's `accounts`
//! partition that says the account has one. Argon2id with the library defaults (0033 §6);
//! the PHC string carries its own parameters, so they can change without a migration.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use scatter_actors::issuer::PASSWORD;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use tokio_postgres::GenericClient;

use crate::AccountsError;

/// The payload type of a binding record (payloads.md §8).
pub const PAYLOAD_BINDING: &str = "scatter:v0/binding";

/// Hashes a password.
pub fn hash(password: &str) -> Result<String, AccountsError> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| AccountsError::Random(e.to_string()))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| AccountsError::Hash(e.to_string()))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AccountsError::Hash(e.to_string()))?
        .to_string())
}

/// Whether the password matches the PHC string.
#[must_use]
pub fn verify(phc: &str, password: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

/// Sets (or replaces) the account's password.
pub async fn set<C: GenericClient>(
    client: &C,
    actor_key: &str,
    password: &str,
) -> Result<(), AccountsError> {
    if password.chars().count() < 8 {
        return Err(AccountsError::Invalid(
            "a password has at least 8 characters".into(),
        ));
    }
    let phc = hash(password)?;
    client
        .execute(
            "INSERT INTO private.password (actor_key, hash, updated) VALUES ($1, $2, now())
             ON CONFLICT (actor_key) DO UPDATE SET hash = EXCLUDED.hash, updated = now()",
            &[&actor_key, &phc],
        )
        .await?;
    Ok(())
}

/// Whether the account has this password. `false` for an account without one.
pub async fn check<C: GenericClient>(
    client: &C,
    actor_key: &str,
    password: &str,
) -> Result<bool, AccountsError> {
    let row = client
        .query_opt(
            "SELECT hash FROM private.password WHERE actor_key = $1",
            &[&actor_key],
        )
        .await?;
    Ok(row.is_some_and(|r| verify(&r.get::<_, String>(0), password)))
}

/// Whether the account has a password at all.
pub async fn has<C: GenericClient>(client: &C, actor_key: &str) -> Result<bool, AccountsError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM private.password WHERE actor_key = $1",
            &[&actor_key],
        )
        .await?
        .is_some())
}

/// The binding record that goes with a password (payloads.md §8): keyed by the account,
/// `{"issuer": "password", "subject": "{user id}"}`, in the tenant's `accounts` partition.
/// The caller appends it; the hash never leaves `private`.
pub fn binding_draft(
    actor_key: &str,
    user_id: u64,
    attested_by: &str,
    now: u64,
) -> Result<Draft, AccountsError> {
    let content = Value::map(vec![
        (Value::text("issuer"), Value::text(PASSWORD)),
        (Value::text("subject"), Value::text(&user_id.to_string())),
    ]);
    Ok(Draft {
        appended_at: now,
        payload_type: PAYLOAD_BINDING.into(),
        key: Some(actor_key.to_string()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &content,
            &Value::Null,
            &Value::map(vec![(Value::text("actor"), Value::text(attested_by))]),
        )
        .map_err(|e| AccountsError::Invalid(format!("binding body: {e}")))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_differ() {
        let a = hash("correct horse battery staple").unwrap();
        let b = hash("correct horse battery staple").unwrap();
        assert_ne!(a, b, "salted");
        assert!(a.starts_with("$argon2id$"));
        assert!(verify(&a, "correct horse battery staple"));
        assert!(!verify(&a, "Correct horse battery staple"));
        assert!(!verify("not a hash", "x"));
    }

    #[test]
    fn binding_is_a_core_record() {
        let d = binding_draft("librarybase:7", 7, "instance:librarybase", 1).unwrap();
        assert_eq!(d.payload_type, PAYLOAD_BINDING);
        assert_eq!(d.key.as_deref(), Some("librarybase:7"));
    }
}
