//! Property tests of the normalizers (22 §4.9; 0033 §15): a normalizer is idempotent. A
//! key it produces is a key it accepts unchanged, so the view's comparisons (0004 §7),
//! keyed IDs (0009 §1–2), resolver keys (0029 §1), notation keys (0048 §2) and sitelink
//! URLs (0026 §1) never depend on how many times a value went through.
//!
//! Each normalizer sees two kinds of input: arbitrary printable text, which mostly ends
//! in an error that must be the same the second time, and text shaped to reach the
//! accepting path.

use proptest::prelude::*;
use scatter_normalize::resolver::{doi, is_doi};
use scatter_normalize::url::{
    DEFAULT_SCHEMES, is_normalized, normalize_percent_encoding, normalize_url,
};
use scatter_normalize::{
    KeyedRegistry, ResolverRegistry, casefold, domain, is_nfc, keyword, nfc, notation,
};

/// Arbitrary printable text, a few dozen characters.
fn text() -> impl Strategy<Value = String> {
    "\\PC{0,40}"
}

fn domain_like() -> impl Strategy<Value = String> {
    "[A-Za-z0-9äöüß-]{1,12}(\\.[A-Za-z0-9-]{1,10}){0,3}\\.?"
}

fn keyword_like() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 _\\-ÄÖÜé]{1,30}"
}

fn doi_like() -> impl Strategy<Value = String> {
    "(doi:|https://doi\\.org/|DOI\\.ORG/)?10\\.[0-9]{3,10}/[A-Za-z0-9%()./_-]{0,20} ?"
}

fn url_like() -> impl Strategy<Value = String> {
    "(https?|ftp)://[a-zA-Z0-9.-]{1,16}(:[0-9]{1,5})?(/[a-zA-Z0-9%._~!$&'()*+,;=:@-]{0,12}){0,3}/?(\\?[a-z=&%0-9]{0,12})?(#[a-zA-Z0-9%]{0,8})?"
}

fn notation_like() -> impl Strategy<Value = String> {
    "(osm|OSM|nope):[a-z_:]{0,12}(=[A-Za-z0-9 _:-]{0,12})?"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// 0004 §7: NFC is idempotent, and so is case folding (0017 §5; 0029 §1).
    #[test]
    fn nfc_and_casefold_are_idempotent(s in text()) {
        let n = nfc(&s);
        prop_assert!(is_nfc(&n));
        prop_assert_eq!(nfc(&n), n);
        let f = casefold(&s);
        prop_assert_eq!(casefold(&f), f);
    }

    /// 0009 §1–2: a Domain key normalizes to itself.
    #[test]
    fn domain_is_idempotent(s in prop_oneof![text(), domain_like()]) {
        if let Ok(key) = domain::normalize(&s) {
            prop_assert!(domain::is_key(&key), "{}", key);
            prop_assert_eq!(domain::normalize(&key), Ok(key));
        }
    }

    /// 0009 §1–2: a Keyword key normalizes to itself.
    #[test]
    fn keyword_is_idempotent(s in prop_oneof![text(), keyword_like()]) {
        if let Ok(key) = keyword::normalize(&s) {
            prop_assert!(keyword::is_key(&key), "{}", key);
            prop_assert_eq!(keyword::normalize(&key), Ok(key));
        }
    }

    /// 0009 §1–2: every registered keyed type's normalizer is idempotent.
    #[test]
    fn keyed_types_are_idempotent(s in prop_oneof![text(), domain_like(), keyword_like(), notation_like()]) {
        for t in KeyedRegistry::default_registry().types() {
            if let Ok(key) = t.normalize(&s) {
                prop_assert!(t.is_key(&key), "{}: {}", t.name, key);
                let again = t.normalize(&key);
                prop_assert_eq!(again.as_deref(), Ok(key.as_str()), "{}", t.name);
            }
        }
    }

    /// 0029 §1: the DOI normalizer, before the case rule, is idempotent.
    #[test]
    fn doi_is_idempotent(s in prop_oneof![text(), doi_like()]) {
        if let Ok(key) = doi(&s) {
            prop_assert!(is_doi(&key), "{}", key);
            prop_assert_eq!(doi(&key), Ok(key));
        }
    }

    /// 0029 §1: every default resolver with a normalizer is idempotent, case rule
    /// included.
    #[test]
    fn resolvers_are_idempotent(s in prop_oneof![text(), doi_like(), url_like()]) {
        for r in ResolverRegistry::default_registry().resolvers() {
            if let Ok(key) = r.normalize(&s) {
                let again = r.normalize(&key);
                prop_assert_eq!(again.as_deref(), Ok(key.as_str()), "{}", r.name);
            }
        }
    }

    /// 0026 §1: the URL normalizer is idempotent, and so is its percent-encoding step.
    #[test]
    fn url_is_idempotent(s in prop_oneof![text(), url_like()]) {
        if let Ok(key) = normalize_url(&s, DEFAULT_SCHEMES) {
            prop_assert!(is_normalized(&key), "{}", key);
            prop_assert_eq!(normalize_url(&key, DEFAULT_SCHEMES), Ok(key));
        }
        let p = normalize_percent_encoding(&s);
        prop_assert_eq!(normalize_percent_encoding(&p), p);
    }

    /// 0048 §2: a notation key normalizes to itself.
    #[test]
    fn notation_is_idempotent(s in prop_oneof![text(), notation_like()]) {
        if let Ok(key) = notation::normalize(&s) {
            prop_assert_eq!(notation::normalize(&key), Ok(key));
        }
    }
}
