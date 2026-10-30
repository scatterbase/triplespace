//! The wiki's Interface pages (0095): message overrides and names of IDs, as
//! `siprop=triplespace` reports them, `{{int:}}` reads them, and `meta=allmessages` lists
//! them beside the messages the build ships.
//!
//! The instance's pages are the primary tenant's (0046); a wiki's own page for the same
//! key and language replaces the instance's. Only pages anyone may read count (0095 §5).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use tokio_postgres::Client;

use crate::response::ApiError;

/// The site's interface messages in English, shipped with the build, which
/// `meta=allmessages` reports as the defaults (0095 §8).
const SHIPPED_EN: &str = include_str!("../../../i18n/en.json");

/// The wiki's Interface pages anyone may read, as `(title, text)` with the title inside
/// the namespace: the instance's first, then the wiki's own, so that a later one replaces
/// an earlier one for the same key and language.
///
/// # Errors
///
/// The database's.
pub async fn pages(db: &Client, tenant: &str) -> Result<Vec<(String, String)>, ApiError> {
    let primary = triplespace_projections::tenants::primary(db)
        .await
        .map_err(ApiError::internal)?;
    let mut pages = Vec::new();
    if let Some(p) = primary.filter(|p| p != tenant) {
        pages.extend(
            triplespace_projections::pages::interface_pages(db, &p)
                .await
                .map_err(ApiError::internal)?,
        );
    }
    pages.extend(
        triplespace_projections::pages::interface_pages(db, tenant)
            .await
            .map_err(ApiError::internal)?,
    );
    Ok(pages)
}

/// An Interface page's title read as the message key it overrides and the language it is
/// for, `""` for the base page (0095 §2, §3): `Ts-new/de` is `ts-new` in German, and
/// `Name/instance:scatter` names that ID in every language. The first letter is folded
/// back to lower case, as MediaWiki reads `MediaWiki:Mainpage` as `mainpage`.
#[must_use]
pub fn key(title: &str) -> (String, String) {
    let mut chars = title.chars();
    let key: String = chars
        .next()
        .map(|c| c.to_lowercase().chain(chars).collect())
        .unwrap_or_default();
    // An ID has no `/`, so after `name/` only a language can follow one.
    let split = if let Some(rest) = key.strip_prefix("name/") {
        rest.rsplit_once('/')
            .filter(|(_, l)| language(l))
            .map(|(id, l)| (format!("name/{id}"), l.to_string()))
    } else {
        key.rsplit_once('/')
            .filter(|(_, l)| language(l))
            .map(|(k, l)| (k.to_string(), l.to_string()))
    };
    split.unwrap_or((key, String::new()))
}

/// Whether a subpage name reads as a language code: two or three lower-case letters, then
/// subtags of letters and digits (`de`, `zh-hant`, `sr-ec`).
fn language(s: &str) -> bool {
    let mut parts = s.split('-');
    parts
        .next()
        .is_some_and(|p| (2..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase()))
        && parts
            .all(|p| !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// A wiki's overrides: by key, then language (`""` for the base page), the text and the
/// page's title.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    by_key: BTreeMap<String, BTreeMap<String, (String, String)>>,
}

impl Overrides {
    /// From [`pages`]: a later page for the same key and language replaces an earlier one.
    #[must_use]
    pub fn from_pages(pages: &[(String, String)]) -> Self {
        let mut by_key: BTreeMap<String, BTreeMap<String, (String, String)>> = BTreeMap::new();
        for (title, text) in pages {
            let (key, lang) = key(title);
            by_key
                .entry(key)
                .or_default()
                .insert(lang, (text.trim_end().to_string(), title.clone()));
        }
        Self { by_key }
    }

    /// The override of a message in a language, with the title of the page that gives it:
    /// the language's own page, or the base page where the language is the wiki's
    /// (`content_lang`), as MediaWiki reads its namespace (0095 §2).
    #[must_use]
    pub fn message(&self, key: &str, lang: &str, content_lang: &str) -> Option<(&str, &str)> {
        let by_lang = self.by_key.get(key)?;
        by_lang
            .get(lang)
            .filter(|_| !lang.is_empty() && lang != content_lang)
            .or_else(|| {
                (lang == content_lang)
                    .then(|| by_lang.get(lang).or_else(|| by_lang.get("")))
                    .flatten()
            })
            .map(|(t, title)| (t.as_str(), title.as_str()))
    }

    /// Every key the wiki overrides in some language.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.by_key.keys()
    }

    /// `siprop=triplespace`'s `interface`: `{messages: {key: {language: text}}}`.
    #[must_use]
    pub fn report(&self) -> Value {
        let messages: Map<String, Value> = self
            .by_key
            .iter()
            .map(|(k, by_lang)| {
                (
                    k.clone(),
                    Value::Object(
                        by_lang
                            .iter()
                            .map(|(l, (t, _))| (l.clone(), json!(t)))
                            .collect(),
                    ),
                )
            })
            .collect();
        json!({ "messages": messages })
    }
}

