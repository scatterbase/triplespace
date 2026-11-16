//! A tenant owner's invitation (0111 §5, A2; 08 §1.4): when a tenant request named no
//! login for the new owner account, a holder of `ts-createtenant` asks for an invitation,
//! and its holder chooses a password on the new tenant's `Special:CreateAccount`, which
//! creates a login for the owner account. The code is 32 random bytes, URL-safe base64,
//! shown once; `private.tenant_invitation` keeps its SHA-256, the account, who asked, and
//! when it expires, seven days on. One per account: asking again replaces an unused one.

use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use crate::AccountsError;

/// How long an invitation is valid.
pub const VALID_DAYS: i32 = 7;

fn hash(code: &str) -> Vec<u8> {
    Sha256::digest(code.trim().as_bytes()).to_vec()
}

/// Issues an invitation for `actor_key` on `tenant`, replacing any the account has, and
/// returns its code, which is not kept.
///
/// # Errors
///
/// The store's, or the random source's.
pub async fn issue<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
    created_by: &str,
) -> Result<String, AccountsError> {
    let code = crate::random_token(32)?;
    client
        .execute(
            "INSERT INTO private.tenant_invitation (tenant, actor_key, code_hash, expires, created_by)
             VALUES ($1, $2, $3, now() + make_interval(days => $4), $5)
             ON CONFLICT (tenant, actor_key) DO UPDATE SET
               code_hash = EXCLUDED.code_hash, created = now(), expires = EXCLUDED.expires,
               created_by = EXCLUDED.created_by, used = NULL",
            &[&tenant, &actor_key, &hash(&code), &VALID_DAYS, &created_by],
        )
        .await?;
    Ok(code)
}

/// An open invitation: the account it is for, and when it expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    /// The owner account's actor key.
    pub actor_key: String,
    /// When it expires, microseconds since the epoch.
    pub expires: u64,
}

/// The open invitation `code` names on `tenant`: unused and unexpired. With `lock`, the
/// row is locked for the caller's transaction, which redeems it.
///
/// # Errors
///
/// The store's.
pub async fn open<C: GenericClient>(
    client: &C,
    tenant: &str,
    code: &str,
    lock: bool,
) -> Result<Option<Open>, AccountsError> {
    let sql = format!(
        "SELECT actor_key, (extract(epoch FROM expires) * 1000000)::bigint
         FROM private.tenant_invitation
         WHERE tenant = $1 AND code_hash = $2 AND used IS NULL AND expires > now(){}",
        if lock { " FOR UPDATE" } else { "" }
    );
    Ok(client
        .query_opt(&sql, &[&tenant, &hash(code)])
        .await?
        .map(|r| Open {
            actor_key: r.get(0),
            expires: u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
        }))
}

/// Marks the invitation for `actor_key` on `tenant` used.
///
/// # Errors
///
/// The store's.
pub async fn spend<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
) -> Result<(), AccountsError> {
    client
        .execute(
            "UPDATE private.tenant_invitation SET used = now() WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &actor_key],
        )
        .await?;
    Ok(())
}
