//! Provider-ranged revision IDs (ADR 0015 §2).
//!
//! The revision-ID space is 63 bits, partitioned by provider number:
//! `revid = provider_number << 40 | n`. Provider number 0 is the instance itself, so local
//! revision IDs are the plain per-tenant sequence. For a provider that publishes revision
//! IDs, `n` is the upstream revision ID; for one that does not, `n` is the mirror record's
//! offset. The split is the same on every instance, so the ID is computed by the writer,
//! needs no allocation, and never changes on rebuild.

use crate::REVID_N_BITS;

/// The largest `n` a provider-ranged revision ID can carry: 2^40 − 1.
pub const MAX_N: u64 = (1 << REVID_N_BITS) - 1;

/// The largest provider number the 63-bit space admits: 2^23 − 1.
pub const MAX_PROVIDER_NUMBER: u32 = (1 << (63 - REVID_N_BITS)) - 1;

/// A revision ID could not be formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RevidError {
    /// `n` does not fit in 40 bits. 0015's open question: a provider near this limit is
    /// re-registered with a wider split, which is a format version.
    #[error("revision number {0} does not fit in {REVID_N_BITS} bits")]
    NTooLarge(u64),
    /// The provider number does not fit in the remaining 23 bits.
    #[error("provider number {0} exceeds {MAX_PROVIDER_NUMBER}")]
    ProviderTooLarge(u32),
}

/// Forms the revision ID `provider_number << 40 | n`.
///
/// ```
/// use scatter_providers::provider_revid;
/// assert_eq!(provider_revid(0, 900).unwrap(), 900);                 // local
/// assert_eq!(provider_revid(2, 900).unwrap(), (2 << 40) | 900);     // Librarybase
/// ```
pub fn provider_revid(provider_number: u32, n: u64) -> Result<u64, RevidError> {
    if n > MAX_N {
        return Err(RevidError::NTooLarge(n));
    }
    if provider_number > MAX_PROVIDER_NUMBER {
        return Err(RevidError::ProviderTooLarge(provider_number));
    }
    Ok((u64::from(provider_number) << REVID_N_BITS) | n)
}

/// Splits a revision ID into `(provider_number, n)`. Provider number 0 is the instance.
///
/// ```
/// use scatter_providers::split_revid;
/// assert_eq!(split_revid((1 << 40) | 2148573921), (1, 2148573921));
/// assert_eq!(split_revid(41877), (0, 41877));
/// ```
#[must_use]
pub fn split_revid(revid: u64) -> (u32, u64) {
    let number = revid >> REVID_N_BITS;
    // The space is 63 bits, so the high part fits in 23 bits.
    let number = u32::try_from(number).unwrap_or(u32::MAX);
    (number, revid & MAX_N)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for (p, n) in [
            (0, 0),
            (0, 1),
            (1, MAX_N),
            (8, 12_345),
            (MAX_PROVIDER_NUMBER, 7),
        ] {
            let r = provider_revid(p, n).unwrap();
            assert_eq!(split_revid(r), (p, n));
            assert!(r < (1 << 63), "stays in 63 bits");
        }
    }

    #[test]
    fn limits() {
        assert_eq!(
            provider_revid(1, MAX_N + 1),
            Err(RevidError::NTooLarge(MAX_N + 1))
        );
        assert_eq!(
            provider_revid(MAX_PROVIDER_NUMBER + 1, 1),
            Err(RevidError::ProviderTooLarge(MAX_PROVIDER_NUMBER + 1))
        );
    }

    #[test]
    fn local_ids_are_unchanged() {
        assert_eq!(provider_revid(0, 41877).unwrap(), 41877);
    }
}
