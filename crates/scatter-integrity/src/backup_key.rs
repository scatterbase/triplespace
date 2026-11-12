//! The backup key set (0103 A3, A5): the keys a backup's sealed sections are encrypted to,
//! so that making a backup needs only their public halves and restoring one needs `k` of
//! their holders, none of whom reveals a private key.
//!
//! - Each backup has a **data key** of its own, 32 random bytes, under which its sealed
//!   sections are encrypted with XChaCha20-Poly1305 ([`DataKey::encrypt`]).
//! - The data key is split into one **share** per key of the set, any `k` of which rebuild
//!   it: Shamir's secret sharing over GF(256) (`vsss-rs`) when `k` is more than one; when
//!   `k` is one, the default, each share is the data key itself.
//! - Each share is **sealed** to its key: an ephemeral X25519 key agrees a secret with the
//!   holder's public key, HKDF-SHA256 derives a key from it, and XChaCha20-Poly1305 seals
//!   the share under that key. The associated data names the share's context and key, so a
//!   sealed share copied to another backup or another holder does not open.
//! - A holder **unseals** their share offline with their secret key; `k` unsealed shares
//!   are **combined** into the data key.
//!
//! The text forms are `backup-key:{base64url}` for a public key, `backup-secret:{base64url}`
//! for a secret key, and `backup-share:{base64url of CBOR}` for an unsealed share.

use std::convert::Infallible;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use scatter_log::cbor::{self, Value};
use sha2::{Digest as _, Sha256};
use x25519_dalek::{PublicKey as XPublic, StaticSecret};

/// The domain separator of a share's key derivation and associated data.
pub const DOMAIN: &str = "scatter-backup-share-v1";

/// The most keys a set may hold: Shamir over GF(256) numbers shares 1 to 255.
pub const MAX_KEYS: usize = 255;

const PUBLIC_PREFIX: &str = "backup-key:";
const SECRET_PREFIX: &str = "backup-secret:";
const SHARE_PREFIX: &str = "backup-share:";
const NONCE: usize = 24;

/// Why a backup key operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BackupKeyError {
    /// A text form did not parse.
    #[error("not a {0}")]
    Parse(&'static str),
    /// The key set is not valid.
    #[error("key set: {0}")]
    Set(String),
    /// The share was not sealed to this key, or was not for this backup.
    #[error("this key opens none of the backup's shares")]
    NotHolder,
    /// A sealed value did not open: tampered with, or under another key.
    #[error("a sealed value did not open")]
    Open,
    /// Too few shares, or shares that do not belong together.
    #[error("shares: {0}")]
    Shares(String),
    /// A sealed key's encoding is wrong.
    #[error("sealed key: {0}")]
    Encoding(&'static str),
    /// The holder's public key agreed no secret (a low-order point).
    #[error("the key agreed no secret")]
    NotContributory,
}

/// The operating system's random source, as `rand_core` 0.10 wants it for `vsss-rs`.
struct OsRng;

impl rand_core::TryRng for OsRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut b = [0u8; 4];
        fill(&mut b);
        Ok(u32::from_le_bytes(b))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut b = [0u8; 8];
        fill(&mut b);
        Ok(u64::from_le_bytes(b))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        fill(dst);
        Ok(())
    }
}

impl rand_core::TryCryptoRng for OsRng {}

/// Fills `dst` from the operating system's random source.
///
/// # Panics
///
/// If the source fails, which on the platforms Triplespace runs on it does not.
fn fill(dst: &mut [u8]) {
    getrandom::fill(dst).expect("the operating system's random source");
}

/// `N` bytes from the operating system's random source, read into uninitialised memory,
/// so that no value is ever in the buffer but the random one.
///
/// # Panics
///
/// As [`fill`].
fn random<const N: usize>() -> [u8; N] {
    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); N];
    let filled = getrandom::fill_uninit(&mut buf).expect("the operating system's random source");
    <[u8; N]>::try_from(&*filled).expect("`fill_uninit` fills the whole buffer")
}

