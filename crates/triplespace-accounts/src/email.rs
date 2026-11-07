//! A login's email addresses (0100 §1–§4, §8; 07 §2.9): `private.email`, with the
//! address grammar, the six-digit verification codes and their limits, the primary, and
//! the rules for removing an address. Sending the messages is `triplespace-notify`'s;
//! this module says what to send and records what was done.
//!
//! **The login.** Addresses are keyed by the login (0089 §2). Until instance logins are
//! built, the identifier stored in `login` is the actor key of the account whose session
//! acts, which on a single-tenant instance is one login per person (0100 A1).
//!
//! **Codes.** Six decimal digits drawn uniformly from the operating system's random
//! source, valid for an hour, spent by five wrong entries. Only an HMAC-SHA256 of the code
//! is stored, keyed from the instance secret under its own domain separator and taken over
//! the login's ID, the address key and the code; the comparison is constant-time.
//!
//! **Limits.** Once a minute per address, five times a day per login, and five times a day
//! per address across the instance, counted in `private.rate_event` under the classes
//! [`RATE_LOGIN`] and [`RATE_ADDRESS`] with the empty tenant, as the `badlogin` counters
//! are where there is no shared cache (07 §6.3).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac as _};
use sha2::Sha256;
use tokio_postgres::GenericClient;

use crate::AccountsError;
use crate::challenge;
use crate::token::Secret;

/// How long a code is valid, in seconds.
pub const CODE_LIFETIME: u64 = 3600;
/// How many wrong entries spend a code.
pub const MAX_WRONG: i16 = 5;
/// The least time between two codes to one address, in seconds.
pub const PER_ADDRESS_INTERVAL: u64 = 60;
/// Codes a login may have sent in a day.
pub const PER_LOGIN_DAILY: i64 = 5;
/// Codes an address may receive in a day, from every login.
pub const PER_ADDRESS_DAILY: i64 = 5;
/// A day, the window of the daily limits.
const DAY: u64 = 86_400;
/// The rate-limit class counting codes sent per login; the key is the login.
pub const RATE_LOGIN: &str = "email-login";
/// The rate-limit class counting codes sent per address; the key is the address key's
/// hash.
pub const RATE_ADDRESS: &str = "email-to";
/// The longest an address may be, in octets.
const MAX_LENGTH: usize = 254;

/// Why an operation on an address was refused (07 §2.9).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EmailError {
    /// `ts-email-invalid`: not an `addr-spec` with a dotted domain.
    #[error("ts-email-invalid")]
    Invalid,
    /// `ts-email-limit`: the login holds `email.max_addresses` already.
    #[error("ts-email-limit")]
    Limit,
    /// `ts-email-code`: wrong, expired or spent, with the entries left.
    #[error("ts-email-code")]
    Code {
        /// How many wrong entries the code survives still; 0 when it is spent.
        left: i16,
    },
    /// `ts-email-unverified`: an unverified address cannot be the primary.
    #[error("ts-email-unverified")]
    Unverified,
    /// `ts-email-taken`: another login verified the address first.
    #[error("ts-email-taken")]
    Taken,
    /// `ts-email-primary`: the primary cannot be removed while another address is verified.
    #[error("ts-email-primary")]
    Primary,
    /// `ts-email-last`: the last verified address can go only with a password and no
    /// external sign-in method.
    #[error("ts-email-last")]
    Last,
    /// `ts-email-required`: the instance requires an address.
    #[error("ts-email-required")]
    Required,
    /// `ts-email-disabled`: the instance sends no mail.
    #[error("ts-email-disabled")]
    Disabled,
    /// `ratelimited`, with the seconds to wait.
    #[error("ratelimited")]
    RateLimited {
        /// Seconds until the next code may be sent.
        wait: u64,
    },
    /// The login holds no such address.
    #[error("ts-email-unknown")]
    Unknown,
    /// The store failed.
    #[error(transparent)]
    Accounts(#[from] AccountsError),
}

