//! `private.binding` (0088 §7; 0089 §3, §4; 07 §2.7): the index of bindings. The binding
//! records are the truth (0007 §8); a row is written in the transaction that appends its
//! record and points at it, so that removing the sign-in method can erase the record. Two
//! kinds of row: a login's external identity, with the empty tenant and the holder
//! `login:{n}`, whose record is in the instance's `logins` partition; and a tenant account's
//! binding to its login, with the tenant, the issuer `login` and the login ID as subject,
//! whose holder is the actor key and whose record is in the tenant's `accounts` partition.
//! A password's index is `private.password`, keyed by login.

use std::time::SystemTime;

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use tokio_postgres::GenericClient;

use crate::password::PAYLOAD_BINDING;
use crate::{AccountsError, password};

/// One external binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The issuer.
    pub issuer: String,
    /// The subject, normalized.
    pub subject: String,
    /// The holder: `login:{n}` for a login's external identity, an actor key for an
    /// account's binding to its login.
    pub holder: String,
    /// The binding record's partition: 64 random bits.
    pub partition: u64,
    /// The binding record's offset.
    pub offset: u64,
    /// When it was added.
    pub added: SystemTime,
    /// When it was last used to log in.
    pub last_used: Option<SystemTime>,
}

fn u64_of(n: i64) -> u64 {
    u64::try_from(n).unwrap_or_default()
}

/// A partition ID as the `bigint` that stores it: 64 random bits, reinterpreted
/// (0018 §2), as `scatter-log-postgres` stores them.
#[allow(clippy::cast_possible_wrap)]
fn partition_to_db(partition: u64) -> i64 {
    partition as i64
}

#[allow(clippy::cast_sign_loss)]
fn partition_from_db(db: i64) -> u64 {
    db as u64
}

fn i64_of(n: u64) -> Result<i64, AccountsError> {
    i64::try_from(n).map_err(|e| AccountsError::Invalid(e.to_string()))
}

fn binding_of(r: &tokio_postgres::Row) -> Binding {
    Binding {
        issuer: r.get("issuer"),
        subject: r.get("subject"),
        holder: r.get("holder"),
        partition: partition_from_db(r.get("partition")),
        offset: u64_of(r.get("offset")),
        added: r.get("added"),
        last_used: r.get("last_used"),
    }
}

const COLUMNS: &str = "issuer, subject, holder, \"partition\", \"offset\", added, last_used";