/// The messages the build ships, in English: the site's interface messages.
fn shipped() -> &'static BTreeMap<String, String> {
    static SHIPPED: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    SHIPPED.get_or_init(|| {
        let v: Map<String, Value> = serde_json::from_str(SHIPPED_EN).unwrap_or_default();
        v.into_iter()
            .filter(|(k, _)| !k.starts_with('@'))
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect()
    })
}

/// `$1`, `$2`, … in a message replaced by its arguments, as MediaWiki's `amargs` and
/// `{{int:}}` do.
#[must_use]
pub fn with_args(text: &str, args: &[&str]) -> String {
    let mut s = text.to_string();
    for (i, a) in args.iter().enumerate().rev() {
        s = s.replace(&format!("${}", i + 1), a);
    }
    s
}

/// What `meta=allmessages` was asked.
#[derive(Debug, Clone, Default)]
pub struct AllMessages {
    /// `ammessages`: the keys, or `*` for every one.
    pub messages: Vec<String>,
    /// `amlang`: the language; the wiki's when empty.
    pub lang: String,
    /// `amcustomised`: `all`, `modified` or `unmodified`.
    pub customised: String,
    /// `amprefix`.
    pub prefix: String,
    /// `amfilter`: a substring of the key.
    pub filter: String,
    /// `amfrom` and `amto`, inclusive.
    pub from: String,
    /// `amto`.
    pub to: String,
    /// `amargs`.
    pub args: Vec<String>,
    /// `amnocontent`.
    pub nocontent: bool,
    /// `amincludelocal`: with `*`, keys only the wiki has.
    pub includelocal: bool,
    /// `amprop=default`: each message's shipped text too, as `default` (or
    /// `defaultmissing`), as MediaWiki gives it.
    pub default: bool,
}

