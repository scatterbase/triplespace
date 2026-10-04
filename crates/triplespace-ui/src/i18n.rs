//! Interface messages (0034 §9): banana JSON from `i18n/`, the files translatewiki.net
//! reads, embedded in the build.
//!
//! The interface language is `uselang` where the request gives a valid code, else the
//! site's language; the `language` preference (0027) joins when the API reports it. A
//! message is looked up along the language's fallback chain and ends at English. The
//! direction follows the language, not the messages, so `uselang=ar` is right to left
//! even while Arabic has no translations.
//!
//! Messages support `$1`…`$9`, `{{PLURAL:…}}` with CLDR plural rules (ICU4X), and
//! `{{GENDER:…}}` and `{{GRAMMAR:…}}` in their degenerate forms (the neutral form; the
//! word unchanged) until a message needs more. This is the small in-house implementation
//! 0034 §9 allows: the `banana-i18n` crate (0.1.0) parses messages with tree-sitter, a C
//! library, which the site does not otherwise need, for features it does not use yet.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules};

#[derive(rust_embed::Embed)]
#[folder = "../../i18n"]
#[include = "*.json"]
#[exclude = "qqq.json"]
struct Files;

type Catalogue = Arc<BTreeMap<String, String>>;

fn parse(bytes: &[u8]) -> Catalogue {
    let v: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(bytes).unwrap_or_default();
    Arc::new(
        v.into_iter()
            .filter(|(k, _)| !k.starts_with('@'))
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect(),
    )
}

static EN: LazyLock<Catalogue> = LazyLock::new(|| {
    parse(
        &Files::get("en.json")
            .expect("i18n/en.json is embedded")
            .data,
    )
});

static LOADED: LazyLock<Mutex<BTreeMap<String, Option<Catalogue>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// The catalogue of one language, if `i18n/` has it.
fn catalogue(lang: &str) -> Option<Catalogue> {
    if lang == "en" {
        return Some(EN.clone());
    }
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loaded
        .entry(lang.to_string())
        .or_insert_with(|| Files::get(&format!("{lang}.json")).map(|f| parse(&f.data)))
        .clone()
}

/// Languages written right to left (MediaWiki's `$rtl` languages, by their base code).
const RTL: &[&str] = &[
    "ar", "arc", "arq", "ary", "arz", "azb", "bcc", "bgn", "bqi", "ckb", "dv", "fa", "glk", "he",
    "khw", "kk-arab", "kk-cn", "ks", "ku-arab", "lki", "luz", "lrc", "mzn", "nqo", "pnb", "ps",
    "sd", "sdh", "skr", "ug", "ur", "yi",
];

/// Whether a language code is written right to left.
#[must_use]
pub fn is_rtl(lang: &str) -> bool {
    RTL.contains(&lang)
        || lang
            .split('-')
            .next()
            .is_some_and(|base| RTL.contains(&base))
}

/// A language code as MediaWiki accepts it in `uselang`: lower-case letters, then
/// subtags of letters and digits; `None` for anything else.
#[must_use]
pub fn valid_code(code: &str) -> Option<String> {
    let c = code.trim().to_ascii_lowercase();
    let mut parts = c.split('-');
    let first = parts.next()?;
    let ok_first = (2..=3).contains(&first.len()) && first.bytes().all(|b| b.is_ascii_lowercase());
    let ok_rest =
        parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()));
    (ok_first && ok_rest && c.len() <= 35).then_some(c)
}

/// A language's fallback chain, itself first: each shorter prefix of its code, then
/// English. MediaWiki's explicit fallbacks (`de-at` → `de`, `pt-br` → `pt`) are the
/// prefixes; its few cross-language ones (`gsw` → `de`) come with their translations.
#[must_use]
pub fn chain(lang: &str) -> Vec<String> {
    let mut out = vec![lang.to_string()];
    let mut l = lang;
    while let Some((head, _)) = l.rsplit_once('-') {
        out.push(head.to_string());
        l = head;
    }
    if !out.iter().any(|x| x == "en") {
        out.push("en".to_string());
    }
    out
}