impl PartialEq for EmailError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Code { left: a }, Self::Code { left: b }) => a == b,
            (Self::RateLimited { wait: a }, Self::RateLimited { wait: b }) => a == b,
            (Self::Accounts(a), Self::Accounts(b)) => a.to_string() == b.to_string(),
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}

impl From<tokio_postgres::Error> for EmailError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Accounts(e.into())
    }
}

impl EmailError {
    /// The error code the API answers with.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid => "ts-email-invalid",
            Self::Limit => "ts-email-limit",
            Self::Code { .. } => "ts-email-code",
            Self::Unverified => "ts-email-unverified",
            Self::Taken => "ts-email-taken",
            Self::Primary => "ts-email-primary",
            Self::Last => "ts-email-last",
            Self::Required => "ts-email-required",
            Self::Disabled => "ts-email-disabled",
            Self::RateLimited { .. } => "ratelimited",
            Self::Unknown => "ts-email-unknown",
            Self::Accounts(_) => "internal",
        }
    }
}

/// Why an address cannot be delivered to (0100 A2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undeliverable {
    /// A hard bounce.
    Bounce,
    /// A spam complaint.
    Complaint,
}

impl Undeliverable {
    /// The word stored.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bounce => "bounce",
            Self::Complaint => "complaint",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "bounce" => Some(Self::Bounce),
            "complaint" => Some(Self::Complaint),
            _ => None,
        }
    }
}

/// One address of a login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// As written.
    pub address: String,
    /// Lower-cased: how it compares.
    pub address_key: String,
    /// When it was verified, if it was.
    pub verified: Option<SystemTime>,
    /// Whether it is the login's primary.
    pub primary: bool,
    /// When it was added.
    pub added: SystemTime,
    /// When its current code expires, for an unverified address with a live code.
    pub code_expires: Option<SystemTime>,
    /// Whether it took a hard bounce or a complaint, and when.
    pub undeliverable: Option<(Undeliverable, SystemTime)>,
}

impl Address {
    /// Whether the address is verified.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        self.verified.is_some()
    }
}

const COLUMNS: &str = "address, address_key, verified, is_primary, added, code_expires, code_mac IS NOT NULL, undeliverable, undeliverable_reason";

fn address_of(r: &tokio_postgres::Row) -> Address {
    let code_expires: Option<SystemTime> = r.get(5);
    let has_code: bool = r.get(6);
    let undeliverable: Option<SystemTime> = r.get(7);
    let reason: Option<String> = r.get(8);
    Address {
        address: r.get(0),
        address_key: r.get(1),
        verified: r.get(2),
        primary: r.get(3),
        added: r.get(4),
        code_expires: code_expires.filter(|_| has_code),
        undeliverable: undeliverable
            .zip(reason.as_deref().and_then(Undeliverable::parse))
            .map(|(t, r)| (r, t)),
    }
}

/// Parses an address as 07 §2.9 accepts it: an RFC 5322 `addr-spec` with no display name
/// or comment, at most 254 octets, with a domain that has at least one dot. Returns the
/// address as it is stored and sent, with the domain in A-labels, and its key.
///
/// # Errors
///
/// [`EmailError::Invalid`].
pub fn parse(text: &str) -> Result<(String, String), EmailError> {
    let text = text.trim();
    let (local, domain) = text.rsplit_once('@').ok_or(EmailError::Invalid)?;
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local.chars().all(|c| {
            c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c) || !c.is_ascii()
        })
        || local.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(EmailError::Invalid);
    }
    if domain.is_empty() || domain.starts_with('[') {
        return Err(EmailError::Invalid);
    }
    let ascii = idna::domain_to_ascii(domain).map_err(|_| EmailError::Invalid)?;
    if !ascii.contains('.')
        || ascii.starts_with('.')
        || ascii.ends_with('.')
        || ascii.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
    {
        return Err(EmailError::Invalid);
    }
    let address = format!("{local}@{ascii}");
    if address.len() > MAX_LENGTH {
        return Err(EmailError::Invalid);
    }
    let key = address.to_lowercase();
    Ok((address, key))
}