/// `meta=allmessages` (0095 §8): the messages asked for, each the wiki's override where it
/// has one in the language and the shipped message otherwise, marked `customised` when
/// overridden and `missing` when neither has it. Names of IDs are overrides like any
/// other. Shipped messages are the site's, in English.
#[must_use]
pub fn allmessages(
    o: &Overrides,
    ask: &AllMessages,
    content_lang: &str,
    formatversion: u8,
) -> Vec<Value> {
    let lang = if ask.lang.is_empty() {
        content_lang
    } else {
        ask.lang.as_str()
    };
    let shipped = shipped();
    let all = ask.messages.is_empty() || ask.messages.iter().any(|m| m == "*");
    let mut keys: Vec<String> = if all {
        let mut k: Vec<String> = shipped.keys().cloned().collect();
        if ask.includelocal {
            k.extend(o.keys().filter(|k| !shipped.contains_key(*k)).cloned());
        }
        k.sort();
        k.dedup();
        k
    } else {
        // A key is read as MediaWiki reads it, with its first letter in lower case.
        ask.messages.iter().map(|m| key(m).0).collect()
    };
    keys.retain(|k| {
        k.starts_with(&ask.prefix)
            && k.contains(&ask.filter)
            && (ask.from.is_empty() || k.as_str() >= ask.from.as_str())
            && (ask.to.is_empty() || k.as_str() <= ask.to.as_str())
    });
    let args: Vec<&str> = ask.args.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for k in keys {
        let over = o.message(&k, lang, content_lang).map(|(t, _)| t);
        // The site's messages ship in English; another language falls back to them.
        let default = shipped.get(&k).map(String::as_str);
        let customised = over.is_some();
        match ask.customised.as_str() {
            "modified" if !customised => continue,
            "unmodified" if customised => continue,
            _ => {}
        }
        let mut m = Map::new();
        m.insert("name".into(), json!(k));
        m.insert("normalizedname".into(), json!(k));
        match over.or(default) {
            Some(text) => {
                if !ask.nocontent {
                    let text = with_args(text, &args);
                    if formatversion == 2 {
                        m.insert("content".into(), json!(text));
                    } else {
                        m.insert("*".into(), json!(text));
                    }
                }
                if customised {
                    m.insert(
                        "customised".into(),
                        if formatversion == 2 {
                            json!(true)
                        } else {
                            json!("")
                        },
                    );
                }
            }
            None => {
                m.insert(
                    "missing".into(),
                    if formatversion == 2 {
                        json!(true)
                    } else {
                        json!("")
                    },
                );
            }
        }
        if ask.default {
            match default {
                Some(text) => {
                    m.insert("default".into(), json!(with_args(text, &args)));
                }
                None => {
                    m.insert(
                        "defaultmissing".into(),
                        if formatversion == 2 {
                            json!(true)
                        } else {
                            json!("")
                        },
                    );
                }
            }
        }
        out.push(Value::Object(m));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_read_as_keys_and_languages() {
        assert_eq!(key("Ts-new"), ("ts-new".into(), String::new()));
        assert_eq!(key("Ts-new/de"), ("ts-new".into(), "de".into()));
        assert_eq!(key("Ts-new/zh-hant"), ("ts-new".into(), "zh-hant".into()));
        assert_eq!(
            key("Name/instance:scatter"),
            ("name/instance:scatter".into(), String::new())
        );
        assert_eq!(
            key("Name/wwquhzy:25/fr"),
            ("name/wwquhzy:25".into(), "fr".into())
        );
        assert_eq!(
            key("Sidebar/Not a language"),
            ("sidebar/Not a language".into(), String::new())
        );
    }

    #[test]
    fn the_tenant_s_page_replaces_the_instance_s() {
        let o = Overrides::from_pages(&[
            ("Name/instance:x".into(), "Instance name".into()),
            ("Ts-new/de".into(), "Neu".into()),
            ("Name/instance:x".into(), "Our name\n".into()),
        ]);
        let v = o.report();
        assert_eq!(v["messages"]["name/instance:x"][""], "Our name");
        assert_eq!(v["messages"]["ts-new"]["de"], "Neu");
        assert_eq!(o.message("ts-new", "de", "en"), Some(("Neu", "Ts-new/de")));
        assert_eq!(o.message("ts-new", "en", "en"), None);
        assert_eq!(
            o.message("name/instance:x", "en", "en").map(|m| m.0),
            Some("Our name")
        );
    }

    #[test]
    fn allmessages_marks_overrides_and_missing_keys() {
        let o = Overrides::from_pages(&[
            ("Ts-new".into(), "Create $1".into()),
            ("Sidebar".into(), "* navigation".into()),
        ]);
        let ask = AllMessages {
            messages: vec!["ts-new".into(), "Ts-login".into(), "nope".into()],
            args: vec!["it".into()],
            ..AllMessages::default()
        };
        let v = allmessages(&o, &ask, "en", 2);
        assert_eq!(v[0]["content"], "Create it");
        assert_eq!(v[0]["customised"], true);
        assert_eq!(v[1]["name"], "ts-login");
        assert_eq!(v[1]["content"], "Log in");
        assert!(v[1].get("customised").is_none());
        assert_eq!(v[2]["missing"], true);
        let modified = AllMessages {
            messages: vec!["*".into()],
            customised: "modified".into(),
            includelocal: true,
            ..AllMessages::default()
        };
        let v = allmessages(&o, &modified, "en", 1);
        let names: Vec<&str> = v.iter().filter_map(|m| m["name"].as_str()).collect();
        assert_eq!(names, ["sidebar", "ts-new"]);
        assert_eq!(v[0]["*"], "* navigation");
        assert_eq!(v[0]["customised"], "");
        assert!(v[0].get("default").is_none() && v[0].get("defaultmissing").is_none());

        // `amprop=default`: the shipped text beside the override, or that there is none.
        let with_default = AllMessages {
            messages: vec!["ts-new".into(), "sidebar".into()],
            default: true,
            ..AllMessages::default()
        };
        let v = allmessages(&o, &with_default, "en", 2);
        assert_eq!(v[0]["content"], "Create $1");
        assert_ne!(v[0]["default"], "Create $1");
        assert!(v[0]["default"].is_string());
        assert_eq!(v[1]["content"], "* navigation");
        assert_eq!(v[1]["defaultmissing"], true);
    }
}
