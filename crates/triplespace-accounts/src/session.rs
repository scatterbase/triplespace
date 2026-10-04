//! Sessions in `private.session` (0024 §4; 0014 §2 as amended): the store an instance
//! uses when it has no shared cache. The cookie value is the row's key, 32 random bytes;
//! an anonymous session exists so that a login token can be bound to something before the
//! login (as MediaWiki's do); a login replaces it with a fresh session, so a session ID
//! never survives the change of identity.

use std::time::{Duration, SystemTime};

use tokio_postgres::GenericClient;

use crate::{AccountsError, random_token};

/// The cookie that carries the session ID.
pub const COOKIE: &str = "triplespace_session";

/// How long an anonymous session lasts: long enough to fetch a token and log in.
pub const ANONYMOUS_LIFETIME: Duration = Duration::from_hours(1);
/// How long a logged-in session lasts without use.
pub const LIFETIME: Duration = Duration::from_hours(30 * 24);

/// A session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The cookie value.
    pub id: String,
    /// The tenant it was opened on.
    pub tenant: String,
    /// The account, or `None` for an anonymous session.
    pub actor_key: Option<String>,
    /// The API key a bot-password login used.
    pub key_id: Option<String>,
    /// When it expires.
    pub expires: SystemTime,
}

impl Session {
    /// Whether someone is logged in.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.actor_key.is_some()
    }
}

/// Opens a session.
pub async fn create<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: Option<&str>,
    key_id: Option<&str>,
) -> Result<Session, AccountsError> {
    let id = random_token(32)?;
    let lifetime = if actor_key.is_some() {
        LIFETIME
    } else {
        ANONYMOUS_LIFETIME
    };
    let expires = SystemTime::now() + lifetime;
    client
        .execute(
            "INSERT INTO private.session (id, tenant, actor_key, key_id, expires) VALUES ($1, $2, $3, $4, $5)",
            &[&id, &tenant, &actor_key, &key_id, &expires],
        )
        .await?;
    Ok(Session {
        id,
        tenant: tenant.to_string(),
        actor_key: actor_key.map(str::to_owned),
        key_id: key_id.map(str::to_owned),
        expires,
    })
}

/// Loads a live session by cookie value, noting the use. `None` for an unknown, expired
/// or foreign-tenant session.
pub async fn load<C: GenericClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Option<Session>, AccountsError> {
    let row = client
        .query_opt(
            "UPDATE private.session SET last_seen = now()
             WHERE id = $1 AND tenant = $2 AND expires > now()
             RETURNING actor_key, key_id, expires",
            &[&id, &tenant],
        )
        .await?;
    Ok(row.map(|r| Session {
        id: id.to_string(),
        tenant: tenant.to_string(),
        actor_key: r.get(0),
        key_id: r.get(1),
        expires: r.get(2),
    }))
}

/// Ends a session.
pub async fn delete<C: GenericClient>(client: &C, id: &str) -> Result<(), AccountsError> {
    client
        .execute("DELETE FROM private.session WHERE id = $1", &[&id])
        .await?;
    Ok(())
}

/// Drops expired sessions. Cheap enough to run on every login.
pub async fn sweep<C: GenericClient>(client: &C) -> Result<u64, AccountsError> {
    Ok(client
        .execute("DELETE FROM private.session WHERE expires <= now()", &[])
        .await?)
}
