//! Account requests (0087 §6; 07 §2.6): with `account.approval` on, registering files a
//! request instead of creating an account, as ConfirmAccount's request form does.
//!
//! A request is a row of `private.account_request`, never a record: a name that is never
//! approved never enters the log. A pending request holds its name against the
//! name-taken check. Approving creates the account (the caller's to do, through the write
//! path) and deletes the row; rejecting keeps it, with the reason, for 7 days, so that
//! its requester learns the outcome at login. An open request is deleted after
//! `account.request_expiry`.

use scatter_actors::actor::normalize_name;
use serde_json::{Value, json};
use tokio_postgres::GenericClient;

use crate::{AccountsError, password};

/// How long a rejected request is kept, so its requester is told why (0087 §6).
pub const REJECTED_KEPT_SECS: i64 = 7 * 86_400;

/// The longest statement a request may carry, in characters.
pub const STATEMENT_MAX: usize = 1_000;

/// A request, as a reviewer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Its ID.
    pub id: String,
    /// The name asked for.
    pub name: String,
    /// What the requester wrote about themselves.
    pub statement: Option<String>,
    /// The address key it was filed from.
    pub address_key: Option<String>,
    /// When it was filed, in seconds since the epoch.
    pub filed: i64,
    /// When it expires unreviewed.
    pub expires: i64,
}

/// What a login with a request's credential learns (0087 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still waiting: `ts-account-pending`.
    Pending,
    /// Rejected, with the reviewer's reason: `ts-account-rejected`.
    Rejected(String),
}

/// The lower-cased form a name is compared in.
#[must_use]
pub fn name_key(name: &str) -> String {
    normalize_name(name).to_lowercase()
}

/// Deletes what is no longer kept: open requests past their expiry, and rejected ones a
/// week after their rejection.
///
/// # Errors
///
/// If the store fails.
pub async fn sweep<C: GenericClient>(client: &C) -> Result<(), AccountsError> {
    client
        .execute(
            "DELETE FROM private.account_request
             WHERE (decision IS NULL AND expires < now())
                OR (decision = 'rejected' AND decided < now() - make_interval(secs => $1::bigint))",
            &[&REJECTED_KEPT_SECS],
        )
        .await?;
    Ok(())
}

/// Whether a name is taken on a tenant: by an account, case-insensitively, or by a
/// request still held (0087 §6).
///
/// # Errors
///
/// If the store fails.
pub async fn name_taken<C: GenericClient>(
    client: &C,
    tenant: &str,
    name: &str,
) -> Result<bool, AccountsError> {
    sweep(client).await?;
    let key = name_key(name);
    let row = client
        .query_opt(
            "SELECT 1 FROM view.actor WHERE tenant = $1 AND lower(name) = $2
             UNION ALL
             SELECT 1 FROM private.account_request WHERE tenant = $1 AND name_key = $2
             LIMIT 1",
            &[&tenant, &key],
        )
        .await?;
    Ok(row.is_some())
}

/// Files a request for a `password` account. The password is hashed here and kept only
/// as its hash. Returns the request's ID.
///
/// # Errors
///
/// [`AccountsError::Invalid`] for a password under 8 characters or a statement over
/// [`STATEMENT_MAX`]; a store error, including the name being taken by a request filed
/// at the same moment.
pub async fn file<C: GenericClient>(
    client: &C,
    tenant: &str,
    name: &str,
    password_text: &str,
    statement: Option<&str>,
    address_key: &str,
    expiry_secs: i64,
) -> Result<String, AccountsError> {
    if password_text.chars().count() < 8 {
        return Err(AccountsError::Invalid(
            "a password has at least 8 characters".into(),
        ));
    }
    let statement = statement.map(str::trim).filter(|s| !s.is_empty());
    if statement.is_some_and(|s| s.chars().count() > STATEMENT_MAX) {
        return Err(AccountsError::Invalid(format!(
            "a statement has at most {STATEMENT_MAX} characters"
        )));
    }
    let id = crate::random_token(12)?;
    let name = normalize_name(name);
    let credential =
        json!({"issuer": scatter_actors::issuer::PASSWORD, "hash": password::hash(password_text)?});
    client
        .execute(
            "INSERT INTO private.account_request
               (id, tenant, name, name_key, credential, statement, address_key, expires)
             VALUES ($1, $2, $3, $4, $5, $6, $7, now() + make_interval(secs => $8::bigint))",
            &[
                &id,
                &tenant,
                &name,
                &name_key(&name),
                &credential,
                &statement,
                &address_key,
                &expiry_secs,
            ],
        )
        .await?;
    Ok(id)
}