/// A holder's public key: what a key set lists and a backup seals a share to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    /// `backup-key:{base64url}`.
    #[must_use]
    pub fn to_text(&self) -> String {
        format!("{PUBLIC_PREFIX}{}", URL_SAFE_NO_PAD.encode(self.0))
    }

    /// From [`PublicKey::to_text`]'s form.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Parse`].
    pub fn parse(s: &str) -> Result<Self, BackupKeyError> {
        let bad = || BackupKeyError::Parse("backup public key (`backup-key:…`)");
        let b64 = s.trim().strip_prefix(PUBLIC_PREFIX).ok_or_else(bad)?;
        let bytes = URL_SAFE_NO_PAD.decode(b64).map_err(|_| bad())?;
        Ok(Self(bytes.try_into().map_err(|_| bad())?))
    }

    /// The key's ID: the first 8 bytes of its SHA-256, in hex.
    #[must_use]
    pub fn key_id(&self) -> String {
        use std::fmt::Write as _;
        Sha256::digest(self.0)[..8]
            .iter()
            .fold(String::with_capacity(16), |mut out, b| {
                let _ = write!(out, "{b:02x}");
                out
            })
    }
}

/// A holder's secret key, which never reaches the instance.
pub struct SecretKey(StaticSecret);

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretKey({})", self.public().key_id())
    }
}

impl SecretKey {
    /// A new key from the operating system's random source.
    #[must_use]
    pub fn generate() -> Self {
        Self(StaticSecret::from(random::<32>()))
    }

    /// Its public half.
    #[must_use]
    pub fn public(&self) -> PublicKey {
        PublicKey(XPublic::from(&self.0).to_bytes())
    }

    /// `backup-secret:{base64url}`: what the holder keeps, offline.
    #[must_use]
    pub fn to_text(&self) -> String {
        format!(
            "{SECRET_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(self.0.to_bytes())
        )
    }

    /// From [`SecretKey::to_text`]'s form.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Parse`].
    pub fn parse(s: &str) -> Result<Self, BackupKeyError> {
        let bad = || BackupKeyError::Parse("backup secret key (`backup-secret:…`)");
        let b64 = s.trim().strip_prefix(SECRET_PREFIX).ok_or_else(bad)?;
        let bytes: [u8; 32] = URL_SAFE_NO_PAD
            .decode(b64)
            .map_err(|_| bad())?
            .try_into()
            .map_err(|_| bad())?;
        Ok(Self(StaticSecret::from(bytes)))
    }
}

/// The backup key set: the holders' public keys and how many of them a restore needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySet {
    keys: Vec<PublicKey>,
    threshold: usize,
}

impl KeySet {
    /// A set of `keys`, `threshold` of which restore.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Set`] for no keys, more than [`MAX_KEYS`], a key twice, or a
    /// threshold of 0 or more than the keys.
    pub fn new(keys: Vec<PublicKey>, threshold: usize) -> Result<Self, BackupKeyError> {
        if keys.is_empty() {
            return Err(BackupKeyError::Set("it holds no key".into()));
        }
        if keys.len() > MAX_KEYS {
            return Err(BackupKeyError::Set(format!(
                "it holds {} keys; at most {MAX_KEYS}",
                keys.len()
            )));
        }
        for (i, k) in keys.iter().enumerate() {
            if keys[..i].contains(k) {
                return Err(BackupKeyError::Set(format!(
                    "key {} is listed twice",
                    k.key_id()
                )));
            }
        }
        if threshold == 0 || threshold > keys.len() {
            return Err(BackupKeyError::Set(format!(
                "a threshold of {threshold} with {} keys: it must be from 1 to the number of keys",
                keys.len()
            )));
        }
        Ok(Self { keys, threshold })
    }

    /// The keys, in the order the set lists them.
    #[must_use]
    pub fn keys(&self) -> &[PublicKey] {
        &self.keys
    }

    /// How many shares a restore needs.
    #[must_use]
    pub fn threshold(&self) -> usize {
        self.threshold
    }

