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
//! - [`challenge`]: issuing and redeeming the proof-of-work challenges of registration
//!   and login, the address key, and the `createaccount` and `badlogin` counts
//!   (0087 §2–4).
//! - [`requests`]: `private.account_request`, the requests for an account that wait for
//!   review where a tenant reviews registrations (0087 §6).
//! - [`forwarder`]: forwarder keys in `private.forwarder_key`, which let a proxy vouch
//!   for the client address it forwards (0057 §10).
//! - [`settings`]: the settings registry, the resolver of a setting's value and the one
//!   validator every writer of a `site` record calls (0085 §1–3).
//! - [`preferences`]: the preference registry, `private.preference`, and a preference's
//!   resolution through negotiation and the tenant's and instance's defaults (0027 §1;
//!   0085 §2.2).
//! - [`theme`]: the palettes and type sets a tenant offers and the `theme.*` settings
//!   that choose from them (0086 §1–4).
//!
//! This is one of the three crates that read `private` (0013 §4). Everything here takes a
//! connection and runs inside whatever transaction the caller holds; nothing opens one.
//! Actor keys are tenant-qualified throughout (`librarybase:7`), as `view.actor` writes
//! them.

#![forbid(unsafe_code)]

pub mod challenge;
pub mod forwarder;
pub mod identity;
pub mod keys;
pub mod login;
pub mod password;
pub mod preferences;
pub mod requests;
pub mod session;
pub mod settings;
pub mod theme;
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