/// What is typed as a code, with spaces and hyphens ignored: six digits, or nothing.
#[must_use]
pub fn normalize_code(typed: &str) -> Option<String> {
    let digits: String = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// A fresh code: six decimal digits, uniform, leading zeros kept.
///
/// # Errors
///
/// If the operating system's random source fails.
pub fn new_code() -> Result<String, AccountsError> {
    loop {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).map_err(|e| AccountsError::Random(e.to_string()))?;
        let n = u32::from_be_bytes(bytes);
        // Rejection sampling: the largest multiple of 1,000,000 below 2^32.
        if n < 4_294_000_000 {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

/// The code as the message prints it: `123 456`.
#[must_use]
pub fn display_code(code: &str) -> String {
    match code.len() {
        6 => format!("{} {}", &code[..3], &code[3..]),
        _ => code.to_string(),
    }
}

/// The MAC of a code for a login and address, hex.
fn code_mac(secret: &Secret, login: &str, address_key: &str, code: &str) -> String {
    let key = secret.subkey("triplespace-accounts:email-code:v1:");
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("any key length");
    mac.update(login.as_bytes());
    mac.update(b"\0");
    mac.update(address_key.as_bytes());
    mac.update(b"\0");
    mac.update(code.as_bytes());
    hex(&mac.finalize().into_bytes())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// The key an address is counted under, across the instance: a hash, so the rate table
/// holds no address.
fn address_rate_key(secret: &Secret, address_key: &str) -> String {
    let key = secret.subkey("triplespace-accounts:email-rate:v1:");
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("any key length");
    mac.update(address_key.as_bytes());
    hex(&mac.finalize().into_bytes()[..16])
}

/// The login's addresses, the primary first, then by when they were added.
///
/// # Errors
///
/// If the store fails.
pub async fn list<C: GenericClient>(client: &C, login: &str) -> Result<Vec<Address>, EmailError> {
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM private.email WHERE login = $1 ORDER BY is_primary DESC, added, address_key"
            ),
            &[&login],
        )
        .await?;
    Ok(rows.iter().map(address_of).collect())
}

/// One address of the login, by what was typed.
///
/// # Errors
///
/// [`EmailError::Invalid`] for text that is no address, [`EmailError::Unknown`] when the
/// login does not hold it.
pub async fn get<C: GenericClient>(
    client: &C,
    login: &str,
    text: &str,
) -> Result<Address, EmailError> {
    let (_, key) = parse(text)?;
    let row = client
        .query_opt(
            &format!("SELECT {COLUMNS} FROM private.email WHERE login = $1 AND address_key = $2"),
            &[&login, &key],
        )
        .await?;
    row.map(|r| address_of(&r)).ok_or(EmailError::Unknown)
}

/// The login's primary address, if it has one that can be delivered to.
///
/// # Errors
///
/// If the store fails.
pub async fn primary<C: GenericClient>(
    client: &C,
    login: &str,
) -> Result<Option<String>, EmailError> {
    let row = client
        .query_opt(
            "SELECT address FROM private.email WHERE login = $1 AND is_primary AND verified IS NOT NULL AND undeliverable IS NULL",
            &[&login],
        )
        .await?;
    Ok(row.map(|r| r.get(0)))
}

/// Every verified address of the login that can be delivered to: where the security
/// notices go.
///
/// # Errors
///
/// If the store fails.
pub async fn verified<C: GenericClient>(
    client: &C,
    login: &str,
) -> Result<Vec<String>, EmailError> {
    let rows = client
        .query(
            "SELECT address FROM private.email WHERE login = $1 AND verified IS NOT NULL AND undeliverable IS NULL ORDER BY is_primary DESC, added",
            &[&login],
        )
        .await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// What adding an address did: the message to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// A code was issued: send it, with when it expires.
    Code {
        /// The six digits.
        code: String,
        /// When the code stops working.
        expires: SystemTime,
    },
    /// Another login holds the address verified: send the message saying so, with no
    /// code. The requester is answered as for a fresh address.
    HeldElsewhere,
    /// The login holds the address verified already; nothing to send.
    AlreadyVerified,
}

/// Seconds since the epoch of a time, for the wait of a rate limit.
fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Checks the sending limits for a code to an address, and returns the wait otherwise.
async fn check_limits<C: GenericClient>(
    client: &C,
    secret: &Secret,
    login: &str,
    address_key: &str,
    last_sent: Option<SystemTime>,
) -> Result<(), EmailError> {
    if let Some(sent) = last_sent {
        let since = SystemTime::now()
            .duration_since(sent)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        if since < PER_ADDRESS_INTERVAL {
            return Err(EmailError::RateLimited {
                wait: PER_ADDRESS_INTERVAL - since,
            });
        }
    }
    let by_login = challenge::count(client, "", RATE_LOGIN, login, DAY).await?;
    let to_key = address_rate_key(secret, address_key);
    let by_address = challenge::count(client, "", RATE_ADDRESS, &to_key, DAY).await?;
    if by_login >= PER_LOGIN_DAILY || by_address >= PER_ADDRESS_DAILY {
        // The wait is until the oldest counted act leaves the window.
        let oldest = client
            .query_opt(
                "SELECT min(at) FROM private.rate_event WHERE tenant = '' AND ((class = $1 AND key = $2) OR (class = $3 AND key = $4))
                 AND at >= now() - make_interval(secs => $5::bigint)",
                &[
                    &RATE_LOGIN,
                    &login,
                    &RATE_ADDRESS,
                    &to_key,
                    &i64::try_from(DAY).unwrap_or(i64::MAX),
                ],
            )
            .await?
            .and_then(|r| r.get::<_, Option<SystemTime>>(0));
        let wait = oldest.map_or(DAY, |t| {
            (secs(t) + DAY).saturating_sub(secs(SystemTime::now()))
        });
        return Err(EmailError::RateLimited { wait: wait.max(1) });
    }
    Ok(())
}

async fn count_send<C: GenericClient>(
    client: &C,
    secret: &Secret,
    login: &str,
    address_key: &str,
) -> Result<(), EmailError> {
    challenge::record(client, "", RATE_LOGIN, login).await?;
    challenge::record(
        client,
        "",
        RATE_ADDRESS,
        &address_rate_key(secret, address_key),
    )
    .await?;
    Ok(())
}

/// Adds an address to the login and issues a code, or issues a new code to an address
/// the login holds unverified. `max_addresses` is `email.max_addresses`.
///
/// With [`verify`] this implements 0100 §2.
/// With [`make_primary`] and [`remove`] this implements 0100 §3.
/// Reading `email.max_addresses` and `email.required` implements 0100 §10 (A2).
/// The table and its indexes in `0026_email.sql` implements 0100 §8 (A2).
///
/// # Errors
///
/// [`EmailError::Invalid`], [`EmailError::Limit`], [`EmailError::RateLimited`], or the
/// store's.
pub async fn add<C: GenericClient>(
    client: &C,
    secret: &Secret,
    login: &str,
    text: &str,
    max_addresses: i64,
) -> Result<(Address, Added), EmailError> {
    let (address, key) = parse(text)?;
    let existing = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS}, code_sent FROM private.email WHERE login = $1 AND address_key = $2"
            ),
            &[&login, &key],
        )
        .await?;
    let last_sent: Option<SystemTime> = existing.as_ref().and_then(|r| r.get(9));
    if let Some(r) = &existing {
        let a = address_of(r);
        if a.is_verified() && a.undeliverable.is_none() {
            return Ok((a, Added::AlreadyVerified));
        }
    } else {
        let held: i64 = client
            .query_one(
                "SELECT count(*) FROM private.email WHERE login = $1",
                &[&login],
            )
            .await?
            .get(0);
        if held >= max_addresses {
            return Err(EmailError::Limit);
        }
    }
    check_limits(client, secret, login, &key, last_sent).await?;
    let elsewhere = client
        .query_opt(
            "SELECT 1 FROM private.email WHERE address_key = $1 AND login <> $2 AND verified IS NOT NULL",
            &[&key, &login],
        )
        .await?
        .is_some();
    let (mac, expires, added) = if elsewhere {
        (None, None, Added::HeldElsewhere)
    } else {
        let code = new_code()?;
        let expires = SystemTime::now() + Duration::from_secs(CODE_LIFETIME);
        (
            Some(code_mac(secret, login, &key, &code)),
            Some(expires),
            Added::Code { code, expires },
        )
    };
    // A verified address that bounced keeps its verification and primary role while the
    // holder confirms it again (0100 A2): only the code changes.
    client
        .execute(
            "INSERT INTO private.email (login, address, address_key, code_mac, code_expires, code_wrong, code_sent)
             VALUES ($1, $2, $3, $4, $5, 0, now())
             ON CONFLICT (login, address_key) DO UPDATE
               SET code_mac = EXCLUDED.code_mac, code_expires = EXCLUDED.code_expires,
                   code_wrong = 0, code_sent = now()",
            &[&login, &address, &key, &mac, &expires],
        )
        .await?;
    count_send(client, secret, login, &key).await?;
    let row = client
        .query_one(
            &format!("SELECT {COLUMNS} FROM private.email WHERE login = $1 AND address_key = $2"),
            &[&login, &key],
        )
        .await?;
    Ok((address_of(&row), added))
}

