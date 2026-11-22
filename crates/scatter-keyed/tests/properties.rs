//! Property tests of the keyed types' normalizers (22 §4.9; 0033 §15): each is idempotent,
//! so keyed IDs (0009 §1–2), keyword keys (0017 §5) and notation keys (0048 §2) never depend
//! on how many times a value went through.

use proptest::prelude::*;
use scatter_keyed::{KeyedRegistry, keyword, notation};

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

fn notation_like() -> impl Strategy<Value = String> {
    "(osm|OSM|nope):[a-z_:]{0,12}(=[A-Za-z0-9 _:-]{0,12})?"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

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

    /// 0048 §2: a notation key normalizes to itself.
    #[test]
    fn notation_is_idempotent(s in prop_oneof![text(), notation_like()]) {
        if let Ok(key) = notation::normalize(&s) {
            prop_assert_eq!(notation::normalize(&key), Ok(key));
        }
    }
}
