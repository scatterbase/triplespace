//! `Special:Notifications` (0021 §6; 19 §6.2): the viewer's inbox as a page, newest first,
//! fifty to a page, everything or the unread only, with **Mark as read** on each unread
//! item and **Mark all as read** above them; a menu narrows the list to one kind of
//! notification (`reason=`).
//!
//! **Notifications alike are bundled** (16 §5.2): rows next to each other in the list with
//! the same reason about the same page or thread, within a day of the newest of them, are
//! one item that says how many more there are, and its **Mark as read** marks them all.
//! The rows stay separate: the bell still counts each. Opening it is looking: the bell's mark goes
//! (`echomarkseen`). Each item says who did what where, and links to it: a thread's post,
//! or the user page an item about an account names.
//!
//! The list is what the API lets the viewer see now: an item about a thread they may no
//! longer read, or a post since hidden, is not there (16 §5.2).

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::Notification;
use triplespace_client::settings::Saved;

use crate::Site;
use crate::codex::{Message, MessageKind, MessageModel};
use crate::entity::history::{Choice, Switch};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::{clock, date};
use crate::special::watchlist::LoginPage;

const TITLE: &str = "Special:Notifications";

/// How many items a page shows.
const PER_PAGE: u32 = 50;

/// The kinds of notification the list can be narrowed to (16 §5.2), each named by the
/// message `ts-notif-reason-{reason}`, as the preference matrix names them.
pub(crate) const REASONS: [&str; 9] = [
    "talk",
    "reply",
    "mention",
    "thread-status",
    "watch",
    "rights",
    "rename",
    "group-join",
    "job",
];

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `special/notifications.html` draws.
// ---------------------------------------------------------------------------------------

/// A part of an item's sentence: text, or a link in its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Part {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// A link.
    Link {
        /// The page.
        href: String,
        /// The text.
        text: String,
        /// The `class`, for the thread link.
        class: Option<&'static str>,
    },
}

/// **Mark as read** or **Mark all as read**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarkForm {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// `markread` or `markall`.
    pub name: String,
    /// The ID, or `1`.
    pub value: String,
    /// Whether to come back to the unread view.
    pub unread: bool,
    /// The kind of notification to come back to, where the list is narrowed to one.
    pub reason: Option<String>,
    /// The button.
    pub label: String,
}

/// The menu that narrows the list to one kind of notification: a form that works by
/// `GET`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReasonForm {
    /// The menu's label.
    pub label: String,
    /// Whether the unread view is kept.
    pub unread: bool,
    /// Every kind first, then each kind.
    pub options: Vec<crate::special::controls::OptionModel>,
    /// The button.
    pub apply: String,
}

/// One item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// The reason, as the API names it.
    pub reason: String,
    /// Whether it is unread.
    pub unread: bool,
    /// The sentence.
    pub sentence: Vec<Part>,
    /// ISO 8601.
    pub datetime: String,
    /// "HH:MM, date".
    pub time: String,
    /// The talk page, where the item has one.
    pub talk: Option<Choice>,
    /// **Mark as read**, for an unread item.
    pub mark: Option<MarkForm>,
    /// "and n more like this", where the item stands for a bundle.
    pub more: Option<String>,
}

/// `Special:Notifications`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotificationsPage {
    /// The filters: all, unread.
    pub filters: Switch,
    /// The kinds of notification.
    pub reasons: ReasonForm,
    /// **Mark all as read**, when any is unread.
    pub markall: Option<MarkForm>,
    /// What the page says when it has no items.
    pub empty: Option<MessageModel>,
    /// The items.
    pub items: Vec<Item>,
    /// **Older**.
    pub older: Option<Choice>,
}