fn request_of(r: &tokio_postgres::Row) -> Request {
    Request {
        id: r.get("id"),
        name: r.get("name"),
        statement: r.get("statement"),
        address_key: r.get("address_key"),
        filed: r.get("filed_s"),
        expires: r.get("expires_s"),
    }
}

const COLUMNS: &str = "id, name, statement, address_key,
    extract(epoch FROM filed)::bigint AS filed_s, extract(epoch FROM expires)::bigint AS expires_s";

/// The open requests on a tenant, oldest first.
///
/// # Errors
///
/// If the store fails.
pub async fn open<C: GenericClient>(
    client: &C,
    tenant: &str,
) -> Result<Vec<Request>, AccountsError> {
    sweep(client).await?;
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM private.account_request
                 WHERE tenant = $1 AND decision IS NULL ORDER BY filed, id"
            ),
            &[&tenant],
        )
        .await?;
    Ok(rows.iter().map(request_of).collect())
}

/// How many requests are open on a tenant.
///
/// # Errors
///
/// If the store fails.
pub async fn open_count<C: GenericClient>(client: &C, tenant: &str) -> Result<i64, AccountsError> {
    let row = client
        .query_one(
            "SELECT count(*) FROM private.account_request
             WHERE tenant = $1 AND decision IS NULL AND expires >= now()",
            &[&tenant],
        )
        .await?;
    Ok(row.get(0))
}

/// An open request and the hash of its password, locked for the caller's transaction,
/// so two reviewers cannot both approve it.
///
/// # Errors
///
/// If the store fails.
pub async fn take<C: GenericClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Option<(Request, String)>, AccountsError> {
    let row = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS}, credential FROM private.account_request
                 WHERE tenant = $1 AND id = $2 AND decision IS NULL AND expires >= now()
                 FOR UPDATE"
            ),
            &[&tenant, &id],
        )
        .await?;
    Ok(row.and_then(|r| {
        let credential: Value = r.get("credential");
        let hash = credential["hash"].as_str()?.to_string();
        Some((request_of(&r), hash))
    }))
}

/// Deletes a request: once its account is created.
///
/// # Errors
///
/// If the store fails.
pub async fn delete<C: GenericClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<(), AccountsError> {
    client
        .execute(
            "DELETE FROM private.account_request WHERE tenant = $1 AND id = $2",
            &[&tenant, &id],
        )
        .await?;
    Ok(())
}

/// Rejects an open request with a reason. `false` when there is no open request by that
/// ID.
///
/// # Errors
///
/// If the store fails.
pub async fn reject<C: GenericClient>(
    client: &C,
    tenant: &str,
    id: &str,
    reviewer: &str,
    reason: &str,
) -> Result<bool, AccountsError> {
    let n = client
        .execute(
            "UPDATE private.account_request
             SET decision = 'rejected', reviewer = $3, decided = now(), reason = $4
             WHERE tenant = $1 AND id = $2 AND decision IS NULL AND expires >= now()",
            &[&tenant, &id, &reviewer, &reason],
        )
        .await?;
    Ok(n == 1)
}

/// What a login with a name and password learns from a request: `None` where there is
/// no request by that name or the password is not the request's, so a stranger learns
/// nothing a wrong password would not tell them.
///
/// # Errors
///
/// If the store fails.
pub async fn outcome<C: GenericClient>(
    client: &C,
    tenant: &str,
    name: &str,
    password_text: &str,
) -> Result<Option<Outcome>, AccountsError> {
    sweep(client).await?;
    let row = client
        .query_opt(
            "SELECT credential, decision, reason FROM private.account_request
             WHERE tenant = $1 AND name_key = $2",
            &[&tenant, &name_key(name)],
        )
        .await?;
    let Some(r) = row else { return Ok(None) };
    let credential: Value = r.get(0);
    let matches = credential["hash"]
        .as_str()
        .is_some_and(|h| password::verify(h, password_text));
    if !matches {
        return Ok(None);
    }
    let decision: Option<String> = r.get(1);
    Ok(Some(if decision.as_deref() == Some("rejected") {
        Outcome::Rejected(r.get::<_, Option<String>>(2).unwrap_or_default())
    } else {
        Outcome::Pending
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_lower_cased() {
        assert_eq!(name_key("alice_smith"), "alice smith");
        assert_eq!(name_key("Alice Smith"), name_key("alice smith"));
    }
}
