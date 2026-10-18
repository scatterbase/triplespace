//! The proof-of-work challenge of account registration (0087 §2–3; 07 §2.6).
//!
//! A **challenge** is the string `1.{tenant}.{trigger}.{bits}.{expires}.{salt}.{mac}`: the
//! format version; the tenant; the trigger it was issued for; the difficulty in bits; the
//! expiry in seconds since the epoch; 16 random bytes, base64url without padding; and a
//! MAC, the first 16 bytes of HMAC-SHA256, base64url without padding, over everything
//! before it (without the final dot), a zero byte and the binding, the session it was
//! issued to. The binding is not in the string, so a challenge carried to another session
//! fails.
//!
//! A **solution** is a decimal number `n`, written without leading zeros, such that
//! SHA-256 of `{challenge}:{n}` in UTF-8 begins with at least `bits` zero bits. Finding
//! one takes `2^bits` hashes on average; checking one takes one.
//!
//! The crate is pure: no I/O and no clock. Its test vectors, `vectors.json`, fix every
//! detail, and the site's JavaScript solver is tested against the same file.

#![forbid(unsafe_code)]

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac as _};
use sha2::{Digest as _, Sha256};

/// The format version this crate writes and reads.
pub const VERSION: &str = "1";

/// The work's name, as `meta=authmanagerinfo` reports it.
pub const ALGORITHM: &str = "sha256-leading-zero-bits";

/// The most bits a challenge may ask for: beyond this a solution takes hours.
pub const MAX_BITS: u8 = 32;

/// How long a challenge stays valid after it is issued, in seconds (0087 §3).
pub const LIFETIME_SECS: u64 = 600;

/// The act a challenge guards (0087 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Trigger {
    /// Creating an account, or filing a request for one, with the `password` issuer.
    CreateAccount,
    /// The save that would create a temporary account.
    TempAccount,
    /// A `password` login after repeated failures.
    BadLogin,
}

impl Trigger {
    /// Every trigger, in the order 0087 §2 lists them.
    pub const ALL: [Self; 3] = [Self::CreateAccount, Self::TempAccount, Self::BadLogin];

    /// The trigger's name, as `challenge.triggers` lists it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateAccount => "createaccount",
            Self::TempAccount => "tempaccount",
            Self::BadLogin => "badlogin",
        }
    }

    /// A trigger by name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a challenge or a solution is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The string is not a challenge of this format.
    Malformed,
    /// The MAC does not match: forged, altered, or issued to another session.
    BadMac,
    /// Past its expiry.
    Expired,
    /// Issued for another tenant or another act.
    WrongAct,
    /// The solution is not a canonical decimal number.
    BadSolution,
    /// The solution does not meet the difficulty.
    NotSolved,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "not a challenge",
            Self::BadMac => "the challenge was not issued to this session",
            Self::Expired => "the challenge has expired",
            Self::WrongAct => "the challenge was issued for something else",
            Self::BadSolution => "the solution is not a number",
            Self::NotSolved => "the solution does not meet the difficulty",
        })
    }
}

impl std::error::Error for Refusal {}

/// A challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// The tenant it was issued on.
    pub tenant: String,
    /// The act it guards.
    pub trigger: Trigger,
    /// The difficulty: leading zero bits.
    pub bits: u8,
    /// The expiry, seconds since the epoch.
    pub expires: u64,
    /// 16 random bytes.
    pub salt: [u8; 16],
    mac: String,
}

/// Whether a tenant name can sit in a challenge as it is: no dots, nothing outside
/// letters, digits, `-` and `_`.
fn tenant_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn body(tenant: &str, trigger: Trigger, bits: u8, expires: u64, salt: &[u8; 16]) -> String {
    format!(
        "{VERSION}.{tenant}.{trigger}.{bits}.{expires}.{}",
        URL_SAFE_NO_PAD.encode(salt)
    )
}

fn mac_of(key: &[u8], body: &str, binding: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(body.as_bytes());
    mac.update(&[0]);
    mac.update(binding.as_bytes());
    URL_SAFE_NO_PAD.encode(&mac.finalize().into_bytes()[..16])
}

impl Challenge {
    /// Issues a challenge to a binding (a session ID), under a key the issuer keeps.
    ///
    /// # Panics
    ///
    /// If the tenant holds a dot or anything a challenge cannot carry, or `bits` is above
    /// [`MAX_BITS`]: callers pass a tenant slug and a validated setting.
    #[must_use]
    pub fn issue(
        key: &[u8],
        binding: &str,
        tenant: &str,
        trigger: Trigger,
        bits: u8,
        expires: u64,
        salt: [u8; 16],
    ) -> Self {
        assert!(tenant_ok(tenant), "a tenant a challenge can carry");
        assert!(bits <= MAX_BITS, "at most {MAX_BITS} bits");
        let mac = mac_of(key, &body(tenant, trigger, bits, expires, &salt), binding);
        Self {
            tenant: tenant.to_string(),
            trigger,
            bits,
            expires,
            salt,
            mac,
        }
    }