/// One item's sentence, its links in place.
fn sentence(m: &Messages, n: &Notification) -> Vec<Part> {
    let who = match &n.agent {
        Some(agent) => Part::Link {
            href: format!("/wiki/User:{}", title_url(agent)),
            text: agent.clone(),
            class: None,
        },
        None => Part::Text {
            text: m.get("ts-notif-someone"),
        },
    };
    let thread = match (&n.title, &n.subject) {
        (Some(title), Some(subject)) => Some(Part::Link {
            href: match n.revid {
                Some(rev) => format!("/wiki/{}#post-{rev}", title_url(title)),
                None => format!("/wiki/{}", title_url(title)),
            },
            text: format!("“{subject}”"),
            class: Some("ts-notif__thread"),
        }),
        _ => None,
    };
    let account = n.title.as_deref().map(|t| Part::Link {
        href: format!("/wiki/{}", title_url(t)),
        text: t.split_once(':').map_or(t, |(_, n)| n).to_string(),
        class: None,
    });
    // A job's end (16 §5.2): the job, linked to its page, and how it ended, which the
    // event's parameters say (counts for a finish, `error` for a failure, `by` for an
    // interruption).
    let job = n
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Special:Jobs/"))
        .map(|id| Part::Link {
            href: format!("/wiki/Special:Jobs/{id}"),
            text: m.with("ts-changes-job", &[id]),
            class: None,
        });
    let key = match n.reason.as_str() {
        "job" if n.params.get("error").is_some() => "ts-notif-job-failed",
        "job" if n.params.get("by").is_some() => "ts-notif-job-interrupted",
        "job" if n.params.get("counts").is_some() => "ts-notif-job-finished",
        "job" if n.params.get("reason").is_some() => "ts-notif-job-rejected",
        "job" => "ts-notif-job-reverted",
        "talk" => "ts-notif-talk",
        "reply" => "ts-notif-reply",
        "mention" => "ts-notif-mention",
        "thread-status" => "ts-notif-thread-status",
        "rights" => "ts-notif-rights",
        "rename" => "ts-notif-rename",
        "group-join" => "ts-notif-group-join",
        "watch" if n.subject.is_some() => "ts-notif-watch-thread",
        "watch" => "ts-notif-watch",
        _ => "ts-notif-other",
    };
    let page = n.title.as_deref().map(|t| Part::Link {
        href: match n.revid {
            Some(rev) => format!("/w/index.php?title={}&oldid={rev}", title_url(t)),
            None => format!("/wiki/{}", title_url(t)),
        },
        text: t.to_string(),
        class: None,
    });
    let second = match n.reason.as_str() {
        "job" => job,
        "group-join" => account,
        "watch" if n.subject.is_none() => page,
        _ => thread,
    };
    // The message, its `$1` and `$2` replaced by the parts.
    let text = m.get(key);
    let mut parts = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("$1").into_iter().chain(rest.find("$2")).min() {
        if i > 0 {
            parts.push(Part::Text {
                text: rest[..i].to_string(),
            });
        }
        let which = &rest[i..i + 2];
        let part = if which == "$1" {
            Some(&who)
        } else {
            second.as_ref()
        };
        if let Some(p) = part {
            parts.push(p.clone());
        }
        rest = &rest[i + 2..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text {
            text: rest.to_string(),
        });
    }
    parts
}

fn mark_form(
    token: &str,
    ids: Option<&[i64]>,
    label: &str,
    unread_only: bool,
    reason: Option<&str>,
) -> MarkForm {
    let (name, value) = match ids {
        Some(ids) => (
            "markread",
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        None => ("markall", "1".to_string()),
    };
    MarkForm {
        action: format!("/w/index.php?title={TITLE}"),
        token: token.to_string(),
        name: name.to_string(),
        value,
        unread: unread_only,
        reason: reason.map(str::to_string),
        label: label.to_string(),
    }
}

/// How long after its newest row a bundle still takes an older one: a day.
const BUNDLE_WINDOW: i64 = 24 * 60 * 60;

/// Seconds since the epoch of an ISO 8601 time in UTC (`2026-11-10T01:11:00Z`); `None`
/// for anything else.
fn epoch(iso: &str) -> Option<i64> {
    if iso.len() < 19 {
        return None;
    }
    let field = |from: usize, to: usize| iso.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let seconds = field(11, 13)? * 3600 + field(14, 16)? * 60 + field(17, 19)?;
    // Days from the civil date (Howard Hinnant's algorithm).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some((era * 146_097 + day_of_era - 719_468) * 86_400 + seconds)
}

/// The list as bundles (16 §5.2): each a run of rows next to each other with the same
/// reason about the same page or thread, the newest first, none more than
/// [`BUNDLE_WINDOW`] older than it. A row about no page is a bundle of its own.
fn bundles(items: &[Notification]) -> Vec<Vec<&Notification>> {
    let mut out: Vec<Vec<&Notification>> = Vec::new();
    for n in items {
        let joins = out.last().is_some_and(|b| {
            let first = b[0];
            n.title.is_some()
                && first.reason == n.reason
                && first.title == n.title
                && match (epoch(&first.timestamp), epoch(&n.timestamp)) {
                    (Some(a), Some(b)) => (a - b).abs() <= BUNDLE_WINDOW,
                    _ => false,
                }
        });
        match out.last_mut() {
            Some(b) if joins => b.push(n),
            _ => out.push(vec![n]),
        }
    }
    out
}

/// The page's address with the view it shows: the unread only, one kind, a later page.
fn href(unread_only: bool, reason: Option<&str>, from: Option<&str>) -> String {
    let mut q = Vec::new();
    if let Some(f) = from {
        q.push(format!("from={}", url_q(f)));
    }
    if unread_only {
        q.push("unread=1".to_string());
    }
    if let Some(r) = reason {
        q.push(format!("reason={r}"));
    }
    if q.is_empty() {
        format!("/wiki/{TITLE}")
    } else {
        format!("/wiki/{TITLE}?{}", q.join("&"))
    }
}

/// The kind the query names, where it is one the list can be narrowed to.
fn reason_of(q: &std::collections::BTreeMap<String, String>) -> Option<&'static str> {
    let asked = q.get("reason")?;
    REASONS.iter().find(|r| *r == asked).copied()
}