/// Enters a code for an address of the login (0100 §2): a correct one marks the address
/// verified, clears the code and any bounce, and makes it primary if the login has none.
/// Returns the address and whether it became the primary now.
///
/// # Errors
///
/// [`EmailError::Code`] for a wrong, expired or spent code, [`EmailError::Taken`] when
/// another login verified the address since, [`EmailError::Unknown`], or the store's.
pub async fn verify<C: GenericClient>(
    client: &C,
    secret: &Secret,
    login: &str,
    text: &str,
    typed: &str,
) -> Result<(Address, bool), EmailError> {
    let (_, key) = parse(text)?;
    let row = client
        .query_opt(
            "SELECT code_mac, code_expires, code_wrong, verified, undeliverable FROM private.email
             WHERE login = $1 AND address_key = $2 FOR UPDATE",
            &[&login, &key],
        )
        .await?
        .ok_or(EmailError::Unknown)?;
    let mac: Option<String> = row.get(0);
    let expires: Option<SystemTime> = row.get(1);
    let wrong: i16 = row.get(2);
    let was_verified: Option<SystemTime> = row.get(3);
    let (Some(mac), Some(expires)) = (mac, expires) else {
        return Err(EmailError::Code { left: 0 });
    };
    if expires <= SystemTime::now() {
        client
            .execute(
                "UPDATE private.email SET code_mac = NULL, code_expires = NULL WHERE login = $1 AND address_key = $2",
                &[&login, &key],
            )
            .await?;
        return Err(EmailError::Code { left: 0 });
    }
    let correct = normalize_code(typed)
        .is_some_and(|c| constant_eq(&code_mac(secret, login, &key, &c), &mac));
    if !correct {
        let wrong = wrong + 1;
        let left = (MAX_WRONG - wrong).max(0);
        if left == 0 {
            client
                .execute(
                    "UPDATE private.email SET code_mac = NULL, code_expires = NULL, code_wrong = $3 WHERE login = $1 AND address_key = $2",
                    &[&login, &key, &wrong],
                )
                .await?;
        } else {
            client
                .execute(
                    "UPDATE private.email SET code_wrong = $3 WHERE login = $1 AND address_key = $2",
                    &[&login, &key, &wrong],
                )
                .await?;
        }
        return Err(EmailError::Code { left });
    }
    let taken = client
        .query_opt(
            "SELECT 1 FROM private.email WHERE address_key = $1 AND login <> $2 AND verified IS NOT NULL",
            &[&key, &login],
        )
        .await?
        .is_some();
    if taken {
        return Err(EmailError::Taken);
    }
    let has_primary = client
        .query_opt(
            "SELECT 1 FROM private.email WHERE login = $1 AND is_primary",
            &[&login],
        )
        .await?
        .is_some();
    let becomes_primary = !has_primary;
    client
        .execute(
            "UPDATE private.email
               SET verified = COALESCE(verified, now()), code_mac = NULL, code_expires = NULL, code_wrong = 0,
                   is_primary = is_primary OR $3, undeliverable = NULL, undeliverable_reason = NULL
             WHERE login = $1 AND address_key = $2",
            &[&login, &key, &becomes_primary],
        )
        .await?;
    let row = client
        .query_one(
            &format!("SELECT {COLUMNS} FROM private.email WHERE login = $1 AND address_key = $2"),
            &[&login, &key],
        )
        .await?;
    // Confirming a bounced primary again is not a new verification.
    Ok((address_of(&row), becomes_primary && was_verified.is_none()))
}

