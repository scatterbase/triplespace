//! The Appearance menu (0085 §6; 19 §2.7) and the reader's appearance on `<html>`
//! (0086 §4; 19 §5.9).
//!
//! Every page's header has an **Appearance** disclosure, for every viewer, holding the
//! preference keys of the `appearance` group. Signed in, its values are the account's,
//! the server writes them on `<html>`, and the panel is a form the web tier saves
//! without JavaScript. Not logged in, the page is public and carries no choice: the head
//! script applies what the reader keeps in `localStorage` before the first paint, and
//! without JavaScript the panel says the page follows the device.
//!
//! The panel offers colour mode, contrast and text size, which are never withheld, and
//! the palettes and type sets the wiki lets readers choose (0086 §4), the wiki's own
//! first, as "Wiki default".

use std::fmt::Write as _;

use serde_json::{Map, Value, json};

use crate::html::esc;
use crate::i18n::Messages;
use crate::theme::{Sheet, Theme};

/// The keys that choose a stylesheet, whose choices are the wiki's.
pub const SHEET_KEYS: [(&str, Sheet); 2] = [
    ("ui.palette", Sheet::Palette),
    ("ui.typeset", Sheet::Typeset),
];

/// A reader's palette or type set where the wiki lets them choose it; empty for the
/// wiki's own.
fn sheet_value(
    theme: &Theme,
    options: Option<&Map<String, Value>>,
    key: &str,
    sheet: Sheet,
) -> String {
    let own = options
        .and_then(|o| o.get(key))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty());
    let chosen = theme.chosen(sheet, own);
    if own == Some(chosen.as_str()) && chosen != default_of(theme, sheet) {
        chosen
    } else {
        String::new()
    }
}

fn default_of(theme: &Theme, sheet: Sheet) -> &str {
    match sheet {
        Sheet::Palette => theme.default_palette(),
        Sheet::Typeset => theme.default_typeset(),
    }
}

/// The keys the panel offers, with their choices and defaults, as `preferences.toml`
/// registers them (a test holds the two together).
pub const KEYS: &[(&str, &[&str], &str)] = &[
    ("ui.mode", &["auto", "light", "dark"], "auto"),
    ("ui.contrast", &["auto", "more"], "auto"),
    (
        "ui.text-size",
        &["small", "medium", "large", "x-large"],
        "medium",
    ),
];

/// The browser storage entry a reader's choices are kept under, one per host.
pub const STORAGE_KEY: &str = "ts-appearance";

/// A value of one of [`KEYS`], or its default when the value is not one of its choices.
fn value_of<'a>(options: Option<&'a Map<String, Value>>, key: &str) -> &'a str {
    let (_, choices, default) =
        KEYS.iter()
            .find(|(k, _, _)| *k == key)
            .copied()
            .unwrap_or(("", &[], ""));
    options
        .and_then(|o| o.get(key))
        .and_then(Value::as_str)
        .filter(|v| choices.contains(v))
        .unwrap_or(default)
}

/// The attributes and classes `<html>` carries (19 §5.9): for a signed-in reader the
/// account's choices and `data-appearance="account"`, which tells the head script the
/// server has decided; for anyone else `skin-theme-clientpref-os` alone, so the page
/// stays the same for every reader and the head script applies their own.
#[must_use]
pub fn html_attributes(options: Option<&Map<String, Value>>) -> (String, String) {
    let Some(options) = options else {
        return ("skin-theme-clientpref-os".into(), String::new());
    };
    let mode = value_of(Some(options), "ui.mode");
    let class = match mode {
        "dark" => "skin-theme-clientpref-night",
        "light" => "skin-theme-clientpref-day",
        _ => "skin-theme-clientpref-os",
    };
    let mut attrs = String::from(" data-appearance=\"account\"");
    if mode != "auto" {
        let _ = write!(attrs, " data-mode=\"{}\"", esc(mode));
    }
    if value_of(Some(options), "ui.contrast") == "more" {
        attrs.push_str(" data-contrast=\"more\"");
    }
    let _ = write!(
        attrs,
        " data-text-size=\"{}\"",
        esc(value_of(Some(options), "ui.text-size"))
    );
    (class.into(), attrs)
}

