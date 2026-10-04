//! The page frame (0010 §2): the global header (site name, search, the site's links, the
//! **New** menu, the account menu or **Log in**), the page header (identity line, title,
//! tabs), the page's body and the footer.
//!
//! The notifications bell comes with the inbox (0021 §6), which is not in Phase 0.

use askama::Template;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::assets;
use crate::codex::SearchInput;
use crate::i18n::Messages;
use crate::theme::Theme;

/// The characters a title keeps unencoded in a path or a query value, as MediaWiki's
/// `wfUrlencode` leaves them.
const TITLE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':')
    .remove(b'/')
    .remove(b'(')
    .remove(b')')
    .remove(b'!')
    .remove(b',');

/// A title as it appears in a URL: spaces as underscores, the rest percent-encoded.
#[must_use]
pub fn title_url(title: &str) -> String {
    utf8_percent_encode(&title.replace(' ', "_"), TITLE).to_string()
}

/// One of the page's tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// The label.
    pub label: String,
    /// Where it goes.
    pub href: String,
    /// Whether it is the page being shown.
    pub current: bool,
}

/// The signed-in viewer, for the account menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The name.
    pub name: String,
    /// The account menu's accessible name.
    pub label: String,
    /// The user page.
    pub href: String,
}

/// What a page puts in the frame.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// The title, as shown.
    pub title: String,
    /// The title the login and logout links return to, as shown; none for a page that is
    /// not one (a 404).
    pub returnto: Option<String>,
    /// The title's language, where it differs from the page's (a label in a fallback
    /// language).
    pub title_lang: Option<String>,
    /// The document title, where it differs from the title (`Label (Q6)`).
    pub doc_title: Option<String>,
    /// The identity line, as HTML built from escaped parts.
    pub identity_html: Option<String>,
    /// What goes under the title (an entity's terms), as HTML built from escaped parts.
    pub subtitle_html: Option<String>,
    /// The tabs.
    pub tabs: Vec<Tab>,
    /// The body, as HTML the page built from escaped parts.
    pub body: String,
    /// What the search box already holds.
    pub search: String,
}

/// The frame.
#[derive(Debug, Template)]
#[template(path = "frame.html")]
pub struct Frame<'a> {
    m: &'a Messages,
    sitename: &'a str,
    title: &'a str,
    title_lang: Option<&'a str>,
    doc_title: &'a str,
    identity: Option<&'a str>,
    subtitle: Option<&'a str>,
    tabs: &'a [Tab],
    body: &'a str,
    search: String,
    account: Option<Account>,
    returnto: String,
    css: Option<&'a str>,
    js: Option<&'a str>,
    theme_href: String,
}

/// Renders a page in the frame.
///
/// # Errors
///
/// If the template fails to render, which it does only on a write error.
pub fn render(
    m: &Messages,
    sitename: &str,
    theme: &Theme,
    user: Option<&str>,
    page: &Page,
) -> askama::Result<String> {
    let search = SearchInput {
        name: "search",
        value: &page.search,
        placeholder: &m.get("ts-search-placeholder"),
        label: &m.get("ts-search-label"),
        button: &m.get("ts-search-button"),
    }
    .render()?;
    let account = user.map(|name| Account {
        name: name.to_string(),
        label: m.with("ts-account-menu", &[name]),
        href: format!("/wiki/User:{}", title_url(name)),
    });
    let entries = assets::entries();
    let css = if m.dir() == "rtl" {
        entries.css_rtl.as_deref()
    } else {
        entries.css.as_deref()
    };
    Frame {
        m,
        sitename,
        title: &page.title,
        title_lang: page.title_lang.as_deref(),
        doc_title: page.doc_title.as_deref().unwrap_or(&page.title),
        identity: page.identity_html.as_deref(),
        subtitle: page.subtitle_html.as_deref(),
        tabs: &page.tabs,
        body: &page.body,
        search,
        account,
        returnto: page
            .returnto
            .as_deref()
            .map_or_else(|| "Main_Page".to_string(), title_url),
        css,
        js: entries.js.as_deref(),
        theme_href: theme.href(),
    }
    .render()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page {
            title: "Main Page".into(),
            returnto: Some("Main Page".into()),
            tabs: vec![Tab {
                label: "Read".into(),
                href: "/wiki/Main_Page".into(),
                current: true,
            }],
            body: "<p>x</p>".into(),
            ..Page::default()
        }
    }

    #[test]
    fn anonymous_frame() {
        let m = Messages::for_language("en");
        let html = render(&m, "Librarybase", Theme::shipped(), None, &page()).unwrap();
        assert!(
            html.starts_with(
                "<!DOCTYPE html>\n<html class=\"client-nojs\" lang=\"en\" dir=\"ltr\">"
            )
        );
        assert!(html.contains("<title>Main Page – Librarybase</title>"));
        assert!(html.contains(">Librarybase</a>"));
        assert!(html.contains("name=\"search\""));
        assert!(html.contains("href=\"/wiki/Special:UserLogin?returnto=Main_Page\">Log in</a>"));
        assert!(html.contains("aria-current=\"page\">Read</a>"));
        assert!(html.contains(&Theme::shipped().href()));
        assert!(html.contains("<p>x</p>"));
    }

    #[test]
    fn account_menu_escapes_the_name() {
        let m = Messages::for_language("en");
        let html = render(&m, "L", Theme::shipped(), Some("A <b> C"), &page()).unwrap();
        assert!(html.contains("aria-label=\"Account menu for A &#60;b&#62; C\""));
        assert!(html.contains("href=\"/wiki/User:A_%3Cb%3E_C\""));
        assert!(!html.contains("Special:UserLogin"));
    }

    #[test]
    fn titles_in_urls() {
        assert_eq!(title_url("Main Page"), "Main_Page");
        assert_eq!(title_url("Item:Q6"), "Item:Q6");
        assert_eq!(title_url("A&B?c"), "A%26B%3Fc");
        assert_eq!(title_url("Ünïcode"), "%C3%9Cn%C3%AFcode");
    }
}