/// The binding record for a subject of an issuer (payloads.md §8), keyed by its holder:
/// a login's key for an external identity, an account's actor key for its login.
///
/// # Errors
///
/// If the body cannot be built.
pub fn draft(
    holder: &str,
    issuer: &str,
    subject: &str,
    attested_by: &str,
    now: u64,
) -> Result<Draft, AccountsError> {
    let content = Value::map(vec![
        (Value::text("issuer"), Value::text(issuer)),
        (Value::text("subject"), Value::text(subject)),
    ]);
    Ok(Draft {
        appended_at: now,
        payload_type: PAYLOAD_BINDING.into(),
        key: Some(holder.to_string()),
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

/// The holder a subject is bound to: with the empty tenant, the `login:{n}` an external
/// identity authenticates; with a tenant and the issuer `login`, the account.
///
/// # Errors
///
/// If the store fails.
pub async fn find<C: GenericClient>(
    client: &C,
    tenant: &str,
    issuer: &str,
    subject: &str,
) -> Result<Option<String>, AccountsError> {
    Ok(client
        .query_opt(
            "SELECT holder FROM private.binding WHERE tenant = $1 AND issuer = $2 AND subject = $3",
            &[&tenant, &issuer, &subject],
        )
        .await?
        .map(|r| r.get(0)))
}

/// A holder's bindings, oldest first: a login's external identities for `login:{n}`.
///
/// # Errors
///
/// If the store fails.
pub async fn of<C: GenericClient>(client: &C, holder: &str) -> Result<Vec<Binding>, AccountsError> {
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM private.binding WHERE holder = $1 AND issuer <> 'login' ORDER BY added, issuer"
            ),
            &[&holder],
        )
        .await?;
    Ok(rows.iter().map(binding_of).collect())
}

/// A login's external identities, oldest first.
///
/// # Errors
///
/// If the store fails.
pub async fn of_login<C: GenericClient>(
    client: &C,
    login: i64,
) -> Result<Vec<Binding>, AccountsError> {
    of(client, &crate::logins::key_of(login)).await
}

/// Indexes a binding record just appended.
///
/// # Errors
///
/// A store error, including the subject being bound already.
pub async fn insert<C: GenericClient>(
    client: &C,
    tenant: &str,
    issuer: &str,
    subject: &str,
    holder: &str,
    partition: u64,
    offset: u64,
) -> Result<(), AccountsError> {
    client
        .execute(
            "INSERT INTO private.binding (tenant, issuer, subject, holder, \"partition\", \"offset\", last_used)
             VALUES ($1, $2, $3, $4, $5, $6, now())",
            &[
                &tenant,
                &issuer,
                &subject,
                &holder,
                &partition_to_db(partition),
                &i64_of(offset)?,
            ],
        )
        .await?;
    Ok(())
}

/// Notes a login through a binding.
///
/// # Errors
///
/// If the store fails.
pub async fn touch<C: GenericClient>(
    client: &C,
    tenant: &str,
    issuer: &str,
    subject: &str,
) -> Result<(), AccountsError> {
    client
        .execute(
            "UPDATE private.binding SET last_used = now() WHERE tenant = $1 AND issuer = $2 AND subject = $3",
            &[&tenant, &issuer, &subject],
        )
        .await?;
    Ok(())
}

/// A holder's binding to an issuer, locked for the caller's transaction.
///
/// # Errors
///
/// If the store fails.
pub async fn take<C: GenericClient>(
    client: &C,
    holder: &str,
    issuer: &str,
) -> Result<Option<Binding>, AccountsError> {
    Ok(client
        .query_opt(
            &format!(
                "SELECT {COLUMNS} FROM private.binding WHERE holder = $1 AND issuer = $2 FOR UPDATE"
            ),
            &[&holder, &issuer],
        )
        .await?
        .as_ref()
        .map(binding_of))
}

/// Deletes a binding's row.
///
/// # Errors
///
/// If the store fails.
pub async fn delete<C: GenericClient>(
    client: &C,
    holder: &str,
    issuer: &str,
) -> Result<(), AccountsError> {
    client
        .execute(
            "DELETE FROM private.binding WHERE holder = $1 AND issuer = $2",
            &[&holder, &issuer],
        )
        .await?;
    Ok(())
}

/// How many ways a login has to log in: its external identities and its password.
///
/// # Errors
///
/// If the store fails.
pub async fn ways_to_log_in<C: GenericClient>(
    client: &C,
    login: i64,
) -> Result<usize, AccountsError> {
    let n: i64 = client
        .query_one(
            "SELECT count(*) FROM private.binding WHERE holder = $1 AND tenant = ''",
            &[&crate::logins::key_of(login)],
        )
        .await?
        .get(0);
    Ok(usize::try_from(n).unwrap_or_default() + usize::from(password::has(client, login).await?))
}

/// How many external identities a login holds.
///
/// # Errors
///
/// If the store fails.
pub async fn external_count<C: GenericClient>(
    client: &C,
    login: i64,
) -> Result<usize, AccountsError> {
    let n: i64 = client
        .query_one(
            "SELECT count(*) FROM private.binding WHERE holder = $1 AND tenant = '' AND issuer <> 'login'",
            &[&crate::logins::key_of(login)],
        )
        .await?
        .get(0);
    Ok(usize::try_from(n).unwrap_or_default())
}

/// The rows of external identities still keyed by a tenant account, for `logins migrate`
/// (0089 §15), oldest first.
///
/// # Errors
///
/// If the store fails.
pub async fn unmigrated<C: GenericClient>(
    client: &C,
) -> Result<Vec<(String, Binding)>, AccountsError> {
    let rows = client
        .query(
            &format!(
                "SELECT tenant, {COLUMNS} FROM private.binding WHERE tenant <> '' AND issuer <> 'login' AND issuer <> 'password' ORDER BY added"
            ),
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<_, String>("tenant"), binding_of(r)))
        .collect())
}

/// Deletes a row by its full key, for `logins migrate`.
///
/// # Errors
///
/// If the store fails.
pub async fn delete_row<C: GenericClient>(
    client: &C,
    tenant: &str,
    issuer: &str,
    subject: &str,
) -> Result<(), AccountsError> {
    client
        .execute(
            "DELETE FROM private.binding WHERE tenant = $1 AND issuer = $2 AND subject = $3",
            &[&tenant, &issuer, &subject],
        )
        .await?;
    Ok(())
}