/// The data block the site's script reads: whether the reader is signed in, the
/// account's values, which keys the account holds itself, and the palettes and type sets
/// a reader may choose.
#[must_use]
pub fn data(theme: &Theme, options: Option<&Map<String, Value>>, own: Option<&[String]>) -> String {
    let mut values: Map<String, Value> = KEYS
        .iter()
        .map(|(k, _, _)| ((*k).to_string(), json!(value_of(options, k))))
        .collect();
    let mut choices = Map::new();
    for (key, sheet) in SHEET_KEYS {
        values.insert(key.into(), json!(sheet_value(theme, options, key, sheet)));
        let names: Vec<String> = theme
            .choices(sheet)
            .into_iter()
            .map(|o| o.name)
            .filter(|n| n != default_of(theme, sheet))
            .collect();
        choices.insert(key.into(), json!(names));
    }
    let mut keys: Vec<&str> = KEYS.iter().map(|(k, _, _)| *k).collect();
    keys.extend(SHEET_KEYS.iter().map(|(k, _)| *k));
    let v = json!({
        "signedIn": options.is_some(),
        "storageKey": STORAGE_KEY,
        "keys": keys,
        "choices": choices,
        "values": if options.is_some() { Value::Object(values) } else { Value::Null },
        "own": own,
    });
    // A JSON block's text must not close the element.
    v.to_string().replace("</", "<\\/")
}

/// One select of the panel.
fn select(s: &mut String, m: &Messages, key: &str, options: &[(String, String)], current: &str) {
    let id = format!("ts-appearance-{}", key.replace('.', "-"));
    let label_key = format!("ts-pref-{}", key.replace('.', "-"));
    let _ = write!(
        s,
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"{id}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><select class=\"cdx-select\" id=\"{id}\" name=\"{}\" data-key=\"{}\">",
        esc(&m.get(&label_key)),
        esc(key),
        esc(key)
    );
    for (value, label) in options {
        let _ = write!(
            s,
            "<option value=\"{}\"{}>{}</option>",
            esc(value),
            if current == value { " selected" } else { "" },
            esc(label)
        );
    }
    s.push_str("</select></div></div>");
}

/// The options of a palette or type-set select: the wiki's own as "Wiki default", then
/// the others the reader may choose, the accessibility ones marked.
#[must_use]
pub fn sheet_options(m: &Messages, theme: &Theme, sheet: Sheet) -> Vec<(String, String)> {
    let all = theme.choices(sheet);
    let default = default_of(theme, sheet);
    let default_label = all
        .iter()
        .find(|o| o.name == default)
        .map_or_else(|| default.to_string(), |o| o.label.clone());
    let mut out = vec![(
        String::new(),
        m.with("ts-appearance-wiki-default", &[&default_label]),
    )];
    for o in all.into_iter().filter(|o| o.name != default) {
        let label = if o.accessibility {
            m.with("ts-appearance-accessibility-choice", &[&o.label])
        } else {
            o.label
        };
        out.push((o.name, label));
    }
    out
}