/// Makes a verified address the login's primary (0100 §3).
///
/// # Errors
///
/// [`EmailError::Unverified`], [`EmailError::Unknown`], or the store's.
pub async fn make_primary<C: GenericClient>(
    client: &C,
    login: &str,
    text: &str,
) -> Result<Address, EmailError> {
    let a = get(client, login, text).await?;
    if !a.is_verified() {
        return Err(EmailError::Unverified);
    }
    if !a.primary {
        client
            .execute(
                "UPDATE private.email SET is_primary = false WHERE login = $1 AND is_primary",
                &[&login],
            )
            .await?;
        client
            .execute(
                "UPDATE private.email SET is_primary = true WHERE login = $1 AND address_key = $2",
                &[&login, &a.address_key],
            )
            .await?;
    }
    Ok(Address { primary: true, ..a })
}

/// What decides whether a login may be left without an address (0100 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemovalRules {
    /// `email.required`: the login's last verified address can never go.
    pub required: bool,
    /// Whether the login has a password.
    pub has_password: bool,
    /// Whether the login has an external sign-in method.
    pub has_external: bool,
}

/// Why the primary cannot be removed now, if it cannot: for the page's disabled button.
#[must_use]
pub fn primary_removal_blocker(addresses: &[Address], rules: RemovalRules) -> Option<EmailError> {
    let verified = addresses.iter().filter(|a| a.is_verified()).count();
    if verified > 1 {
        Some(EmailError::Primary)
    } else if rules.required {
        Some(EmailError::Required)
    } else if !rules.has_password || rules.has_external {
        Some(EmailError::Last)
    } else {
        None
    }
}

