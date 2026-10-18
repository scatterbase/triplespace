//! Challenges and the counts they rise with (0087 §2–4, A1; 07 §2.6, §6.3): issuing a
//! challenge to a session, checking a solution and spending it, the address key, and the
//! `createaccount` and `badlogin` counts, kept in `private` where there is no shared
//! cache.
//!
//! The work itself, the string and its MAC are `scatter-challenge`'s. The MAC key is
//! derived from the instance secret with its own domain separator, as the token key is.

use std::net::IpAddr;

use scatter_challenge::{Challenge, LIFETIME_SECS, Refusal, Trigger};
use tokio_postgres::GenericClient;

use crate::AccountsError;
use crate::token::Secret;

/// The `createaccount` rate-limit class: accounts created and requests filed, by
/// address key, 6 a day for every group (0087 §4; 07 §6.2).
pub const CREATEACCOUNT: &str = "createaccount";
/// Failed `password` logins by address key: a challenge after 3 within 5 minutes.
pub const BADLOGIN_ADDRESS: &str = "badlogin-address";
/// Failed `password` logins by account: a challenge after 20 within 10 minutes.
pub const BADLOGIN_ACCOUNT: &str = "badlogin-account";

/// The `createaccount` limit and window.
pub const CREATEACCOUNT_LIMIT: (i64, u64) = (6, 86_400);
/// The `badlogin` limit and window by address key, ConfirmEdit's defaults.
pub const BADLOGIN_ADDRESS_LIMIT: (i64, u64) = (3, 300);
/// The `badlogin` limit and window by account.
pub const BADLOGIN_ACCOUNT_LIMIT: (i64, u64) = (20, 600);

/// The longest window, after which a counted act is swept.
const LONGEST_WINDOW: u64 = 86_400;

/// The key a request is counted under: an IPv4 address, or the /64 prefix of an IPv6
/// address (0087 §4), written `2001:db8:1:2::/64`. An IPv4 address mapped into IPv6 is
/// the IPv4 address. A request whose address is unknown is counted as `unknown`.
#[must_use]
pub fn address_key(ip: Option<IpAddr>) -> String {
    match ip.map(|ip| ip.to_canonical()) {
        Some(IpAddr::V4(v4)) => v4.to_string(),
        Some(IpAddr::V6(v6)) => {
            let s = v6.segments();
            let prefix = std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0);
            format!("{prefix}/64")
        }
        None => "unknown".into(),
    }
}

/// The MAC key of challenges, from the instance secret.
fn key(secret: &Secret) -> [u8; 32] {
    secret.subkey("triplespace-accounts:challenge:v1:")
}

/// Seconds since the epoch, now.
fn now_secs() -> u64 {
    crate::now_micros() / 1_000_000
}

/// Issues a challenge for an act on a tenant to a session (or key) binding, at a
/// difficulty.
///
/// # Errors
///
/// If the random source fails.
pub fn issue(
    secret: &Secret,
    binding: &str,
    tenant: &str,
    trigger: Trigger,
    bits: u8,
) -> Result<Challenge, AccountsError> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| AccountsError::Random(e.to_string()))?;
    Ok(Challenge::issue(
        &key(secret),
        binding,
        tenant,
        trigger,
        bits.min(scatter_challenge::MAX_BITS),
        now_secs() + LIFETIME_SECS,
        salt,
    ))
}

/// Why a solution was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsolved {
    /// The client sent none.
    Missing,
    /// It was refused by the checks of 0087 §3.
    Refused(Refusal),
    /// It was accepted once already.
    Spent,
}

/// Checks a presented challenge and solution for an act and, if they pass, spends the
/// challenge, so that it is accepted once on every replica (0087 §3). A failure spends
/// nothing.
///
/// # Errors
///
/// `Ok(Err(_))` for a solution that is not accepted; `Err` if the store fails.
#[allow(clippy::too_many_arguments)]
pub async fn redeem<C: GenericClient>(
    client: &C,
    secret: &Secret,
    binding: &str,
    tenant: &str,
    trigger: Trigger,
    challenge: Option<&str>,
    solution: Option<&str>,
) -> Result<Result<(), Unsolved>, AccountsError> {
    let (Some(challenge), Some(solution)) = (
        challenge.filter(|s| !s.is_empty()),
        solution.filter(|s| !s.is_empty()),
    ) else {
        return Ok(Err(Unsolved::Missing));
    };
    let c = match Challenge::parse(challenge) {
        Ok(c) => c,
        Err(r) => return Ok(Err(Unsolved::Refused(r))),
    };
    if let Err(r) = c.check(&key(secret), binding, now_secs(), tenant, trigger, solution) {
        return Ok(Err(Unsolved::Refused(r)));
    }
    client
        .execute(
            "DELETE FROM private.challenge_spent WHERE expires < now()",
            &[],
        )
        .await?;
    let expires = i64::try_from(c.expires).unwrap_or(i64::MAX);
    let fresh = client
        .execute(
            "INSERT INTO private.challenge_spent (tenant, mac, expires)
             VALUES ($1, $2, to_timestamp($3::bigint))
             ON CONFLICT DO NOTHING",
            &[&tenant, &c.mac(), &expires],
        )
        .await?;
    Ok(if fresh == 1 {
        Ok(())
    } else {
        Err(Unsolved::Spent)
    })
}