/// The panel: a form of selects, which posts to the web tier for a signed-in reader and
/// is worked by the site's script for one who is not.
#[must_use]
pub fn panel(
    m: &Messages,
    theme: &Theme,
    options: Option<&Map<String, Value>>,
    returnto: &str,
) -> String {
    let signed_in = options.is_some();
    let mut s = format!(
        "<form class=\"ts-appearance__form ts-appearance__form--{}\" method=\"post\" action=\"/w/index.php?title=Special:Preferences&amp;appearance=1\"><input type=\"hidden\" name=\"returnto\" value=\"{}\">",
        if signed_in { "account" } else { "browser" },
        esc(returnto)
    );
    if !signed_in {
        let _ = write!(
            s,
            "<p class=\"ts-appearance__nojs\">{}</p>",
            esc(&m.get("ts-appearance-nojs"))
        );
    }
    s.push_str("<div class=\"ts-appearance__controls\">");
    for (key, choices, _) in KEYS {
        let label_key = format!("ts-pref-{}", key.replace('.', "-"));
        let opts: Vec<(String, String)> = choices
            .iter()
            .map(|c| ((*c).to_string(), m.get(&format!("{label_key}-{c}"))))
            .collect();
        select(&mut s, m, key, &opts, value_of(options, key));
    }
    for (key, sheet) in SHEET_KEYS {
        let opts = sheet_options(m, theme, sheet);
        if opts.len() > 1 {
            select(
                &mut s,
                m,
                key,
                &opts,
                &sheet_value(theme, options, key, sheet),
            );
        }
    }
    if signed_in {
        let _ = write!(
            s,
            "<button class=\"cdx-button cdx-button--action-progressive cdx-button--weight-primary ts-appearance__apply\" type=\"submit\">{}</button>",
            esc(&m.get("ts-appearance-apply"))
        );
    }
    s.push_str("</div></form>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_match_the_registry() {
        let reg: toml::Table =
            toml::from_str(include_str!("../../../docs/registry/preferences.toml")).unwrap();
        let prefs = reg["preference"].as_array().unwrap();
        for (key, choices, default) in KEYS {
            let p = prefs
                .iter()
                .find(|p| p["key"].as_str() == Some(key))
                .unwrap_or_else(|| panic!("{key} is registered"));
            assert_eq!(p["group"].as_str(), Some("appearance"), "{key}");
            assert_eq!(p["browser"].as_bool(), Some(true), "{key}");
            let reg_choices: Vec<&str> = p["choices"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(toml::Value::as_str)
                .collect();
            assert_eq!(&reg_choices, choices, "{key}");
            assert_eq!(p["default"].as_str(), Some(*default), "{key}");
        }
    }

    #[test]
    fn attributes() {
        let (class, attrs) = html_attributes(None);
        assert_eq!(class, "skin-theme-clientpref-os");
        assert!(attrs.is_empty());
        let o: Map<String, Value> = serde_json::from_value(
            json!({"ui.mode": "dark", "ui.contrast": "more", "ui.text-size": "large"}),
        )
        .unwrap();
        let (class, attrs) = html_attributes(Some(&o));
        assert_eq!(class, "skin-theme-clientpref-night");
        assert_eq!(
            attrs,
            " data-appearance=\"account\" data-mode=\"dark\" data-contrast=\"more\" data-text-size=\"large\""
        );
        let junk: Map<String, Value> =
            serde_json::from_value(json!({"ui.mode": "\"><script>", "ui.text-size": 3})).unwrap();
        let (class, attrs) = html_attributes(Some(&junk));
        assert_eq!(class, "skin-theme-clientpref-os");
        assert_eq!(
            attrs,
            " data-appearance=\"account\" data-text-size=\"medium\""
        );
    }

    #[test]
    fn the_data_block_cannot_close_its_element() {
        let o: Map<String, Value> = Map::new();
        let t = Theme::shipped();
        let d = data(&t, Some(&o), Some(&["</script>".to_string()]));
        assert!(!d.contains("</"));
        assert!(data(&t, None, None).contains("\"signedIn\":false"));
        let v: Value = serde_json::from_str(&data(&t, Some(&o), None)).unwrap();
        assert_eq!(
            v["choices"]["ui.palette"],
            json!(["parchment-co", "parchment-ob"])
        );
        assert_eq!(v["values"]["ui.palette"], "");
    }

    #[test]
    fn the_panel_offers_what_the_wiki_lets_readers_choose() {
        let m = Messages::for_language("en");
        let o: Map<String, Value> =
            serde_json::from_value(json!({"ui.palette": "parchment-ob", "ui.typeset": "nope"}))
                .unwrap();
        let html = panel(&m, &Theme::shipped(), Some(&o), "Main Page");
        assert!(html.contains("<select class=\"cdx-select\" id=\"ts-appearance-ui-palette\" name=\"ui.palette\" data-key=\"ui.palette\"><option value=\"\">"));
        assert!(html.contains("<option value=\"parchment-ob\" selected>"));
        assert!(
            html.contains("<option value=\"\" selected>"),
            "an unknown type set is the wiki's"
        );
        assert!(html.contains("<option value=\"opendyslexic\">"));
    }
}
