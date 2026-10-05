//! The page frame (0010 §2): the global header (site name, search, the site's links, the
//! **New** menu, the account menu or **Log in**), the page header (identity line, title,
//! tabs), the page's body and the footer. Each part of the header shows only where its
//! feature is offered ([`Features`]).
//!
//! The notifications bell comes with the inbox (0021 §6), which is not in Phase 0.

use askama::Template;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::assets;
use crate::codex::SearchInput;
use crate::features::{Feature, Features};
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
    /// The menu's links, as label and target; the name alone where there are none.
    pub items: Vec<(String, String)>,
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

/// What the frame needs of the site.
#[derive(Debug, Clone, Copy)]
pub struct Chrome<'a> {
    /// The site's name.
    pub sitename: &'a str,
    /// The main page's title.
    pub mainpage: &'a str,
    /// What the site offers.
    pub features: &'a Features,
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
    search: Option<String>,
    account: Option<Account>,
    mainpage_href: String,
    login: bool,
    nav: Vec<(String, String)>,
    new_menu: Vec<(String, String)>,
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
    chrome: &Chrome,
    theme: &Theme,
    user: Option<&str>,
    page: &Page,
) -> askama::Result<String> {
    let f = chrome.features;
    let search = if f.has(Feature::Search) {
        Some(
            SearchInput {
                name: "search",
                value: &page.search,
                placeholder: &m.get("ts-search-placeholder"),
                label: &m.get("ts-search-label"),
                button: &m.get("ts-search-button"),
            }
            .render()?,
        )
    } else {
        None
    };
    let returnto = page
        .returnto
        .as_deref()
        .map_or_else(|| title_url(chrome.mainpage), title_url);
    let account = user.map(|name| {
        let mut items = Vec::new();
        if f.has(Feature::Pages) {
            items.push((
                m.get("ts-account-userpage"),
                format!("/wiki/User:{}", title_url(name)),
            ));
        }
        if f.has(Feature::Account) {
            items.push((m.get("ts-account"), "/wiki/Special:Account".to_string()));
        }
        if f.has(Feature::Login) {
            items.push((
                m.get("ts-logout"),
                format!("/wiki/Special:UserLogout?returnto={returnto}"),
            ));
        }
        Account {
            name: name.to_string(),
            label: m.with("ts-account-menu", &[name]),
            items,
        }
    });
    let mut nav = Vec::new();
    if f.has(Feature::RecentChanges) {
        nav.push((
            m.get("ts-nav-recentchanges"),
            "/wiki/Special:RecentChanges".to_string(),
        ));
    }
    if f.has(Feature::Jobs) {
        nav.push((m.get("ts-nav-jobs"), "/wiki/Special:Jobs".to_string()));
    }
    nav.push((
        m.get("ts-nav-specialpages"),
        "/wiki/Special:SpecialPages".to_string(),
    ));
    let mut new_menu = Vec::new();
    if f.has(Feature::Create) {
        new_menu.push((m.get("ts-new-item"), "/wiki/Special:NewItem".to_string()));
        new_menu.push((
            m.get("ts-new-property"),
            "/wiki/Special:NewProperty".to_string(),
        ));
    }
    if f.has(Feature::Pages) {
        new_menu.push((m.get("ts-new-page"), "/w/index.php?action=edit".to_string()));
    }
    let entries = assets::entries();
    let css = if m.dir() == "rtl" {
        entries.css_rtl.as_deref()
    } else {
        entries.css.as_deref()
    };
    Frame {
        m,
        sitename: chrome.sitename,
        title: &page.title,
        title_lang: page.title_lang.as_deref(),
        doc_title: page.doc_title.as_deref().unwrap_or(&page.title),
        identity: page.identity_html.as_deref(),
        subtitle: page.subtitle_html.as_deref(),
        tabs: &page.tabs,
        body: &page.body,
        search,
        account,
        mainpage_href: format!("/wiki/{}", title_url(chrome.mainpage)),
        login: f.has(Feature::Login),
        nav,
        new_menu,
        returnto,
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

    fn chrome(features: &Features) -> Chrome<'_> {
        Chrome {
            sitename: "Librarybase",
            mainpage: "Project:Home",
            features,
        }
    }

    fn all() -> Features {
        Features::all_built()
    }

    #[test]
    fn anonymous_frame() {
        let m = Messages::for_language("en");
        let f = all();
        let html = render(&m, &chrome(&f), Theme::shipped(), None, &page()).unwrap();
        assert!(
            html.starts_with(
                "<!DOCTYPE html>\n<html class=\"client-nojs\" lang=\"en\" dir=\"ltr\">"
            )
        );
        assert!(html.contains("<title>Main Page – Librarybase</title>"));
        assert!(html.contains(">Librarybase</a>"));
        assert!(html.contains("name=\"search\""));
        assert!(html.contains("href=\"/wiki/Project:Home\">Librarybase</a>"));
        assert!(html.contains("aria-current=\"page\">Read</a>"));
        assert!(html.contains(&Theme::shipped().href()));
        assert!(html.contains("<p>x</p>"));
    }

    #[test]
    fn account_menu_escapes_the_name() {
        let m = Messages::for_language("en");
        let f = all();
        let html = render(&m, &chrome(&f), Theme::shipped(), Some("A <b> C"), &page()).unwrap();
        assert!(html.contains("A &#60;b&#62; C"));
        assert!(!html.contains("Special:UserLogin"));
    }

    #[test]
    fn features_not_offered_are_not_shown() {
        let m = Messages::for_language("en");
        let none = Features::default();
        let html = render(&m, &chrome(&none), Theme::shipped(), None, &page()).unwrap();
        assert!(
            !html.contains("name=\"search\""),
            "no search box without search"
        );
        for absent in [
            "RecentChanges",
            "Special:Jobs",
            "Special:NewItem",
            "action=edit",
            "Special:UserLogin",
        ] {
            assert!(!html.contains(absent), "{absent}");
        }
        assert!(html.contains("Special:SpecialPages"));
        let signed_in =
            render(&m, &chrome(&none), Theme::shipped(), Some("Alice"), &page()).unwrap();
        assert!(signed_in.contains("Alice"));
        assert!(!signed_in.contains("Special:Account"));
        assert!(!signed_in.contains("User:Alice"));
    }

    #[test]
    fn titles_in_urls() {
        assert_eq!(title_url("Main Page"), "Main_Page");
        assert_eq!(title_url("Item:Q6"), "Item:Q6");
        assert_eq!(title_url("A&B?c"), "A%26B%3Fc");
        assert_eq!(title_url("Ünïcode"), "%C3%9Cn%C3%AFcode");
    }
}