    /// Reads a challenge string.
    ///
    /// # Errors
    ///
    /// [`Refusal::Malformed`] for anything else.
    pub fn parse(s: &str) -> Result<Self, Refusal> {
        let parts: Vec<&str> = s.split('.').collect();
        let [version, tenant, trigger, bits, expires, salt, mac] = parts[..] else {
            return Err(Refusal::Malformed);
        };
        if version != VERSION || !tenant_ok(tenant) || mac.len() != 22 {
            return Err(Refusal::Malformed);
        }
        let trigger = Trigger::parse(trigger).ok_or(Refusal::Malformed)?;
        let bits: u8 = bits.parse().map_err(|_| Refusal::Malformed)?;
        if bits > MAX_BITS {
            return Err(Refusal::Malformed);
        }
        let expires: u64 = expires.parse().map_err(|_| Refusal::Malformed)?;
        let salt: [u8; 16] = URL_SAFE_NO_PAD
            .decode(salt)
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or(Refusal::Malformed)?;
        Ok(Self {
            tenant: tenant.to_string(),
            trigger,
            bits,
            expires,
            salt,
            mac: mac.to_string(),
        })
    }

    /// The MAC, which names the challenge in the spent set.
    #[must_use]
    pub fn mac(&self) -> &str {
        &self.mac
    }

    /// Whether the MAC is the one the key gives for this binding, compared in constant
    /// time.
    #[must_use]
    pub fn verify_mac(&self, key: &[u8], binding: &str) -> bool {
        let expected = mac_of(
            key,
            &body(
                &self.tenant,
                self.trigger,
                self.bits,
                self.expires,
                &self.salt,
            ),
            binding,
        );
        expected.len() == self.mac.len()
            && expected
                .bytes()
                .zip(self.mac.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }

    /// Checks a solution, in 0087 §3's order: the MAC, the expiry, the tenant and the
    /// act, the work. Whether the challenge is already spent is the caller's to check
    /// after, since only the caller holds the spent set.
    ///
    /// # Errors
    ///
    /// The first check that fails.
    pub fn check(
        &self,
        key: &[u8],
        binding: &str,
        now: u64,
        tenant: &str,
        trigger: Trigger,
        solution: &str,
    ) -> Result<(), Refusal> {
        if !self.verify_mac(key, binding) {
            return Err(Refusal::BadMac);
        }
        if now >= self.expires {
            return Err(Refusal::Expired);
        }
        if self.tenant != tenant || self.trigger != trigger {
            return Err(Refusal::WrongAct);
        }
        let n = parse_solution(solution).ok_or(Refusal::BadSolution)?;
        if meets(&self.to_string(), n, self.bits) {
            Ok(())
        } else {
            Err(Refusal::NotSolved)
        }
    }
}

impl fmt::Display for Challenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}",
            body(
                &self.tenant,
                self.trigger,
                self.bits,
                self.expires,
                &self.salt
            ),
            self.mac
        )
    }
}

/// A solution as a number: decimal digits, at most 20, no leading zero but `0` itself.
#[must_use]
pub fn parse_solution(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The number of leading zero bits of a digest.
#[must_use]
pub fn leading_zero_bits(digest: &[u8]) -> u32 {
    let mut n = 0;
    for b in digest {
        if *b == 0 {
            n += 8;
        } else {
            return n + b.leading_zeros();
        }
    }
    n
}

/// SHA-256 of `{challenge}:{n}`.
#[must_use]
pub fn work(challenge: &str, n: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(challenge.as_bytes());
    h.update(b":");
    h.update(n.to_string().as_bytes());
    h.finalize().into()
}

/// Whether `n` solves a challenge at a difficulty.
#[must_use]
pub fn meets(challenge: &str, n: u64, bits: u8) -> bool {
    leading_zero_bits(&work(challenge, n)) >= u32::from(bits)
}

/// The smallest solution of a challenge string, counting up from 0: what a client
/// without a browser runs (`triplespace-cli challenge solve`), and what tests use.
///
/// # Panics
///
/// Only if no `u64` solves it, which at [`MAX_BITS`] has probability nil.
#[must_use]
pub fn solve(challenge: &str, bits: u8) -> u64 {
    let prefix = {
        let mut h = Sha256::new();
        h.update(challenge.as_bytes());
        h.update(b":");
        h
    };
    let mut buf = itoa_buffer();
    (0..=u64::MAX)
        .find(|n| {
            let mut h = prefix.clone();
            h.update(itoa(&mut buf, *n));
            leading_zero_bits(&h.finalize()) >= u32::from(bits)
        })
        .expect("a solution exists")
}

fn itoa_buffer() -> [u8; 20] {
    [0; 20]
}

/// `n` in decimal, written into the end of a buffer.
fn itoa(buf: &mut [u8; 20], mut n: u64) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + u8::try_from(n % 10).expect("a digit");
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buf[i..]
}

