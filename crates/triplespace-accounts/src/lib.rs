//! Accounts (0007 §3; 0016 §3; 0024 §2, §4; 0013 §4): the credentials an instance mints
//! and checks, the sessions they open, and what a request may do.
//!
//! - [`password`]: the built-in `password` issuer's hashes in `private.password`
//!   (Argon2id), and the binding record that pairs with one.
//! - [`keys`]: subsidiary API keys in `private.api_key`: issued once as
//!   `{key ID}.{secret}`, stored as a hash, checked as a bearer credential or through the
//!   bot-password login.
//! - [`session`]: `private.session`, the session store of an instance without a shared
//!   cache (0014 Q5, closed by A-amendment): a cookie value, the tenant, the actor and the
//!   key a bot-password login used.
//! - [`login`]: `action=login` for subsidiaries and `action=clientlogin` for primary
//!   accounts, with the refusals of 0024 §8.
//! - [`token`]: CSRF and login tokens derived from the session (or, for a stateless
//!   bearer request, from the key) with the instance secret.
//! - [`identity`]: who a request is, from its cookie or bearer header, and the
//!   [`scatter_actors::evaluate::Effective`] permissions that follow.
//!
//! This is one of the three crates that read `private` (0013 §4). Everything here takes a
//! connection and runs inside whatever transaction the caller holds; nothing opens one.
//! Actor keys are tenant-qualified throughout (`librarybase:7`), as `view.actor` writes
//! them.

#![forbid(unsafe_code)]

pub mod identity;
pub mod keys;
pub mod login;
pub mod password;
pub mod session;
pub mod token;

pub use identity::{Auth, Identity, Resolved};
pub use keys::{Issued, KeyAuth, KeyInfo};
pub use login::LoginError;
pub use session::Session;
pub use token::Secret;

/// Why an accounts operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AccountsError {
    /// The query failed.
    #[error("accounts: {0}")]
    Sql(#[from] tokio_postgres::Error),
    /// The hash could not be computed or parsed.
    #[error("password hash: {0}")]
    Hash(String),
    /// The actor key is not one.
    #[error("{0}")]
    ActorKey(#[from] scatter_actors::key::ActorKeyError),
    /// A label, name or grant is not acceptable.
    #[error("{0}")]
    Invalid(String),
    /// No such account.
    #[error("no account `{0}`")]
    NoAccount(String),
    /// The random source failed.
    #[error("random: {0}")]
    Random(String),
}

/// `n` random bytes, URL-safe base64 without padding.
pub(crate) fn random_token(n: usize) -> Result<String, AccountsError> {
    use base64::Engine as _;
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| AccountsError::Random(e.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Microseconds since the epoch, now.
pub(crate) fn now_micros() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}