/// `GET Special:Notifications`.
#[allow(clippy::too_many_lines)]
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Notifications) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    if cx.user.is_none() {
        let body = cx.draw(
            "login.html",
            LoginPage {
                notice: Message::block(MessageKind::Notice, &cx.m.get("ts-notifications-login"))
                    .model(),
                login: Some(Choice {
                    href: format!("/wiki/Special:UserLogin?returnto={TITLE}"),
                    label: cx.m.get("ts-login"),
                    current: false,
                }),
            },
        );
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-notifications-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let unread_only = params.map.get("unread").is_some_and(|v| v == "1");
    let reason = reason_of(&params.map);
    let from = params.map.get("from").map(String::as_str);
    let listing = match site
        .client()
        .notifications(&incoming, unread_only, reason, PER_PAGE, from)
        .await
    {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(listing.cache);
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache.clone());
    let cookies = token.set_cookies.clone();
    let tok = token.value.value;
    // Opening the inbox is looking at it: the bell's mark goes.
    if let Ok(Saved::Ok(_)) = site.client().mark_seen(&incoming, &tok).await
        && let Some(n) = cx.notifications.as_mut()
    {
        n.seen = n.newest_unread.clone();
    }
    let m = &cx.m;
    let items = &listing.value.items;
    let body = cx.draw(
        "special/notifications.html",
        NotificationsPage {
            filters: Switch {
                label: m.get("ts-notifications-filter"),
                items: vec![
                    Choice {
                        href: href(false, reason, None),
                        label: m.get("ts-notifications-all"),
                        current: !unread_only,
                    },
                    Choice {
                        href: href(true, reason, None),
                        label: m.get("ts-notifications-unread"),
                        current: unread_only,
                    },
                ],
            },
            reasons: ReasonForm {
                label: m.get("ts-notifications-reason"),
                unread: unread_only,
                options: std::iter::once(crate::special::controls::OptionModel {
                    value: String::new(),
                    label: m.get("ts-notifications-reason-all"),
                    selected: reason.is_none(),
                })
                .chain(
                    REASONS
                        .iter()
                        .map(|r| crate::special::controls::OptionModel {
                            value: (*r).to_string(),
                            label: m.get(&format!("ts-notif-reason-{r}")),
                            selected: reason == Some(*r),
                        }),
                )
                .collect(),
                apply: m.get("ts-notifications-reason-apply"),
            },
            markall: items.iter().any(|n| !n.read).then(|| {
                mark_form(
                    &tok,
                    None,
                    &m.get("ts-notifications-markall"),
                    unread_only,
                    reason,
                )
            }),
            empty: items.is_empty().then(|| {
                Message::block(
                    MessageKind::Notice,
                    &m.get(match (reason, unread_only) {
                        (Some(_), _) => "ts-notifications-none-reason",
                        (None, true) => "ts-notifications-none-unread",
                        (None, false) => "ts-notifications-empty",
                    }),
                )
                .model()
            }),
            items: bundles(items)
                .iter()
                .map(|bundle| {
                    // The newest row speaks for the bundle; what is unread in it is
                    // marked together.
                    let n = bundle[0];
                    let unread: Vec<i64> =
                        bundle.iter().filter(|x| !x.read).map(|x| x.id).collect();
                    Item {
                        reason: n.reason.clone(),
                        unread: !unread.is_empty(),
                        sentence: sentence(m, n),
                        datetime: n.timestamp.clone(),
                        time: format!("{}, {}", clock(&n.timestamp), date(m, &n.timestamp)),
                        talk: n.talk.as_deref().map(|t| Choice {
                            href: format!("/wiki/{}", title_url(t)),
                            label: t.to_string(),
                            current: false,
                        }),
                        mark: (!unread.is_empty()).then(|| {
                            mark_form(
                                &tok,
                                Some(&unread),
                                &m.get("ts-notifications-markread"),
                                unread_only,
                                reason,
                            )
                        }),
                        more: (bundle.len() > 1).then(|| {
                            m.with("ts-notifications-more", &[&(bundle.len() - 1).to_string()])
                        }),
                    }
                })
                .collect(),
            older: listing.value.next.as_ref().map(|next| Choice {
                href: href(unread_only, reason, Some(next)),
                label: m.get("ts-notifications-older"),
                current: false,
            }),
        },
    );
    let r = pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-notifications-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &cookies)
}