/// Removes an address (0100 §3). Returns the address removed, so that the security
/// notices can name it and reach it.
///
/// # Errors
///
/// [`EmailError::Primary`], [`EmailError::Last`], [`EmailError::Required`],
/// [`EmailError::Unknown`], or the store's.
pub async fn remove<C: GenericClient>(
    client: &C,
    login: &str,
    text: &str,
    rules: RemovalRules,
) -> Result<Address, EmailError> {
    let a = get(client, login, text).await?;
    if a.primary {
        let all = list(client, login).await?;
        if let Some(e) = primary_removal_blocker(&all, rules) {
            return Err(e);
        }
    }
    client
        .execute(
            "DELETE FROM private.email WHERE login = $1 AND address_key = $2",
            &[&login, &a.address_key],
        )
        .await?;
    Ok(a)
}

/// Marks every row holding an address undeliverable after a hard bounce or a complaint
/// (0100 A2): nothing more is sent to it until its holder confirms it by code again.
/// Returns the logins affected.
///
/// # Errors
///
/// If the store fails.
pub async fn mark_undeliverable<C: GenericClient>(
    client: &C,
    text: &str,
    reason: Undeliverable,
) -> Result<Vec<String>, EmailError> {
    let (_, key) = parse(text)?;
    let rows = client
        .query(
            "UPDATE private.email SET undeliverable = now(), undeliverable_reason = $2
             WHERE address_key = $1 AND verified IS NOT NULL RETURNING login",
            &[&key, &reason.as_str()],
        )
        .await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// Whether the login holds a verified address that can be delivered to.
///
/// # Errors
///
/// If the store fails.
pub async fn has_verified<C: GenericClient>(client: &C, login: &str) -> Result<bool, EmailError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM private.email WHERE login = $1 AND verified IS NOT NULL",
            &[&login],
        )
        .await?
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 07 §2.9's grammar: an `addr-spec` with a dotted domain, at most 254 octets, the
    /// domain in A-labels, compared lower-cased.
    #[test]
    fn addresses_parse() {
        assert_eq!(
            parse("Alice.Smith+wiki@Example.ORG").unwrap(),
            (
                "Alice.Smith+wiki@example.org".to_string(),
                "alice.smith+wiki@example.org".to_string()
            )
        );
        assert_eq!(
            parse("  bob@bücher.example ").unwrap().0,
            "bob@xn--bcher-kva.example"
        );
        for bad in [
            "",
            "alice",
            "alice@",
            "@example.org",
            "alice@localhost",
            "Alice <alice@example.org>",
            "alice@example.org (comment)",
            "al ice@example.org",
            "alice@-bad.example",
            "alice@[127.0.0.1]",
            ".alice@example.org",
            "a..b@example.org",
            "\"quoted\"@example.org",
        ] {
            assert_eq!(parse(bad), Err(EmailError::Invalid), "{bad:?}");
        }
        let long = format!("{}@example.org", "a".repeat(64));
        assert!(parse(&long).is_ok());
        let too_long = format!("{}@{}.example.org", "a".repeat(64), "b".repeat(190));
        assert_eq!(parse(&too_long), Err(EmailError::Invalid));
    }

    /// Codes are six digits with leading zeros kept, printed in two groups, and read with
    /// spaces and hyphens ignored.
    #[test]
    fn codes() {
        for _ in 0..50 {
            let c = new_code().unwrap();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(display_code("012345"), "012 345");
        assert_eq!(normalize_code(" 012 345 "), Some("012345".into()));
        assert_eq!(normalize_code("012-345"), Some("012345".into()));
        assert_eq!(normalize_code("12345"), None);
        assert_eq!(normalize_code("0123456"), None);
        assert_eq!(normalize_code("01234a"), None);
    }

    /// The MAC is over the login, the address key and the code, so the same code for
    /// another address or login does not match.
    #[test]
    fn macs_bind_login_and_address() {
        let s = Secret::from_bytes([7; 32]);
        let a = code_mac(&s, "lb:1", "alice@example.org", "123456");
        assert_eq!(a, code_mac(&s, "lb:1", "alice@example.org", "123456"));
        assert_ne!(a, code_mac(&s, "lb:2", "alice@example.org", "123456"));
        assert_ne!(a, code_mac(&s, "lb:1", "bob@example.org", "123456"));
        assert_ne!(a, code_mac(&s, "lb:1", "alice@example.org", "123457"));
        assert_eq!(a.len(), 64);
        assert!(constant_eq(&a, &a.clone()));
        assert!(!constant_eq(&a, "x"));
    }

    fn addr(verified: bool, primary: bool) -> Address {
        Address {
            address: "a@example.org".into(),
            address_key: "a@example.org".into(),
            verified: verified.then_some(UNIX_EPOCH),
            primary,
            added: UNIX_EPOCH,
            code_expires: None,
            undeliverable: None,
        }
    }

    /// 0100 §3: the primary goes only when no other address is verified, the instance does
    /// not require one, the login has a password and no external method.
    #[test]
    fn primary_removal_rules() {
        let two = [addr(true, true), addr(true, false)];
        assert_eq!(
            primary_removal_blocker(&two, RemovalRules::default()),
            Some(EmailError::Primary)
        );
        let one = [addr(true, true), addr(false, false)];
        let ok = RemovalRules {
            required: false,
            has_password: true,
            has_external: false,
        };
        assert_eq!(primary_removal_blocker(&one, ok), None);
        assert_eq!(
            primary_removal_blocker(
                &one,
                RemovalRules {
                    required: true,
                    ..ok
                }
            ),
            Some(EmailError::Required)
        );
        assert_eq!(
            primary_removal_blocker(
                &one,
                RemovalRules {
                    has_password: false,
                    ..ok
                }
            ),
            Some(EmailError::Last)
        );
        assert_eq!(
            primary_removal_blocker(
                &one,
                RemovalRules {
                    has_external: true,
                    ..ok
                }
            ),
            Some(EmailError::Last)
        );
    }
}
