//! The built-in `password` issuer (0007 §1 decision #1; 0089 §3; 07 §2.2): a sign-in
//! method of an instance login. The hash lives in `private.password`, keyed by the login;
//! one password serves every tenant account bound to the login. Argon2id with the library
//! defaults (0033 §6); the PHC string carries its own parameters, so they can change
//! without a migration. Rows still keyed by an actor key are what `triplespace logins
//! migrate` converts (0089 §15).

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use scatter_actors::issuer::PASSWORD;
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

/// Sets (or replaces) the login's password.
pub async fn set<C: GenericClient>(
    client: &C,
    login: i64,
    password: &str,
) -> Result<(), AccountsError> {
    if password.chars().count() < 8 {
        return Err(AccountsError::Invalid(
            "a password has at least 8 characters".into(),
        ));
    }
    let phc = hash(password)?;
    set_hash(client, login, &phc).await
}

/// Sets the account's password from a hash made earlier, as an approved request's
/// (0087 §6), which kept only the hash.
pub async fn set_hash<C: GenericClient>(
    client: &C,
    login: i64,
    phc: &str,
) -> Result<(), AccountsError> {
    if PasswordHash::new(phc).is_err() {
        return Err(AccountsError::Hash("not a PHC string".into()));
    }
    client
        .execute(
            "INSERT INTO private.password (login, hash, updated) VALUES ($1, $2, now())
             ON CONFLICT (login) WHERE login IS NOT NULL DO UPDATE SET hash = EXCLUDED.hash, updated = now()",
            &[&login, &phc],
        )
        .await?;
    Ok(())
}

/// Whether the login has this password. `false` for a login without one.
pub async fn check<C: GenericClient>(
    client: &C,
    login: i64,
    password: &str,
) -> Result<bool, AccountsError> {
    let row = client
        .query_opt(
            "SELECT hash FROM private.password WHERE login = $1",
            &[&login],
        )
        .await?;
    Ok(row.is_some_and(|r| verify(&r.get::<_, String>(0), password)))
}

/// Whether the login has a password at all.
pub async fn has<C: GenericClient>(client: &C, login: i64) -> Result<bool, AccountsError> {
    Ok(client
        .query_opt("SELECT 1 FROM private.password WHERE login = $1", &[&login])
        .await?
        .is_some())
}

/// Removes the login's password.
pub async fn remove<C: GenericClient>(client: &C, login: i64) -> Result<bool, AccountsError> {
    Ok(client
        .execute("DELETE FROM private.password WHERE login = $1", &[&login])
        .await?
        > 0)
}

/// The rows still keyed by an actor key, for `logins migrate` (0089 §15): the actor key
/// and the hash.
pub async fn unmigrated<C: GenericClient>(
    client: &C,
) -> Result<Vec<(String, String)>, AccountsError> {
    let rows = client
        .query(
            "SELECT actor_key, hash FROM private.password WHERE login IS NULL AND actor_key IS NOT NULL ORDER BY updated",
            &[],
        )
        .await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// Moves an actor-keyed row to its login (0089 §15).
pub async fn migrate_row<C: GenericClient>(
    client: &C,
    actor_key: &str,
    login: i64,
) -> Result<(), AccountsError> {
    client
        .execute(
            "UPDATE private.password SET login = $2, actor_key = NULL WHERE actor_key = $1 AND login IS NULL",
            &[&actor_key, &login],
        )
        .await?;
    Ok(())
}

/// The binding record of a password, as accounts held them before 0089 §3 moved
/// passwords to the login (payloads.md §8): keyed by the account,
/// `{"issuer": "password", "subject": "{user id}"}`. Kept for reading old records; a login's
/// password has no binding record, since the hash in `private.password` is the whole of it.
pub fn binding_draft(
    actor_key: &str,
    user_id: u64,
    attested_by: &str,
    now: u64,
) -> Result<Draft, AccountsError> {
    crate::bindings::draft(actor_key, PASSWORD, &user_id.to_string(), attested_by, now)
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