/// The test vectors, as JSON: what [`Challenge::issue`] gives for a key, a binding and
/// the fields, and the smallest solution of each.
pub const VECTORS: &str = include_str!("../vectors.json");

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn hex(b: &[u8]) -> String {
        b.iter().fold(String::new(), |mut s, x| {
            use std::fmt::Write as _;
            let _ = write!(s, "{x:02x}");
            s
        })
    }

    fn salt(s: &str) -> [u8; 16] {
        let v: Vec<u8> = (0..16)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        v.try_into().unwrap()
    }

    #[test]
    fn the_vectors_hold() {
        let v: Value = serde_json::from_str(VECTORS).unwrap();
        let vectors = v["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 4);
        for t in vectors {
            let c = Challenge::issue(
                t["key"].as_str().unwrap().as_bytes(),
                t["binding"].as_str().unwrap(),
                t["tenant"].as_str().unwrap(),
                Trigger::parse(t["trigger"].as_str().unwrap()).unwrap(),
                u8::try_from(t["bits"].as_u64().unwrap()).unwrap(),
                t["expires"].as_u64().unwrap(),
                salt(t["salt_hex"].as_str().unwrap()),
            );
            let s = c.to_string();
            assert_eq!(s, t["challenge"].as_str().unwrap());
            assert_eq!(Challenge::parse(&s).unwrap(), c);
            let n = t["solution"].as_u64().unwrap();
            assert_eq!(solve(&s, c.bits), n, "{s}");
            assert_eq!(hex(&work(&s, n)), t["hash"].as_str().unwrap());
            if c.expires == 0 {
                continue;
            }
            assert_eq!(
                c.check(
                    t["key"].as_str().unwrap().as_bytes(),
                    t["binding"].as_str().unwrap(),
                    c.expires - 1,
                    &c.tenant,
                    c.trigger,
                    &n.to_string()
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn checks_refuse_in_order() {
        let key = b"k";
        let c = Challenge::issue(key, "s1", "wiki", Trigger::CreateAccount, 8, 1000, [7; 16]);
        let n = solve(&c.to_string(), 8).to_string();
        let check = |binding, now, tenant, trigger, solution: &str| {
            c.check(key, binding, now, tenant, trigger, solution)
        };
        assert_eq!(
            check("s2", 0, "wiki", Trigger::CreateAccount, &n),
            Err(Refusal::BadMac)
        );
        assert_eq!(
            check("s1", 1000, "wiki", Trigger::CreateAccount, &n),
            Err(Refusal::Expired)
        );
        assert_eq!(
            check("s1", 0, "other", Trigger::CreateAccount, &n),
            Err(Refusal::WrongAct)
        );
        assert_eq!(
            check("s1", 0, "wiki", Trigger::BadLogin, &n),
            Err(Refusal::WrongAct)
        );
        assert_eq!(
            check("s1", 0, "wiki", Trigger::CreateAccount, "007"),
            Err(Refusal::BadSolution)
        );
        let wrong = (0..u64::MAX)
            .find(|m| !meets(&c.to_string(), *m, 8))
            .unwrap()
            .to_string();
        assert_eq!(
            check("s1", 0, "wiki", Trigger::CreateAccount, &wrong),
            Err(Refusal::NotSolved)
        );
        assert_eq!(check("s1", 0, "wiki", Trigger::CreateAccount, &n), Ok(()));
        // An altered field breaks the MAC.
        let altered = c.to_string().replacen(".8.", ".1.", 1);
        assert!(!Challenge::parse(&altered).unwrap().verify_mac(key, "s1"));
    }

    #[test]
    fn parsing_refuses_what_is_not_a_challenge() {
        for s in [
            "",
            "1.wiki.createaccount.8.10.AAAAAAAAAAAAAAAAAAAAAA",
            "2.wiki.createaccount.8.10.AAAAAAAAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA",
            "1.wiki.edit.8.10.AAAAAAAAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA",
            "1.wiki.createaccount.33.10.AAAAAAAAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA",
            "1.wi/ki.createaccount.8.10.AAAAAAAAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert_eq!(Challenge::parse(s), Err(Refusal::Malformed), "{s}");
        }
        assert_eq!(parse_solution("0"), Some(0));
        assert_eq!(parse_solution("18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_solution("18446744073709551616"), None);
        assert_eq!(parse_solution("-1"), None);
        assert_eq!(parse_solution("01"), None);
    }

    #[test]
    fn zero_bits() {
        assert_eq!(leading_zero_bits(&[0, 0, 0x10]), 19);
        assert_eq!(leading_zero_bits(&[0x80]), 0);
        assert_eq!(leading_zero_bits(&[0, 0]), 16);
        let mut buf = itoa_buffer();
        assert_eq!(itoa(&mut buf, 0), b"0");
        assert_eq!(itoa(&mut buf, u64::MAX), b"18446744073709551615");
    }
}