    /// The JSON a `backup-keys` config record holds: `{"keys": [...], "threshold": k}`.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "keys": self.keys.iter().map(PublicKey::to_text).collect::<Vec<_>>(),
            "threshold": self.threshold,
        })
    }

    /// From [`KeySet::to_json`]'s form; `threshold` defaults to 1 (0103 A3).
    ///
    /// # Errors
    ///
    /// A key that does not parse, or a set [`KeySet::new`] refuses.
    pub fn from_json(v: &serde_json::Value) -> Result<Self, BackupKeyError> {
        let keys = v
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| BackupKeyError::Set("`keys` is not a list".into()))?
            .iter()
            .map(|k| {
                k.as_str()
                    .ok_or(BackupKeyError::Parse("backup public key (`backup-key:…`)"))
                    .and_then(PublicKey::parse)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let threshold = match v.get("threshold") {
            None => 1,
            Some(t) => t
                .as_u64()
                .and_then(|t| usize::try_from(t).ok())
                .ok_or_else(|| BackupKeyError::Set("`threshold` is not a count".into()))?,
        };
        Self::new(keys, threshold)
    }
}

/// A backup's data key: the key its sealed sections are encrypted under. Zeroed on drop.
pub struct DataKey([u8; 32]);

impl Drop for DataKey {
    fn drop(&mut self) {
        self.0 = [0; 32];
    }
}

impl DataKey {
    fn generate() -> Self {
        Self(random::<32>())
    }

    /// Encrypts `plaintext` for the place `aad` names: a 24-byte random nonce, then the
    /// XChaCha20-Poly1305 ciphertext.
    #[must_use]
    pub fn encrypt(&self, aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let nonce = random::<NONCE>();
        let ct = XChaCha20Poly1305::new(&self.0.into())
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .expect("XChaCha20-Poly1305 encrypts any length a backup holds");
        let mut out = nonce.to_vec();
        out.extend(ct);
        out
    }

    /// Opens what [`DataKey::encrypt`] sealed for `aad`.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Open`] for another key, another place, or a change.
    pub fn decrypt(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, BackupKeyError> {
        if sealed.len() < NONCE {
            return Err(BackupKeyError::Open);
        }
        let (nonce, ct) = sealed.split_at(NONCE);
        let nonce: [u8; NONCE] = nonce.try_into().map_err(|_| BackupKeyError::Open)?;
        XChaCha20Poly1305::new(&self.0.into())
            .decrypt(&XNonce::from(nonce), Payload { msg: ct, aad })
            .map_err(|_| BackupKeyError::Open)
    }
}

/// One share, sealed to one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedShare {
    /// The key it is sealed to.
    pub key_id: String,
    epk: [u8; 32],
    sealed: Vec<u8>,
}

/// A backup's data key, as the backup carries it: a share sealed to each key of the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedKey {
    /// What the key protects, bound into every share: the backup it belongs to.
    pub context: String,
    /// How many shares rebuild the key.
    pub threshold: usize,
    /// One per key of the set the backup was made under, in the set's order.
    pub shares: Vec<SealedShare>,
}

/// A share a holder has unsealed: what they hand to the restore. Zeroed on drop.
pub struct Share {
    context: String,
    key_id: String,
    bytes: Vec<u8>,
}

impl Drop for Share {
    fn drop(&mut self) {
        self.bytes.iter_mut().for_each(|b| *b = 0);
    }
}

impl std::fmt::Debug for Share {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Share({}, {})", self.context, self.key_id)
    }
}

fn info(context: &str, key_id: &str) -> Vec<u8> {
    [DOMAIN, context, key_id].join("\0").into_bytes()
}