/// Counts an act under a class and key, and sweeps what has left the longest window.
///
/// # Errors
///
/// If the store fails.
pub async fn record<C: GenericClient>(
    client: &C,
    tenant: &str,
    class: &str,
    key: &str,
) -> Result<(), AccountsError> {
    let longest = i64::try_from(LONGEST_WINDOW).unwrap_or(i64::MAX);
    client
        .execute(
            "DELETE FROM private.rate_event WHERE at < now() - make_interval(secs => $1::bigint)",
            &[&longest],
        )
        .await?;
    client
        .execute(
            "INSERT INTO private.rate_event (tenant, class, key) VALUES ($1, $2, $3)",
            &[&tenant, &class, &key],
        )
        .await?;
    Ok(())
}

/// How many acts were counted under a class and key within the last `window` seconds.
///
/// # Errors
///
/// If the store fails.
pub async fn count<C: GenericClient>(
    client: &C,
    tenant: &str,
    class: &str,
    key: &str,
    window: u64,
) -> Result<i64, AccountsError> {
    let window = i64::try_from(window).unwrap_or(i64::MAX);
    let row = client
        .query_one(
            "SELECT count(*) FROM private.rate_event
             WHERE tenant = $1 AND class = $2 AND key = $3
               AND at >= now() - make_interval(secs => $4::bigint)",
            &[&tenant, &class, &key, &window],
        )
        .await?;
    Ok(row.get(0))
}

/// The difficulty for a caller (0087 §4): the base plus one bit for each account created
/// from the address key in the last day, up to the ceiling.
///
/// # Errors
///
/// If the store fails.
pub async fn difficulty<C: GenericClient>(
    client: &C,
    tenant: &str,
    address: &str,
    bits: u8,
    max_bits: u8,
) -> Result<u8, AccountsError> {
    let created = count(
        client,
        tenant,
        CREATEACCOUNT,
        address,
        CREATEACCOUNT_LIMIT.1,
    )
    .await?;
    let rise = u8::try_from(created.clamp(0, 32)).unwrap_or(32);
    Ok(bits
        .saturating_add(rise)
        .min(max_bits.max(bits))
        .min(scatter_challenge::MAX_BITS))
}

/// Whether a `password` login from an address key, for a name, needs a challenge: the
/// address has failed 3 times within 5 minutes, or the account 20 times within 10
/// (0087 §2).
///
/// # Errors
///
/// If the store fails.
pub async fn badlogin_applies<C: GenericClient>(
    client: &C,
    tenant: &str,
    address: &str,
    account: &str,
) -> Result<bool, AccountsError> {
    let by_address = count(
        client,
        tenant,
        BADLOGIN_ADDRESS,
        address,
        BADLOGIN_ADDRESS_LIMIT.1,
    )
    .await?;
    if by_address >= BADLOGIN_ADDRESS_LIMIT.0 {
        return Ok(true);
    }
    let by_account = count(
        client,
        tenant,
        BADLOGIN_ACCOUNT,
        &account.to_lowercase(),
        BADLOGIN_ACCOUNT_LIMIT.1,
    )
    .await?;
    Ok(by_account >= BADLOGIN_ACCOUNT_LIMIT.0)
}

/// Counts a failed `password` login, by address key and by account.
///
/// # Errors
///
/// If the store fails.
pub async fn record_badlogin<C: GenericClient>(
    client: &C,
    tenant: &str,
    address: &str,
    account: &str,
) -> Result<(), AccountsError> {
    record(client, tenant, BADLOGIN_ADDRESS, address).await?;
    record(client, tenant, BADLOGIN_ACCOUNT, &account.to_lowercase()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_keys() {
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        assert_eq!(address_key(ip("192.0.2.7")), "192.0.2.7");
        assert_eq!(address_key(ip("2001:db8:1:2:3:4:5:6")), "2001:db8:1:2::/64");
        assert_eq!(
            address_key(ip("2001:db8:1:2:ffff::1")),
            address_key(ip("2001:db8:1:2::9"))
        );
        assert_eq!(address_key(ip("::ffff:192.0.2.7")), "192.0.2.7");
        assert_eq!(address_key(None), "unknown");
    }

    #[test]
    fn issued_challenges_check() {
        let secret = Secret::derive(b"seed");
        let c = issue(&secret, "s1", "wiki", Trigger::CreateAccount, 4).unwrap();
        let n = scatter_challenge::solve(&c.to_string(), 4).to_string();
        assert_eq!(
            c.check(
                &key(&secret),
                "s1",
                now_secs(),
                "wiki",
                Trigger::CreateAccount,
                &n
            ),
            Ok(())
        );
        // A key for tokens is not the key for challenges.
        assert!(!c.verify_mac(&secret.subkey("other"), "s1"));
    }
}
