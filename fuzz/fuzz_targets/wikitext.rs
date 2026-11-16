//! The wikitext parser and renderer over arbitrary text (22 §4.9; 0033 §9.1, §15): any
//! UTF-8 input must render and extract without a panic, inside the parser's own time
//! bound (`PARSE_TIMEOUT`, five seconds; the run's `-timeout` is set above it in
//! `run.sh`). Nothing else is asserted: what the HTML looks like is the golden tests'.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use scatter_wikitext::html::url_encode_title;
use scatter_wikitext::{Config, ConfigSource, Interwiki, Namespace, PageInfo, Resolver, Title};

/// A resolver that answers from the title alone, so that both the existing and the
/// missing branches, redirects and entity labels are rendered.
struct Fixture;

impl Resolver for Fixture {
    fn info(&self, t: &Title) -> PageInfo {
        PageInfo {
            exists: t.text.len().is_multiple_of(2),
            redirect: t.text.starts_with('R'),
            redirect_to: t
                .text
                .starts_with('R')
                .then(|| format!("Target of {}", t.text)),
            label: (t.namespace == 120).then(|| format!("label of {}", t.text)),
        }
    }
    fn url(&self, t: &Title) -> String {
        format!("/wiki/{}", url_encode_title(&t.text))
    }
    fn missing_url(&self, t: &Title) -> String {
        format!(
            "/w/index.php?title={}&action=edit",
            url_encode_title(&t.text)
        )
    }
}

fn config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let mut s = ConfigSource::default();
        s.namespaces.push(Namespace::new(120, "Item"));
        s.namespaces.push(Namespace::new(122, "Property"));
        s.namespaces.push(Namespace::new(828, "Module"));
        s.interwiki.push(Interwiki {
            prefix: "wikipedia".into(),
            url: "https://en.wikipedia.org/wiki/$1".into(),
        });
        Config::new(s)
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The first byte picks the page the text is rendered as: a File page and a subpage
    // reach the link and image rules a main-namespace page does not.
    let page = match data.first().map(|b| b % 4) {
        Some(1) => Title::new(6, "Fuzz.png"),
        Some(2) => Title::new(2, "Fuzz/Sub"),
        Some(3) => Title::new(120, "Q1"),
        _ => Title::new(0, "Fuzz"),
    };
    let _ = scatter_wikitext::render(text, config(), &Fixture, &page);
    let _ = scatter_wikitext::extract(text, config(), &page);
});