/// The key a share is sealed under: HKDF-SHA256 over the agreed secret, salted with both
/// public keys, its info naming the context and the recipient.
fn share_cipher(
    shared: &[u8; 32],
    epk: &[u8; 32],
    rpk: &[u8; 32],
    info: &[u8],
) -> XChaCha20Poly1305 {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(epk);
    salt[32..].copy_from_slice(rpk);
    let mut okm = chacha20poly1305::Key::default();
    Hkdf::<Sha256>::new(Some(&salt), shared)
        .expand(info, &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 length");
    XChaCha20Poly1305::new(&okm)
}

fn seal_share(to: &PublicKey, context: &str, share: &[u8]) -> Result<SealedShare, BackupKeyError> {
    let eph = StaticSecret::from(random::<32>());
    let epk = XPublic::from(&eph).to_bytes();
    let shared = eph.diffie_hellman(&XPublic::from(to.0));
    if !shared.was_contributory() {
        return Err(BackupKeyError::NotContributory);
    }
    let key_id = to.key_id();
    let info = info(context, &key_id);
    let nonce = random::<NONCE>();
    let ct = share_cipher(shared.as_bytes(), &epk, &to.0, &info)
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: share,
                aad: &info,
            },
        )
        .expect("XChaCha20-Poly1305 encrypts a share");
    let mut sealed = nonce.to_vec();
    sealed.extend(ct);
    Ok(SealedShare {
        key_id,
        epk,
        sealed,
    })
}

/// Seals `key` to every key of `set` for `context`.
///
/// # Errors
///
/// A key of the set that agrees no secret.
pub fn seal_key(set: &KeySet, context: &str, key: &DataKey) -> Result<SealedKey, BackupKeyError> {
    let shares: Vec<Vec<u8>> = if set.threshold == 1 {
        vec![key.0.to_vec(); set.keys.len()]
    } else {
        vsss_rs::Gf256::split_bytes(set.threshold, set.keys.len(), key.0, OsRng)
            .map_err(|e| BackupKeyError::Shares(format!("splitting: {e:?}")))?
    };
    let shares = set
        .keys
        .iter()
        .zip(&shares)
        .map(|(k, s)| seal_share(k, context, s))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SealedKey {
        context: context.to_string(),
        threshold: set.threshold,
        shares,
    })
}

/// A new data key for a backup named `context`, sealed to every key of `set`.
///
/// # Errors
///
/// As [`seal_key`].
pub fn seal_new(set: &KeySet, context: &str) -> Result<(DataKey, SealedKey), BackupKeyError> {
    let key = DataKey::generate();
    let sealed = seal_key(set, context, &key)?;
    Ok((key, sealed))
}

