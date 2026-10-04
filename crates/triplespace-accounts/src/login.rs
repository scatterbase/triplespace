//! Logging in (0024 §4, §8; 0012 §4): `action=login` is the bot-password form for
//! subsidiaries, `lgname={subsidiary name}@{label}`; `action=clientlogin` is the
//! username-and-password form for primary accounts with a binding to the built-in
//! `password` issuer. Neither is offered to the other kind of account.

use scatter_actors::actor::{ActorKind, ActorStatus, normalize_name};
use tokio_postgres::GenericClient;

use crate::keys::{KeyAuth, authenticate_label};
use crate::session::{self, Session};
use crate::{AccountsError, password};

/// Why a login was refused. The Action API maps these to `login.result` and
/// `clientlogin.status` (0024 §8).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoginError {
    /// A primary account's name was given to the bot-password form: `ts-use-oauth`.
    #[error(
        "`{0}` is a primary account; log in with action=clientlogin, or with a bot password of one of its subsidiaries"
    )]
    UseOauth(String),
    /// A subsidiary's name was given to the password form.
    #[error("`{0}` is a subsidiary account; log in with action=login and a bot password")]
    SubsidiaryPassword(String),
    /// No such name, wrong label, wrong secret or password, or an inactive account: one
    /// answer for all of them.
    #[error("Failed")]
    Failed,
    /// The store failed.
    #[error(transparent)]
    Accounts(#[from] AccountsError),
}

/// An account by name on a tenant.
async fn by_name<C: GenericClient>(
    client: &C,
    tenant: &str,
    name: &str,
) -> Result<Option<(String, ActorKind, ActorStatus)>, AccountsError> {
    let name = normalize_name(name);
    let row = client
        .query_opt(
            "SELECT actor_key, kind, status FROM view.actor WHERE tenant = $1 AND name = $2",
            &[&tenant, &name],
        )
        .await?;
    Ok(row.map(|r| {
        let kind: Option<ActorKind> =
            serde_json::from_value(serde_json::Value::String(r.get(1))).ok();
        let status: Option<ActorStatus> =
            serde_json::from_value(serde_json::Value::String(r.get(2))).ok();
        (
            r.get(0),
            kind.unwrap_or(ActorKind::Registered),
            status.unwrap_or_default(),
        )
    }))
}

/// The bot-password form: checks `{name}@{label}` and the secret, and opens a session
/// that records the key (0024 §4). The anonymous session the login token came from, if
/// any, is ended.
pub async fn bot_login<C: GenericClient>(
    client: &C,
    tenant: &str,
    lgname: &str,
    lgpassword: &str,
    previous_session: Option<&str>,
) -> Result<(Session, KeyAuth), LoginError> {
    let Some((name, label)) = lgname.rsplit_once('@') else {
        // No label: either a primary account's name (ts-use-oauth) or nonsense.
        return match by_name(client, tenant, lgname).await? {
            Some((_, ActorKind::Bot, _)) | None => Err(LoginError::Failed),
            Some(_) => Err(LoginError::UseOauth(lgname.to_string())),
        };
    };
    let Some((actor_key, kind, status)) = by_name(client, tenant, name).await? else {
        return Err(LoginError::Failed);
    };
    if kind != ActorKind::Bot {
        return Err(LoginError::UseOauth(name.to_string()));
    }
    if status != ActorStatus::Active {
        return Err(LoginError::Failed);
    }
    let Some(key) = authenticate_label(client, &actor_key, label, lgpassword).await? else {
        return Err(LoginError::Failed);
    };
    if let Some(prev) = previous_session {
        session::delete(client, prev).await?;
    }
    session::sweep(client).await?;
    let s = session::create(client, tenant, Some(&actor_key), Some(&key.key_id)).await?;
    Ok((s, key))
}

/// The password form for primary accounts (0007 §3 as amended): checks the name and
/// password against `private.password` and opens a session.
pub async fn password_login<C: GenericClient>(
    client: &C,
    tenant: &str,
    username: &str,
    password_text: &str,
    previous_session: Option<&str>,
) -> Result<Session, LoginError> {
    let Some((actor_key, kind, status)) = by_name(client, tenant, username).await? else {
        return Err(LoginError::Failed);
    };
    if kind == ActorKind::Bot {
        return Err(LoginError::SubsidiaryPassword(username.to_string()));
    }
    if status != ActorStatus::Active || !password::check(client, &actor_key, password_text).await? {
        return Err(LoginError::Failed);
    }
    if let Some(prev) = previous_session {
        session::delete(client, prev).await?;
    }
    session::sweep(client).await?;
    Ok(session::create(client, tenant, Some(&actor_key), None).await?)
}
