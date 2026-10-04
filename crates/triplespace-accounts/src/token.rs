//! CSRF and login tokens (mediawiki-compat §2.5; 0056 §1): derived, not stored. A token is
//! `HMAC-SHA256(instance secret, "{purpose}:{binding}")`, URL-safe base64, 32 characters,
//! then MediaWiki's `+\` suffix. The binding is the session ID for a cookie session and
//! the key ID for a stateless bearer request, so a token is useless outside the session or
//! key it was issued to. An anonymous request's CSRF token is the constant `+\`.

use base64::Engine as _;
use hmac::{Hmac, Mac as _};
use sha2::Sha256;

/// What an anonymous session uses for a CSRF token.
pub const ANONYMOUS: &str = r"+\";

/// The instance secret tokens derive from: 32 bytes the server reads from its key file.
#[derive(Clone)]
pub struct Secret([u8; 32]);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

impl Secret {
    /// From the instance signing key's seed, through a domain-separating hash, so the
    /// signing key itself is never used as an HMAC key.
    #[must_use]
    pub fn derive(seed: &[u8]) -> Self {
        use sha2::Digest as _;
        let mut h = Sha256::new();
        h.update(b"triplespace-accounts:tokens:v1:");
        h.update(seed);
        Self(h.finalize().into())
    }

    /// From raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn mac(&self, purpose: &str, binding: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("any key length");
        mac.update(purpose.as_bytes());
        mac.update(b":");
        mac.update(binding.as_bytes());
        let out = mac.finalize().into_bytes();
        let mut text = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(out);
        text.truncate(32);
        text.push_str(ANONYMOUS);
        text
    }

    /// A token of a type (`csrf`, `login`, …) bound to a session ID or key ID.
    #[must_use]
    pub fn token(&self, token_type: &str, binding: &str) -> String {
        self.mac(token_type, binding)
    }

    /// Whether a presented token is the one for this binding.
    #[must_use]
    pub fn check(&self, token_type: &str, binding: &str, presented: &str) -> bool {
        let expected = self.mac(token_type, binding);
        expected.len() == presented.len()
            && expected
                .bytes()
                .zip(presented.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_bind_to_purpose_and_session() {
        let s = Secret::derive(b"seed");
        let t = s.token("csrf", "session-1");
        assert!(t.ends_with(r"+\"));
        assert_eq!(t.len(), 34);
        assert!(s.check("csrf", "session-1", &t));
        assert!(!s.check("login", "session-1", &t));
        assert!(!s.check("csrf", "session-2", &t));
        assert_ne!(Secret::derive(b"other").token("csrf", "session-1"), t);
    }
}