/// The languages a term (label, description, alias) is looked up in, in order: the
/// interface language's chain with `mul` before English, or after it for a reader
/// whose language is English (0009 §5).
#[must_use]
pub fn term_chain(lang: &str) -> Vec<String> {
    let mut out = chain(lang);
    // `mul` goes before the English that ends every chain, unless English is the
    // reader's own language.
    if lang == "en" || lang.starts_with("en-") {
        out.push("mul".to_string());
    } else {
        out.insert(out.len() - 1, "mul".to_string());
    }
    out
}

/// The messages of one interface language.
#[derive(Debug, Clone)]
pub struct Messages {
    lang: String,
    chain: Vec<String>,
}

impl Messages {
    /// The messages for a language code; English for one that is not valid.
    #[must_use]
    pub fn for_language(lang: &str) -> Self {
        let lang = valid_code(lang).unwrap_or_else(|| "en".to_string());
        Self {
            chain: chain(&lang),
            lang,
        }
    }

    /// The language code, for `<html lang>`.
    #[must_use]
    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// The writing direction, for `<html dir>`.
    #[must_use]
    pub fn dir(&self) -> &'static str {
        if is_rtl(&self.lang) { "rtl" } else { "ltr" }
    }

    /// The fallback chain for terms.
    #[must_use]
    pub fn term_chain(&self) -> Vec<String> {
        term_chain(&self.lang)
    }

    /// The raw message and the language it was found in.
    fn raw(&self, key: &str) -> Option<(String, String)> {
        self.chain
            .iter()
            .find_map(|l| catalogue(l).and_then(|c| c.get(key).map(|m| (m.clone(), l.clone()))))
    }

    /// A message, or `⧼key⧽` where it is missing, as MediaWiki shows one.
    #[must_use]
    pub fn get(&self, key: &str) -> String {
        self.with(key, &[])
    }

    /// A message with its parameters substituted for `$1`, `$2`, …, and its `PLURAL`,
    /// `GENDER` and `GRAMMAR` calls expanded.
    #[must_use]
    pub fn with(&self, key: &str, params: &[&str]) -> String {
        let Some((mut s, lang)) = self.raw(key) else {
            return format!("⧼{key}⧽");
        };
        for (i, p) in params.iter().enumerate().rev() {
            s = s.replace(&format!("${}", i + 1), p);
        }
        expand(&s, &lang)
    }
}

/// Expands `{{NAME:arg|form|…}}` calls, innermost first.
fn expand(s: &str, lang: &str) -> String {
    let mut s = s.to_string();
    // Each pass expands the calls that hold no other call.
    for _ in 0..8 {
        let Some(start) = innermost(&s) else { break };
        let Some(end) = s[start..].find("}}").map(|e| start + e) else {
            break;
        };
        let inner = &s[start + 2..end];
        let replaced = call(inner, lang).unwrap_or_else(|| inner.to_string());
        s.replace_range(start..end + 2, &replaced);
    }
    s
}

/// The start of a `{{` whose call holds no other `{{` before its `}}`.
fn innermost(s: &str) -> Option<usize> {
    let mut best = None;
    let mut i = 0;
    while let Some(p) = s[i..].find("{{") {
        let at = i + p;
        let close = s[at..].find("}}").map(|c| at + c)?;
        if !s[at + 2..close].contains("{{") {
            best = Some(at);
            break;
        }
        i = at + 2;
    }
    best
}

fn call(inner: &str, lang: &str) -> Option<String> {
    let (name, rest) = inner.split_once(':')?;
    let mut args = rest.split('|');
    let first = args.next().unwrap_or_default().trim();
    let forms: Vec<&str> = args.collect();
    match name.trim().to_ascii_uppercase().as_str() {
        "PLURAL" => Some(plural(first, &forms, lang)),
        // The viewer's gender is not known to the site yet: the neutral (last) form.
        "GENDER" => Some(forms.last().copied().unwrap_or_default().to_string()),
        "GRAMMAR" => Some(forms.first().copied().unwrap_or_default().to_string()),
        _ => None,
    }
}

