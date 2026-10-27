//! The optional bot password a follower logs in with upstream (0092 §10), one per follower
//! in `private.upstream_login`, keyed as its position is: `(tenant, kind, name)`. The
//! secret is sealed under the instance sealing key, as a client credential's is
//! ([`crate::oauth`]), and never leaves the instance (0027 §2).
//!
//! A login is a MediaWiki bot password, `User@label`, and needs only the `basic` and
//! `highvolume` grants: a follower never writes upstream.

use tokio_postgres::GenericClient;

use crate::{AccountsError, Secret, seal};

const SEAL_TABLE: &str = "private.upstream_login";

/// Which follower a login is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follower {
    /// `''` for the instance's followers, else the tenant.
    pub tenant: String,
    /// `repo`, `provider` or `source`.
    pub kind: String,
    /// The repository, the provider slug or the source name.
    pub name: String,
}

impl Follower {
    /// A registry provider's follower.
    #[must_use]
    pub fn provider(slug: &str) -> Self {
        Follower {
            tenant: String::new(),
            kind: "provider".into(),
            name: slug.into(),
        }
    }

    /// A tenant's entity source's follower.
    #[must_use]
    pub fn source(tenant: &str, name: &str) -> Self {
        Follower {
            tenant: tenant.into(),
            kind: "source".into(),
            name: name.into(),
        }
    }

    /// A page repository's follower.
    #[must_use]
    pub fn repo(name: &str) -> Self {
        Follower {
            tenant: String::new(),
            kind: "repo".into(),
            name: name.into(),
        }
    }
}

/// A login, opened.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    /// `User@label`.
    pub username: String,
    /// The bot password.
    pub password: String,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

/// Stores (or replaces) a follower's login, sealing the password.
///
/// # Errors
///
/// [`AccountsError::Invalid`] for an empty name or password; otherwise a store or
/// random-source error.
pub async fn set<C: GenericClient>(
    client: &C,
    secret: &Secret,
    who: &Follower,
    username: &str,
    password: &str,
) -> Result<(), AccountsError> {
    let (username, password) = (username.trim(), password.trim());
    if username.is_empty() || password.is_empty() {
        return Err(AccountsError::Invalid(
            "a login name and a password are both needed".into(),
        ));
    }
    let sealed = seal::seal(
        secret,
        &[SEAL_TABLE, &who.tenant, &who.kind, &who.name],
        password.as_bytes(),
    )?;
    client
        .execute(
            "INSERT INTO private.upstream_login (tenant, kind, name, username, secret)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant, kind, name) DO UPDATE SET username = EXCLUDED.username,
               secret = EXCLUDED.secret, updated = now()",
            &[&who.tenant, &who.kind, &who.name, &username, &sealed],
        )
        .await?;
    Ok(())
}

/// Removes a follower's login. `false` when there was none.
///
/// # Errors
///
/// If the store fails.
pub async fn unset<C: GenericClient>(client: &C, who: &Follower) -> Result<bool, AccountsError> {
    Ok(client
        .execute(
            "DELETE FROM private.upstream_login WHERE tenant = $1 AND kind = $2 AND name = $3",
            &[&who.tenant, &who.kind, &who.name],
        )
        .await?
        == 1)
}

/// A follower's login, opened; `None` when it has none.
///
/// # Errors
///
/// If the store fails or the sealed value does not open (the sealing key changed).
pub async fn get<C: GenericClient>(
    client: &C,
    secret: &Secret,
    who: &Follower,
) -> Result<Option<Login>, AccountsError> {
    let Some(row) = client
        .query_opt(
            "SELECT username, secret FROM private.upstream_login
             WHERE tenant = $1 AND kind = $2 AND name = $3",
            &[&who.tenant, &who.kind, &who.name],
        )
        .await?
    else {
        return Ok(None);
    };
    let sealed: Vec<u8> = row.get(1);
    let opened = seal::open(
        secret,
        &[SEAL_TABLE, &who.tenant, &who.kind, &who.name],
        &sealed,
    )?;
    Ok(Some(Login {
        username: row.get(0),
        password: String::from_utf8(opened).map_err(|_| AccountsError::Sealed)?,
    }))
}