impl SealedKey {
    /// The share sealed to `secret`'s key, opened.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::NotHolder`] when no share is sealed to the key;
    /// [`BackupKeyError::Open`] when the share does not open.
    pub fn unseal(&self, secret: &SecretKey) -> Result<Share, BackupKeyError> {
        let public = secret.public();
        let key_id = public.key_id();
        let sealed = self
            .shares
            .iter()
            .find(|s| s.key_id == key_id)
            .ok_or(BackupKeyError::NotHolder)?;
        let shared = secret.0.diffie_hellman(&XPublic::from(sealed.epk));
        if !shared.was_contributory() {
            return Err(BackupKeyError::NotContributory);
        }
        if sealed.sealed.len() < NONCE {
            return Err(BackupKeyError::Open);
        }
        let (nonce, ct) = sealed.sealed.split_at(NONCE);
        let nonce: [u8; NONCE] = nonce.try_into().map_err(|_| BackupKeyError::Open)?;
        let info = info(&self.context, &key_id);
        let bytes = share_cipher(shared.as_bytes(), &sealed.epk, &public.0, &info)
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ct,
                    aad: &info,
                },
            )
            .map_err(|_| BackupKeyError::Open)?;
        Ok(Share {
            context: self.context.clone(),
            key_id,
            bytes,
        })
    }

    /// The data key, from at least [`SealedKey::threshold`] unsealed shares of this key.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Shares`] for too few distinct shares, or one of another backup.
    pub fn combine(&self, shares: &[Share]) -> Result<DataKey, BackupKeyError> {
        let mut ids: Vec<&str> = Vec::new();
        for s in shares {
            if s.context != self.context {
                return Err(BackupKeyError::Shares(format!(
                    "a share is for `{}`, not this backup (`{}`)",
                    s.context, self.context
                )));
            }
            if !self.shares.iter().any(|x| x.key_id == s.key_id) {
                return Err(BackupKeyError::Shares(format!(
                    "key {} holds no share of this backup",
                    s.key_id
                )));
            }
            if !ids.contains(&s.key_id.as_str()) {
                ids.push(&s.key_id);
            }
        }
        if ids.len() < self.threshold {
            return Err(BackupKeyError::Shares(format!(
                "{} of the {} shares needed",
                ids.len(),
                self.threshold
            )));
        }
        let bytes = if self.threshold == 1 {
            shares[0].bytes.clone()
        } else {
            let mut seen: Vec<&str> = Vec::new();
            let mut chosen: Vec<Vec<u8>> = Vec::new();
            for s in shares {
                if !seen.contains(&s.key_id.as_str()) {
                    seen.push(&s.key_id);
                    chosen.push(s.bytes.clone());
                }
                if chosen.len() == self.threshold {
                    break;
                }
            }
            vsss_rs::Gf256::combine_bytes(&chosen)
                .map_err(|e| BackupKeyError::Shares(format!("combining: {e:?}")))?
        };
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| BackupKeyError::Shares("the shares rebuild no 32-byte key".into()))?;
        Ok(DataKey(key))
    }

    /// The CBOR a backup carries: `{"context", "threshold", "shares": [{"key", "epk",
    /// "sealed"}, …]}`.
    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::map(vec![
            (Value::text("context"), Value::text(&self.context)),
            (
                Value::text("threshold"),
                Value::Int(i128::try_from(self.threshold).unwrap_or(i128::MAX)),
            ),
            (
                Value::text("shares"),
                Value::Array(
                    self.shares
                        .iter()
                        .map(|s| {
                            Value::map(vec![
                                (Value::text("key"), Value::text(&s.key_id)),
                                (Value::text("epk"), Value::Bytes(s.epk.to_vec())),
                                (Value::text("sealed"), Value::Bytes(s.sealed.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }

    /// From [`SealedKey::to_value`]'s form.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Encoding`].
    pub fn from_value(v: &Value) -> Result<Self, BackupKeyError> {
        let bad = BackupKeyError::Encoding;
        let context = v
            .get("context")
            .and_then(Value::as_text)
            .ok_or(bad("context"))?
            .to_string();
        let threshold = v
            .get("threshold")
            .and_then(Value::as_u64)
            .and_then(|t| usize::try_from(t).ok())
            .filter(|t| *t >= 1)
            .ok_or(bad("threshold"))?;
        let Some(Value::Array(list)) = v.get("shares") else {
            return Err(bad("shares"));
        };
        let shares = list
            .iter()
            .map(|s| {
                Ok(SealedShare {
                    key_id: s
                        .get("key")
                        .and_then(Value::as_text)
                        .ok_or(bad("key"))?
                        .to_string(),
                    epk: s
                        .get("epk")
                        .and_then(Value::as_bytes)
                        .and_then(|b| <[u8; 32]>::try_from(b).ok())
                        .ok_or(bad("epk"))?,
                    sealed: s
                        .get("sealed")
                        .and_then(Value::as_bytes)
                        .ok_or(bad("sealed"))?
                        .to_vec(),
                })
            })
            .collect::<Result<Vec<_>, BackupKeyError>>()?;
        if threshold > shares.len() {
            return Err(bad("threshold"));
        }
        Ok(Self {
            context,
            threshold,
            shares,
        })
    }

    /// The encoded form.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Encoding`] if the encoder refuses.
    pub fn encode(&self) -> Result<Vec<u8>, BackupKeyError> {
        cbor::encode(&self.to_value()).map_err(|_| BackupKeyError::Encoding("encode"))
    }

    /// From [`SealedKey::encode`]'s bytes.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Encoding`].
    pub fn decode(bytes: &[u8]) -> Result<Self, BackupKeyError> {
        let v = cbor::decode(bytes).map_err(|_| BackupKeyError::Encoding("decode"))?;
        Self::from_value(&v)
    }
}

impl Share {
    /// The key whose share this is.
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// `backup-share:{base64url of CBOR}`: what a holder writes to a file and hands to the
    /// restore. It opens the backup together with the other shares a restore needs, so it
    /// is kept as the data it protects is kept.
    #[must_use]
    pub fn to_text(&self) -> String {
        let v = Value::map(vec![
            (Value::text("context"), Value::text(&self.context)),
            (Value::text("key"), Value::text(&self.key_id)),
            (Value::text("share"), Value::Bytes(self.bytes.clone())),
        ]);
        let bytes = cbor::encode(&v).expect("a share encodes");
        format!("{SHARE_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
    }

    /// From [`Share::to_text`]'s form.
    ///
    /// # Errors
    ///
    /// [`BackupKeyError::Parse`].
    pub fn parse(s: &str) -> Result<Self, BackupKeyError> {
        let bad = || BackupKeyError::Parse("backup share (`backup-share:…`)");
        let b64 = s.trim().strip_prefix(SHARE_PREFIX).ok_or_else(bad)?;
        let bytes = URL_SAFE_NO_PAD.decode(b64).map_err(|_| bad())?;
        let v = cbor::decode(&bytes).map_err(|_| bad())?;
        Ok(Self {
            context: v
                .get("context")
                .and_then(Value::as_text)
                .ok_or_else(bad)?
                .to_string(),
            key_id: v
                .get("key")
                .and_then(Value::as_text)
                .ok_or_else(bad)?
                .to_string(),
            bytes: v
                .get("share")
                .and_then(Value::as_bytes)
                .ok_or_else(bad)?
                .to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holders(n: usize) -> (Vec<SecretKey>, Vec<PublicKey>) {
        let secrets: Vec<SecretKey> = (0..n).map(|_| SecretKey::generate()).collect();
        let publics = secrets.iter().map(SecretKey::public).collect();
        (secrets, publics)
    }

    #[test]
    fn text_forms_round_trip() {
        let s = SecretKey::generate();
        let p = s.public();
        assert_eq!(PublicKey::parse(&p.to_text()).unwrap(), p);
        assert_eq!(SecretKey::parse(&s.to_text()).unwrap().public(), p);
        assert_eq!(p.key_id().len(), 16);
        assert!(PublicKey::parse("backup-secret:AAAA").is_err());
        assert!(
            PublicKey::parse(&s.to_text()).is_err(),
            "a secret is not a public key"
        );
    }

    #[test]
    fn sets_are_checked() {
        let (_, keys) = holders(3);
        assert!(KeySet::new(vec![], 1).is_err());
        assert!(KeySet::new(keys.clone(), 0).is_err());
        assert!(KeySet::new(keys.clone(), 4).is_err());
        assert!(KeySet::new(vec![keys[0], keys[0]], 1).is_err());
        let set = KeySet::new(keys, 2).unwrap();
        assert_eq!(KeySet::from_json(&set.to_json()).unwrap(), set);
        let mut j = set.to_json();
        j.as_object_mut().unwrap().remove("threshold");
        assert_eq!(
            KeySet::from_json(&j).unwrap().threshold(),
            1,
            "0103 A3: 1 by default"
        );
    }

    // 0103 A3: with k = 1, any one key of the set restores, however many it holds.
    #[test]
    fn any_one_key_restores_by_default() {
        let (secrets, keys) = holders(3);
        let set = KeySet::new(keys, 1).unwrap();
        let (key, sealed) = seal_new(&set, "backup:1").unwrap();
        let blob = key.encrypt(b"private", b"the private section");
        let sealed = SealedKey::decode(&sealed.encode().unwrap()).unwrap();
        for s in &secrets {
            let share = Share::parse(&sealed.unseal(s).unwrap().to_text()).unwrap();
            let again = sealed.combine(&[share]).unwrap();
            assert_eq!(
                again.decrypt(b"private", &blob).unwrap(),
                b"the private section"
            );
        }
    }

    // 0103 A3: any k of the n keys restore; fewer do not.
    #[test]
    fn any_k_of_n_restore_and_fewer_do_not() {
        let (secrets, keys) = holders(5);
        let set = KeySet::new(keys, 3).unwrap();
        let (key, sealed) = seal_new(&set, "backup:2").unwrap();
        let blob = key.encrypt(b"aad", b"payload");
        let shares: Vec<Share> = secrets.iter().map(|s| sealed.unseal(s).unwrap()).collect();
        for picks in [[0, 1, 2], [0, 2, 4], [1, 3, 4], [4, 3, 2]] {
            let chosen: Vec<Share> = picks
                .iter()
                .map(|&i| Share::parse(&shares[i].to_text()).unwrap())
                .collect();
            let k = sealed.combine(&chosen).unwrap();
            assert_eq!(k.decrypt(b"aad", &blob).unwrap(), b"payload", "{picks:?}");
        }
        let two: Vec<Share> = [0, 1]
            .iter()
            .map(|&i| Share::parse(&shares[i].to_text()).unwrap())
            .collect();
        assert!(matches!(
            sealed.combine(&two),
            Err(BackupKeyError::Shares(_))
        ));
        let twice: Vec<Share> = [0, 0, 1]
            .iter()
            .map(|&i| Share::parse(&shares[i].to_text()).unwrap())
            .collect();
        assert!(
            sealed.combine(&twice).is_err(),
            "one holder's share twice counts once"
        );
    }

    #[test]
    fn a_stranger_or_another_backup_does_not_open() {
        let (secrets, keys) = holders(2);
        let set = KeySet::new(keys, 1).unwrap();
        let (_, one) = seal_new(&set, "backup:a").unwrap();
        let (_, other) = seal_new(&set, "backup:b").unwrap();
        assert_eq!(
            one.unseal(&SecretKey::generate()).unwrap_err(),
            BackupKeyError::NotHolder
        );
        // A share sealed for another backup, swapped in, does not open: the context is bound.
        let mut swapped = one.clone();
        swapped.shares[0] = other.shares[0].clone();
        assert_eq!(
            swapped.unseal(&secrets[0]).unwrap_err(),
            BackupKeyError::Open
        );
        // A share of another backup is refused when combining.
        let foreign = other.unseal(&secrets[0]).unwrap();
        assert!(one.combine(&[foreign]).is_err());
        // A changed byte does not open.
        let mut tampered = one.clone();
        let last = tampered.shares[1].sealed.len() - 1;
        tampered.shares[1].sealed[last] ^= 1;
        assert_eq!(
            tampered.unseal(&secrets[1]).unwrap_err(),
            BackupKeyError::Open
        );
    }

    // 0103 A3: re-sealing an older backup to a new set, by hand, with the old shares.
    #[test]
    fn a_backup_is_resealed_to_a_new_set() {
        let (old_secrets, old_keys) = holders(2);
        let (new_secrets, new_keys) = holders(3);
        let old = KeySet::new(old_keys, 2).unwrap();
        let (key, sealed) = seal_new(&old, "backup:9").unwrap();
        let blob = key.encrypt(b"", b"kept");
        let shares: Vec<Share> = old_secrets
            .iter()
            .map(|s| sealed.unseal(s).unwrap())
            .collect();
        let rebuilt = sealed.combine(&shares).unwrap();
        let resealed = seal_key(&KeySet::new(new_keys, 1).unwrap(), "backup:9", &rebuilt).unwrap();
        let k = resealed
            .combine(&[resealed.unseal(&new_secrets[2]).unwrap()])
            .unwrap();
        assert_eq!(k.decrypt(b"", &blob).unwrap(), b"kept");
        assert_eq!(
            resealed.unseal(&old_secrets[0]).unwrap_err(),
            BackupKeyError::NotHolder
        );
    }

    #[test]
    fn the_data_key_binds_its_place() {
        let key = DataKey::generate();
        let blob = key.encrypt(b"private.session", b"rows");
        assert_eq!(key.decrypt(b"private.session", &blob).unwrap(), b"rows");
        assert_eq!(
            key.decrypt(b"private.password", &blob).unwrap_err(),
            BackupKeyError::Open
        );
        assert_eq!(
            DataKey::generate()
                .decrypt(b"private.session", &blob)
                .unwrap_err(),
            BackupKeyError::Open
        );
    }
}