fn url_q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `POST Special:Notifications`: **Mark as read** (`markread`, an ID) or **Mark all as
/// read** (`markall`), then back to the list.
pub async fn post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Notifications) || cx.user.is_none() {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let form = &params.map;
    let token = form.get("wpEditToken").map_or("", String::as_str);
    // One ID, or a bundle's, separated by commas.
    let ids: Option<Vec<i64>> = form
        .get("markread")
        .map(|v| v.split(',').filter_map(|i| i.trim().parse().ok()).collect())
        .filter(|ids: &Vec<i64>| !ids.is_empty());
    if ids.is_none() && !form.contains_key("markall") {
        return pages::see_other(&format!("/wiki/{TITLE}"), &[]);
    }
    match site
        .client()
        .mark_read(&incoming, ids.as_deref(), None, token)
        .await
    {
        Ok(Saved::Ok(_)) => {
            let back = href(
                form.get("unread").is_some_and(|v| v == "1"),
                reason_of(form),
                None,
            );
            pages::see_other(&back, &[])
        }
        Ok(Saved::Refused(r)) => {
            let text = r
                .iter()
                .map(|x| x.message.clone())
                .collect::<Vec<_>>()
                .join(" ");
            pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |m| Page {
                title: m.get("ts-notifications-title"),
                returnto: Some(TITLE.to_string()),
                body: cx.message(MessageKind::Error, &text),
                ..Page::default()
            })
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 16 §5.2: bundling is presentational. Rows next to each other with the same reason
    // about the same page, within a day of the newest, are one item; the rows stay.
    #[test]
    fn rows_alike_are_bundled() {
        let row = |id: i64, reason: &str, title: Option<&str>, time: &str| Notification {
            id,
            reason: reason.into(),
            title: title.map(str::to_string),
            timestamp: time.into(),
            ..Notification::default()
        };
        let items = [
            row(9, "reply", Some("Thread:A"), "2026-11-10T12:00:00Z"),
            row(8, "reply", Some("Thread:A"), "2026-11-10T09:00:00Z"),
            row(7, "reply", Some("Thread:A"), "2026-11-09T12:00:00Z"),
            // More than a day older than the newest of the bundle: a new one.
            row(6, "reply", Some("Thread:A"), "2026-11-09T11:59:59Z"),
            row(5, "mention", Some("Thread:A"), "2026-11-09T11:00:00Z"),
            row(4, "reply", Some("Thread:B"), "2026-11-09T10:00:00Z"),
            // About no page: never bundled.
            row(3, "job", None, "2026-11-09T09:00:00Z"),
            row(2, "job", None, "2026-11-09T08:00:00Z"),
        ];
        let ids: Vec<Vec<i64>> = bundles(&items)
            .iter()
            .map(|b| b.iter().map(|n| n.id).collect())
            .collect();
        assert_eq!(
            ids,
            [vec![9, 8, 7], vec![6], vec![5], vec![4], vec![3], vec![2]]
        );
        assert_eq!(epoch("1970-01-02T00:00:01Z"), Some(86_401));
        assert_eq!(epoch("2026-11-10T00:00:00Z"), Some(1_794_268_800));
        assert_eq!(epoch("yesterday"), None);
        // A bundle's Mark as read carries every unread ID.
        let f = mark_form("t", Some(&[9, 7]), "Mark as read", false, None);
        assert_eq!((f.name.as_str(), f.value.as_str()), ("markread", "9,7"));
    }
}
