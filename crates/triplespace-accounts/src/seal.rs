//! Sealing secrets at rest (0088 §3): XChaCha20-Poly1305 under the **instance sealing
//! key**, a subkey of the instance secret with its own domain separator (`ts-seal-v1`), as
//! the CSRF and challenge keys are derived. A sealed value is a 24-byte random nonce
//! followed by the ciphertext; the associated data names where the value lives (the
//! table, and the row's tenant and issuer or owner), so a value copied to another row
//! does not open. Rotating the signing seed makes every sealed value unreadable, and the
//! operator sets them again.

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::{AccountsError, Secret};

/// The domain separator of the sealing key.
pub const DOMAIN: &str = "ts-seal-v1";

const NONCE: usize = 24;

fn cipher(secret: &Secret) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&secret.subkey(DOMAIN).into())
}

fn aad(parts: &[&str]) -> Vec<u8> {
    parts.join("\0").into_bytes()
}

/// Seals a value for the place `aad` names.
///
/// # Errors
///
/// If the random source fails.
pub fn seal(secret: &Secret, place: &[&str], plaintext: &[u8]) -> Result<Vec<u8>, AccountsError> {
    let mut nonce = [0u8; NONCE];
    getrandom::fill(&mut nonce).map_err(|e| AccountsError::Random(e.to_string()))?;
    let aad = aad(place);
    let ct = cipher(secret)
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| AccountsError::Invalid("sealing failed".into()))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Opens a value sealed for the place `aad` names.
///
/// # Errors
///
/// [`AccountsError::Sealed`] when it does not open: another key, another place, or
/// damaged bytes.
pub fn open(secret: &Secret, place: &[&str], sealed: &[u8]) -> Result<Vec<u8>, AccountsError> {
    if sealed.len() < NONCE {
        return Err(AccountsError::Sealed);
    }
    let (nonce, ct) = sealed.split_at(NONCE);
    let nonce: [u8; NONCE] = nonce.try_into().map_err(|_| AccountsError::Sealed)?;
    let aad = aad(place);
    cipher(secret)
        .decrypt(&XNonce::from(nonce), Payload { msg: ct, aad: &aad })
        .map_err(|_| AccountsError::Sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens_only_in_place() {
        let k = Secret::from_bytes([3; 32]);
        let place = ["private.oauth_client", "", "wikimedia-central"];
        let s = seal(&k, &place, b"s3cret").unwrap();
        assert_ne!(&s[NONCE..], b"s3cret");
        assert_eq!(open(&k, &place, &s).unwrap(), b"s3cret");
        assert_ne!(seal(&k, &place, b"s3cret").unwrap(), s, "a fresh nonce");
        let elsewhere = ["private.oauth_client", "other", "wikimedia-central"];
        assert!(matches!(
            open(&k, &elsewhere, &s),
            Err(AccountsError::Sealed)
        ));
        let other_key = Secret::from_bytes([4; 32]);
        assert!(matches!(
            open(&other_key, &place, &s),
            Err(AccountsError::Sealed)
        ));
        assert!(matches!(
            open(&k, &place, &s[..10]),
            Err(AccountsError::Sealed)
        ));
    }
}