/// `{{PLURAL:n|form|…}}`: an explicit `n=form` first, then the form for the number's CLDR
/// category, in the order the language's categories come (zero, one, two, few, many),
/// with the last form for `other` and for any form not given.
fn plural(number: &str, forms: &[&str], lang: &str) -> String {
    let digits: String = number
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .collect();
    for f in forms {
        if let Some((n, text)) = f.split_once('=')
            && n.trim() == digits
        {
            return text.to_string();
        }
    }
    let plain: Vec<&str> = forms
        .iter()
        .copied()
        .filter(|f| {
            f.split_once('=')
                .is_none_or(|(n, _)| n.trim().parse::<f64>().is_err())
        })
        .collect();
    let Some(last) = plain.last() else {
        return String::new();
    };
    // Whole numbers by their rules; a fraction (rare in a message) takes `other`.
    let Ok(n) = digits.trim_start_matches(['-', '+']).parse::<u64>() else {
        return (*last).to_string();
    };
    let operands = icu_plurals::PluralOperands::from(n);
    let rules = rules_for(lang);
    let category = rules.category_for(operands);
    if category == PluralCategory::Other {
        return (*last).to_string();
    }
    let index = rules
        .categories()
        .filter(|c| *c != PluralCategory::Other)
        .position(|c| c == category)
        .unwrap_or(usize::MAX);
    plain.get(index).copied().unwrap_or(last).to_string()
}

fn rules_for(lang: &str) -> PluralRules {
    let locale: Locale = lang
        .parse()
        .or_else(|_| lang.split('-').next().unwrap_or("en").parse())
        .unwrap_or_else(|_| "en".parse().expect("en parses"));
    PluralRules::try_new_cardinal((&locale).into())
        .or_else(|_| {
            PluralRules::try_new_cardinal((&"en".parse::<Locale>().expect("en parses")).into())
        })
        .expect("English plural rules are compiled in")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_parameters() {
        let m = Messages::for_language("en");
        assert_eq!(m.get("ts-login"), "Log in");
        assert_eq!(
            m.with("ts-account-menu", &["Alice"]),
            "Account menu for Alice"
        );
        assert_eq!(m.get("ts-no-such-message"), "⧼ts-no-such-message⧽");
    }

    #[test]
    fn plural_forms() {
        assert_eq!(plural("1", &["$1 value", "values"], "en"), "$1 value");
        assert_eq!(plural("2", &["value", "values"], "en"), "values");
        assert_eq!(plural("0", &["value", "values", "0=none"], "en"), "none");
        assert_eq!(plural("1,000", &["value", "values"], "en"), "values");
        // Russian: one, few, many, other.
        assert_eq!(
            plural("1", &["значение", "значения", "значений"], "ru"),
            "значение"
        );
        assert_eq!(
            plural("3", &["значение", "значения", "значений"], "ru"),
            "значения"
        );
        assert_eq!(
            plural("5", &["значение", "значения", "значений"], "ru"),
            "значений"
        );
        assert_eq!(
            expand("{{PLURAL:2|one|two}} and {{GENDER:x|he|she|they}}", "en"),
            "two and they"
        );
    }

    #[test]
    fn languages_chains_and_direction() {
        assert_eq!(chain("de-at"), vec!["de-at", "de", "en"]);
        assert_eq!(term_chain("de"), vec!["de", "mul", "en"]);
        assert_eq!(term_chain("en"), vec!["en", "mul"]);
        assert_eq!(term_chain("en-gb"), vec!["en-gb", "en", "mul"]);
        assert_eq!(Messages::for_language("ar").dir(), "rtl");
        assert_eq!(Messages::for_language("ku-arab").dir(), "rtl");
        assert_eq!(Messages::for_language("de").dir(), "ltr");
        assert_eq!(Messages::for_language("<script>").lang(), "en");
        assert_eq!(valid_code("pt-BR").as_deref(), Some("pt-br"));
        assert!(valid_code("e").is_none());
        // Arabic has no translations yet: English text, right to left.
        assert_eq!(Messages::for_language("ar").get("ts-login"), "Log in");
    }

    #[test]
    fn every_message_is_documented() {
        let qqq: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(include_str!("../../../i18n/qqq.json")).unwrap();
        for k in EN.keys() {
            assert!(qqq.contains_key(k), "{k} has no qqq.json entry");
        }
    }
}
