//! `Special:RecentChanges`, `Special:Contributions`, `Special:Log` and `Special:NewPages`
//! (0010 §7; 0020 §4; 19 §3.9; 0047 §4.4): feeds over one list, readable without
//! JavaScript. New pages are the feed's edits that created their target (`show=new`):
//! entities and document pages alike, as Wikidata's new pages are its new items.
//!
//! Both read `GET /activity` ([`triplespace_client::activity`]): recent changes is the
//! everything set, contributions the actor set. The rows are the same, drawn the same
//! way (§3.4's history rows, less the diff links the diff page will bring): the time,
//! a chip for where the record came from, the target with its label, the actor, the
//! summary, the size change and the tags, grouped by day.
//!
//! - **Filters** (`GET` form): the row kinds (edits, jobs, log actions; mirror syncs,
//!   off by default, with a strip above the list that says so and offers to show them),
//!   hiding bot edits, a namespace, the window in days (30 by default, as
//!   MediaWiki's default ⊂ `$wgRCMaxAge`) and the rows per page.
//! - **Paging** is the feed's cursor: **Older** carries `continue`, **Newest** drops it.
//! - **Contributions** take the user from the subpage (`Special:Contributions/Alice`) or
//!   `target=`, with a form for the name when neither is given. An unknown user is said
//!   to be one.
//!
//! What waits for later phases: the diff and history links (Phase 4's diff page), "group
//! by page", patrol marks, the Atom and stream forms, and user pages, which is why an
//! actor links to their contributions.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use triplespace_client::ClientError;
use triplespace_client::activity::{ActivityPage, ActivityQuery};

use triplespace_ui_kit::Site;
use triplespace_ui_kit::changelist::{ListModel, RowActions, list_model};
use triplespace_ui_kit::choice::{Choice, Pager};
use triplespace_ui_kit::codex::{Message, MessageKind, MessageModel};
use triplespace_ui_kit::controls::FormItem;
use triplespace_ui_kit::features::Feature;
use triplespace_ui_kit::frame::{Page, title_url};
use triplespace_ui_kit::i18n::Messages;
use triplespace_ui_kit::pages::{self, Context, Params, Peer};

/// The row kinds a reader can switch, with the message of each and whether it is on
/// when the form says nothing.
const KINDS: [(&str, &str, bool); 4] = [
    ("edit", "ts-changes-kind-edits", true),
    ("job", "ts-changes-kind-jobs", true),
    ("log", "ts-changes-kind-log", true),
    ("sync", "ts-changes-kind-syncs", false),
];

/// The namespaces the filter offers: number and message.
const NAMESPACES: [(&str, &str); 3] = [
    ("120", "ts-search-group-item"),
    ("122", "ts-search-group-property"),
    ("210", "ts-search-group-domain"),
];

const DAYS: [u32; 6] = [1, 3, 7, 14, 30, 90];
const DEFAULT_DAYS: u32 = 30;
const LIMITS: [u32; 4] = [50, 100, 250, 500];
const DEFAULT_LIMIT: u32 = 50;

/// Which page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    RecentChanges,
    Contributions,
    /// `Special:Log` (19 §6.3): the log rows, by type, actor and page.
    Log,
    /// `Special:NewPages` (0047 §4.4): the edits that created a page or an entity, by
    /// MediaWiki's `username`, `namespace` and `hidebots`.
    NewPages,
}

/// The log types `Special:Log` offers, each a message key; any other type typed into
/// the address is passed through.
const LOG_TYPES: [(&str, &str); 10] = [
    ("delete", "ts-log-type-delete"),
    ("move", "ts-log-type-move"),
    ("suppress", "ts-log-type-suppress"),
    ("protect", "ts-log-type-protect"),
    ("rights", "ts-log-type-rights"),
    ("block", "ts-log-type-block"),
    ("create", "ts-log-type-create"),
    ("thread", "ts-log-type-thread"),
    ("reclaim", "ts-log-type-reclaim"),
    ("settings", "ts-log-type-settings"),
];

/// What the query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    which: Which,
    /// The user, for contributions.
    user: Option<String>,
    kinds: Vec<String>,
    hide_bots: bool,
    namespace: Option<String>,
    /// The log type, for the log.
    log_type: Option<String>,
    /// A page's title, for the log.
    page: Option<String>,
    /// A setting, for the log: its history (19 §2.8), which is not cut off by days.
    setting: Option<String>,
    days: u32,
    limit: u32,
    cursor: Option<String>,
}

impl Ask {
    fn of(which: Which, params: &Params, subpage: Option<&str>) -> Self {
        let q = &params.map;
        let asked: Vec<String> = params.all("kind").map(str::to_string).collect();
        let filtered = q.contains_key("filtered");
        let kinds = if asked.is_empty() && !filtered {
            KINDS
                .iter()
                .filter(|(_, _, on)| *on)
                .map(|(k, _, _)| (*k).to_string())
                .collect()
        } else {
            KINDS
                .iter()
                .filter(|(k, _, _)| asked.iter().any(|a| a == k))
                .map(|(k, _, _)| (*k).to_string())
                .collect()
        };
        let pick = |key: &str, choices: &[u32], default: u32| {
            q.get(key)
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|n| choices.contains(n))
                .unwrap_or(default)
        };
        let clean = |u: &str| u.replace('_', " ").trim().to_string();
        // `Special:Log/delete` names a type, `Special:Log/Alice` a user, as MediaWiki's.
        let sub_is_type =
            which == Which::Log && subpage.is_some_and(|t| LOG_TYPES.iter().any(|(k, _)| *k == t));
        let user = match which {
            Which::Contributions => subpage
                .map(str::to_string)
                .or_else(|| q.get("target").cloned())
                .map(|u| clean(&u))
                .filter(|u| !u.is_empty()),
            Which::Log => subpage
                .filter(|_| !sub_is_type)
                .map(str::to_string)
                .or_else(|| q.get("user").cloned())
                .map(|u| clean(&u))
                .filter(|u| !u.is_empty()),
            Which::NewPages => q
                .get("username")
                .map(|u| clean(u))
                .filter(|u| !u.is_empty()),
            Which::RecentChanges => None,
        };
        let log_type = match which {
            Which::Log => subpage
                .filter(|_| sub_is_type)
                .map(str::to_string)
                .or_else(|| q.get("type").cloned())
                .map(|t| t.trim().to_string())
                .filter(|t| {
                    !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                }),
            _ => None,
        };
        let page = match which {
            Which::Log => q.get("page").map(|p| clean(p)).filter(|p| !p.is_empty()),
            _ => None,
        };
        let setting = match which {
            Which::Log => q.get("setting").map(|s| s.trim().to_string()).filter(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
            }),
            _ => None,
        };
        Self {
            which,
            user,
            setting,
            kinds: match which {
                Which::Log => vec!["log".into()],
                Which::NewPages => vec!["edit".into()],
                _ => kinds,
            },
            log_type,
            page,
            // The form says what to show (`bots`, on by default); the URL keeps
            // MediaWiki's `hidebots=1` too.
            hide_bots: q.get("hidebots").is_some_and(|v| v != "0")
                || (filtered && q.get("bots").is_none()),
            namespace: q
                .get("namespace")
                .filter(|n| NAMESPACES.iter().any(|(k, _)| k == n))
                .cloned(),
            days: pick("days", &DAYS, DEFAULT_DAYS),
            limit: pick("limit", &LIMITS, DEFAULT_LIMIT),
            cursor: q.get("continue").cloned().filter(|c| !c.is_empty()),
        }
    }

    /// The feed to fetch, from `now`; `target` is the log's page, resolved.
    fn query(&self, now: u64, lang: &str, target: Option<String>) -> ActivityQuery {
        let since = now.saturating_sub(u64::from(self.days) * 86_400 * 1_000_000);
        // An erased record was an edit: its gap row goes with the edits (19 §3.4).
        let mut kinds = self.kinds.clone();
        if kinds.iter().any(|k| k == "edit") {
            kinds.push("erased".into());
        }
        ActivityQuery {
            target,
            actor: self.user.clone(),
            log_type: self.log_type.clone(),
            setting: self.setting.clone(),
            kinds,
            namespaces: self.namespace.iter().cloned().collect(),
            show: self
                .hide_bots
                .then(|| "!bot".to_string())
                .into_iter()
                .chain((self.which == Which::NewPages).then(|| "new".to_string()))
                .collect(),
            // A setting's history reaches back to its first change.
            since: self.setting.is_none().then(|| iso8601(since)),
            limit: self.limit,
            cursor: self.cursor.clone(),
            lang: lang.to_string(),
            ..ActivityQuery::default()
        }
    }

    /// The page's own URL with these filters and a cursor.
    fn href(&self, cursor: Option<&str>) -> String {
        let mut s = match (self.which, &self.user) {
            (Which::RecentChanges, _) => "/w/index.php?title=Special:RecentChanges".to_string(),
            (Which::Log, _) => "/w/index.php?title=Special:Log".to_string(),
            (Which::Contributions, Some(u)) => format!(
                "/w/index.php?title=Special:Contributions/{}",
                utf8_percent_encode(&u.replace(' ', "_"), NON_ALPHANUMERIC)
            ),
            (Which::Contributions, None) => "/w/index.php?title=Special:Contributions".to_string(),
            (Which::NewPages, _) => "/w/index.php?title=Special:NewPages".to_string(),
        };
        s.push_str("&filtered=1");
        if self.which == Which::NewPages
            && let Some(u) = &self.user
        {
            let _ = write!(s, "&username={}", utf8_percent_encode(u, NON_ALPHANUMERIC));
        }
        for k in &self.kinds {
            let _ = write!(s, "&kind={k}");
        }
        // `filtered=1` reads a missing `bots` as off, so a link that keeps bot edits on
        // says so, as the form's ticked box does.
        if self.hide_bots {
            s.push_str("&hidebots=1");
        } else {
            s.push_str("&bots=1");
        }
        if let Some(n) = &self.namespace {
            let _ = write!(s, "&namespace={n}");
        }
        if self.which == Which::Log {
            if let Some(t) = &self.log_type {
                let _ = write!(s, "&type={}", utf8_percent_encode(t, NON_ALPHANUMERIC));
            }
            if let Some(u) = &self.user {
                let _ = write!(s, "&user={}", utf8_percent_encode(u, NON_ALPHANUMERIC));
            }
            if let Some(k) = &self.setting {
                let _ = write!(s, "&setting={}", utf8_percent_encode(k, NON_ALPHANUMERIC));
            }
            if let Some(p) = &self.page {
                let _ = write!(s, "&page={}", utf8_percent_encode(p, NON_ALPHANUMERIC));
            }
        }
        let _ = write!(s, "&days={}&limit={}", self.days, self.limit);
        if let Some(c) = cursor {
            let _ = write!(s, "&continue={}", utf8_percent_encode(c, NON_ALPHANUMERIC));
        }
        s
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` from µs since the epoch.
fn iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what the skin's `changes.html` macros and the
// `special/changes.html`, `special/watchlist.html` and `special/notifications.html`
// templates draw.
// ---------------------------------------------------------------------------------------

/// The filter form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormModel {
    /// The accessible name.
    pub label: String,
    /// The page it filters.
    pub title: String,
    /// The controls.
    pub items: Vec<FormItem>,
    /// **Apply**.
    pub apply: String,
}

/// The sync strip of 19 §3.9.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Strip {
    /// "Mirror syncs are hidden."
    pub text: String,
    /// **Show them**.
    pub show: Choice,
}

/// A feed page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangesPage {
    /// The filter form.
    pub form: FormModel,
    /// The sync strip, when syncs are off.
    pub strip: Option<Strip>,
    /// What the API refused.
    pub error: Option<MessageModel>,
    /// What the page says when it has no rows.
    pub empty: Option<String>,
    /// The rows.
    pub list: Option<ListModel>,
    /// The pager.
    pub pager: Option<Pager>,
    /// The introduction of contributions asked for no one.
    pub intro: Option<String>,
    /// A Contributions header's operator relation (0024 §9).
    pub accounts: Option<AccountsLine>,
}

/// The operator relation in a Contributions header (0024 §9; 19 §2.5): a subsidiary's
/// header names and links its operator; a primary account's lists its subsidiaries. Each
/// name links to that account's contributions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountsLine {
    /// "Bot operated by", around the operator.
    pub operated_by: Option<OperatedBy>,
    /// "Subsidiary accounts:", and each one.
    pub subsidiaries: Option<SubsidiaryList>,
}

/// "Bot operated by {Name}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatedBy {
    /// The text before the name.
    pub before: String,
    /// The operator.
    pub operator: Choice,
    /// The text after it.
    pub after: String,
}

/// A primary account's subsidiaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubsidiaryList {
    /// "Subsidiary accounts:".
    pub label: String,
    /// Each one, with "retired" or "pending approval" where it is.
    pub items: Vec<(Choice, Option<String>)>,
}

/// The header's model, or `None` for an account with neither end of the relation.
#[must_use]
pub fn accounts_line(m: &Messages, r: &triplespace_client::Relations) -> Option<AccountsLine> {
    let contributions = |name: &str| {
        choice(
            format!("/wiki/Special:Contributions/{}", title_url(name)),
            name.to_string(),
        )
    };
    let operated_by = r.bot.as_ref().map(|b| {
        let text = m.get(match b.status.as_str() {
            "pending" => "ts-identity-bot-pending",
            "retired" => "ts-identity-bot-retired",
            _ => "ts-identity-bot",
        });
        let (before, after) = text.split_once("$1").unwrap_or((text.as_str(), ""));
        OperatedBy {
            before: before.to_string(),
            operator: contributions(&b.operator),
            after: after.to_string(),
        }
    });
    let subsidiaries = (!r.subsidiaries.is_empty()).then(|| SubsidiaryList {
        label: m.get("ts-contributions-subsidiaries"),
        items: r
            .subsidiaries
            .iter()
            .map(|(name, status)| {
                let note = match status.as_str() {
                    "retired" => Some(m.get("ts-account-subsidiary-retired")),
                    "pending" => Some(m.get("ts-account-subsidiary-pending")),
                    _ => None,
                };
                (contributions(name), note)
            })
            .collect(),
    });
    (operated_by.is_some() || subsidiaries.is_some()).then_some(AccountsLine {
        operated_by,
        subsidiaries,
    })
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

/// The filter form.
#[allow(clippy::too_many_lines)]
fn form_model(m: &Messages, ask: &Ask) -> FormModel {
    let title = match (ask.which, &ask.user) {
        (Which::RecentChanges, _) => "Special:RecentChanges".to_string(),
        (Which::Log, _) => "Special:Log".to_string(),
        (Which::Contributions, Some(u)) => format!("Special:Contributions/{u}"),
        (Which::Contributions, None) => "Special:Contributions".to_string(),
        (Which::NewPages, _) => "Special:NewPages".to_string(),
    };
    let text = |label: String, name: &str, value: Option<&str>| {
        FormItem::text(label, name, value.unwrap_or(""))
    };
    let bots = || FormItem::check("bots", "1", !ask.hide_bots, m.get("ts-changes-bots"));
    let select = |label: String, name: &str, options: Vec<(String, String)>, current: &str| {
        FormItem::select(
            label,
            name,
            options
                .into_iter()
                .map(|(v, t)| {
                    let selected = v == current;
                    (v, t, selected)
                })
                .collect(),
        )
    };
    let days = || {
        select(
            m.get("ts-changes-days"),
            "days",
            DAYS.iter()
                .map(|d| {
                    (
                        d.to_string(),
                        m.with("ts-changes-days-n", &[&d.to_string()]),
                    )
                })
                .collect(),
            &ask.days.to_string(),
        )
    };
    let limit = || {
        select(
            m.get("ts-changes-limit"),
            "limit",
            LIMITS
                .iter()
                .map(|l| (l.to_string(), l.to_string()))
                .collect(),
            &ask.limit.to_string(),
        )
    };
    let namespace = || {
        let mut namespaces = vec![(String::new(), m.get("ts-changes-namespace-all"))];
        namespaces.extend(
            NAMESPACES
                .iter()
                .map(|(n, msg)| ((*n).to_string(), m.get(msg))),
        );
        select(
            m.get("ts-changes-namespace"),
            "namespace",
            namespaces,
            ask.namespace.as_deref().unwrap_or(""),
        )
    };
    let mut items = Vec::new();
    match ask.which {
        Which::Contributions => {
            items.push(text(
                m.get("ts-contributions-user"),
                "target",
                ask.user.as_deref(),
            ));
        }
        Which::NewPages => {
            items.push(text(
                m.get("ts-newpages-user"),
                "username",
                ask.user.as_deref(),
            ));
            items.push(bots());
        }
        Which::Log => {
            let mut options = vec![(String::new(), m.get("ts-log-type-all"))];
            options.extend(
                LOG_TYPES
                    .iter()
                    .map(|(k, msg)| ((*k).to_string(), m.get(msg))),
            );
            items.push(select(
                m.get("ts-log-type"),
                "type",
                options,
                ask.log_type.as_deref().unwrap_or(""),
            ));
            items.push(text(m.get("ts-log-user"), "user", ask.user.as_deref()));
            items.push(text(m.get("ts-log-page"), "page", ask.page.as_deref()));
            // One setting's history (19 §2.8): the field shows only when the log is
            // narrowed to one, as a settings row's History link narrows it.
            if ask.setting.is_some() {
                items.push(text(
                    m.get("ts-log-setting"),
                    "setting",
                    ask.setting.as_deref(),
                ));
            }
        }
        Which::RecentChanges => {}
    }
    if ask.which == Which::Log {
        items.push(days());
        items.push(limit());
    } else {
        if ask.which != Which::NewPages {
            let mut kinds: Vec<FormItem> = KINDS
                .iter()
                .map(|(kind, msg, _)| {
                    FormItem::check(
                        "kind",
                        kind,
                        ask.kinds.iter().any(|k| k == kind),
                        m.get(msg),
                    )
                })
                .collect();
            kinds.push(bots());
            items.push(FormItem::group(m.get("ts-changes-kinds"), kinds));
        }
        items.push(namespace());
        items.push(days());
        items.push(limit());
    }
    FormModel {
        label: m.get("ts-changes-filter-label"),
        title,
        items,
        apply: m.get("ts-changes-apply"),
    }
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
fn pager_model(m: &Messages, ask: &Ask, next: Option<&str>) -> Option<Pager> {
    if ask.cursor.is_none() && next.is_none() {
        return None;
    }
    Some(Pager {
        label: m.get("ts-search-pages-label"),
        newest: ask
            .cursor
            .as_ref()
            .map(|_| choice(ask.href(None), m.get("ts-changes-newest"))),
        older: next.map(|n| choice(ask.href(Some(n)), m.get("ts-changes-older"))),
    })
}

/// Fetches the page, adding its caching to the context.
async fn fetch(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    ask: &Ask,
) -> Result<ActivityPage, ClientError> {
    let lang = cx.m.lang().to_string();
    // The log's page, as a target: an entity by its ID, a page by the ID `prop=info`
    // gives it; a title with no page the viewer may see has no rows.
    let target = match &ask.page {
        None => None,
        Some(p) => {
            let t = site.client().log_target(incoming, p).await?;
            cx.inputs.push(t.cache);
            match t.value {
                Some(t) => Some(t),
                None => {
                    return Ok(ActivityPage {
                        rows: Vec::new(),
                        next: None,
                    });
                }
            }
        }
    };
    let f = site
        .client()
        .activity(incoming, &ask.query(now_micros(), &lang, target))
        .await?;
    cx.inputs.push(f.cache);
    Ok(f.value)
}

/// The sync strip of 19 §3.9: when syncs are off, says so and offers them.
fn strip_model(m: &Messages, ask: &Ask) -> Option<Strip> {
    if ask.kinds.iter().any(|k| k == "sync") {
        return None;
    }
    let mut with = ask.clone();
    with.kinds.push("sync".into());
    with.cursor = None;
    Some(Strip {
        text: m.get("ts-changes-syncs-hidden"),
        show: choice(with.href(None), m.get("ts-changes-syncs-show")),
    })
}

async fn serve_page(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    subpage: Option<&str>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let title_key = match which {
        Which::RecentChanges => "Special:RecentChanges",
        Which::Contributions => "Special:Contributions",
        Which::Log => "Special:Log",
        Which::NewPages => "Special:NewPages",
    };
    if !cx.features.has(Feature::RecentChanges) {
        return pages::no_page(&cx, headers, title_key.into());
    }
    let ask = Ask::of(which, params, subpage);
    let mut page = None;
    let mut error = None;
    if which != Which::Contributions || ask.user.is_some() {
        match fetch(site, &mut cx, &incoming, &ask).await {
            Ok(p) => page = Some(p),
            Err(ClientError::Api { code, info }) if code == "bad-parameter" => {
                error = Some(info);
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    // The header names the operator relation; it is an addition, so a failed lookup
    // leaves it out (0024 §9, A25).
    let accounts = match (which, &ask.user) {
        (Which::Contributions, Some(u)) => match site.client().relations(&incoming, u).await {
            Ok(f) => {
                cx.inputs.push(f.cache);
                accounts_line(&cx.m, &f.value)
            }
            Err(_) => None,
        },
        _ => None,
    };
    let m = &cx.m;
    let empty = match (&page, &ask.user, which) {
        (Some(p), Some(u), Which::Contributions) if p.rows.is_empty() => {
            Some(m.with("ts-contributions-none", &[u]))
        }
        (Some(p), _, Which::NewPages) if p.rows.is_empty() => Some(m.get("ts-newpages-none")),
        (Some(p), _, _) if p.rows.is_empty() => Some(m.get("ts-changes-none")),
        _ => None,
    };
    let body = cx.draw(
        "special/changes.html",
        ChangesPage {
            form: form_model(m, &ask),
            strip: (which == Which::RecentChanges)
                .then(|| strip_model(m, &ask))
                .flatten(),
            error: error
                .as_deref()
                .map(|e| Message::block(MessageKind::Error, e).model()),
            empty,
            list: page
                .as_ref()
                .and_then(|p| list_model(m, &cx.site, &p.rows, RowActions::of(&cx))),
            pager: page
                .as_ref()
                .filter(|p| !p.rows.is_empty())
                .and_then(|p| pager_model(m, &ask, p.next.as_deref())),
            intro: (page.is_none() && ask.user.is_none() && which == Which::Contributions)
                .then(|| m.get("ts-contributions-intro")),
            accounts,
        },
    );
    let title = match (which, &ask.user) {
        (Which::RecentChanges, _) => m.get("ts-recentchanges-title"),
        (Which::Log, _) => m.get("ts-log-title"),
        (Which::Contributions, Some(u)) => m.with("ts-contributions-title", &[u]),
        (Which::Contributions, None) => m.get("ts-contributions-title-none"),
        (Which::NewPages, _) => m.get("ts-newpages-title"),
    };
    let returnto = match (which, &ask.user) {
        (Which::Contributions, Some(u)) => format!("Special:Contributions/{u}"),
        _ => title_key.to_string(),
    };
    pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title,
        returnto: Some(returnto),
        body,
        ..Page::default()
    })
}

/// `Special:Log`, `Special:Log/{type}`, `Special:Log/{user}`.
pub async fn log(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let subpage = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest)
        .filter(|s| !s.is_empty());
    serve_page(site, headers, peer, Which::Log, subpage, params).await
}

/// `Special:RecentChanges`.
pub async fn recent_changes(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    params: &Params,
) -> Response {
    serve_page(site, headers, peer, Which::RecentChanges, None, params).await
}

/// `Special:NewPages`.
pub async fn new_pages(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    serve_page(site, headers, peer, Which::NewPages, None, params).await
}

/// `Special:Contributions`, `Special:Contributions/{user}`.
pub async fn contributions(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let subpage = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest)
        .filter(|s| !s.is_empty());
    serve_page(site, headers, peer, Which::Contributions, subpage, params).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0024 §9 (19 §2.5): a subsidiary's header names its operator; a primary account's
    /// lists its subsidiaries, retired ones marked; an account with neither has no line.
    #[test]
    fn a_contributions_header_names_the_operator_relation() {
        use triplespace_client::{BotOf, Relations};
        let m = Messages::for_language("en");
        assert_eq!(accounts_line(&m, &Relations::default()), None);
        let bot = accounts_line(
            &m,
            &Relations {
                bot: Some(BotOf {
                    operator: "Alice Smith".into(),
                    status: "active".into(),
                    flagged: true,
                    request_page: None,
                }),
                subsidiaries: vec![],
            },
        )
        .unwrap();
        assert_eq!(bot.operated_by.as_ref().unwrap().before, "Bot operated by ");
        assert_eq!(
            bot.operated_by.as_ref().unwrap().operator.href,
            "/wiki/Special:Contributions/Alice_Smith"
        );
        let person = accounts_line(
            &m,
            &Relations {
                bot: None,
                subsidiaries: vec![
                    ("AliceBot".into(), "active".into()),
                    ("Old Bot".into(), "retired".into()),
                ],
            },
        )
        .unwrap();
        let s = person.subsidiaries.as_ref().unwrap();
        assert_eq!(s.items[1].0.href, "/wiki/Special:Contributions/Old_Bot");
        assert_eq!(s.items[1].1.as_deref(), Some("retired"));
    }

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn the_defaults_leave_syncs_out() {
        let ask = Ask::of(Which::RecentChanges, &params(&[]), None);
        assert_eq!(ask.kinds, vec!["edit", "job", "log"]);
        assert_eq!(ask.days, DEFAULT_DAYS);
        assert_eq!(ask.limit, DEFAULT_LIMIT);
        assert!(!ask.hide_bots);
        let q = ask.query(90 * 86_400 * 1_000_000, "en", None);
        assert_eq!(q.since.as_deref(), Some("1970-03-02T00:00:00Z"));
        assert_eq!(q.kinds, vec!["edit", "job", "log", "erased"]);
    }

    #[test]
    fn a_filtered_form_is_taken_as_given() {
        let ask = Ask::of(
            Which::RecentChanges,
            &params(&[
                ("filtered", "1"),
                ("kind", "edit"),
                ("kind", "sync"),
                ("kind", "nope"),
                ("hidebots", "1"),
                ("namespace", "122"),
                ("days", "7"),
                ("limit", "999"),
                ("continue", "1|2|3"),
            ]),
            None,
        );
        assert_eq!(ask.kinds, vec!["edit", "sync"]);
        assert!(ask.hide_bots);
        assert_eq!(ask.namespace.as_deref(), Some("122"));
        assert_eq!(ask.days, 7);
        assert_eq!(ask.limit, DEFAULT_LIMIT, "an unknown limit falls back");
        assert_eq!(
            ask.href(Some("4|5|6")),
            "/w/index.php?title=Special:RecentChanges&filtered=1&kind=edit&kind=sync&hidebots=1&namespace=122&days=7&limit=50&continue=4%7C5%7C6"
        );
        let empty = Ask::of(Which::RecentChanges, &params(&[("filtered", "1")]), None);
        assert!(empty.kinds.is_empty(), "every kind unticked is none");
    }

    // 0010 §7: a link the page makes (Show them, the pager) keeps bot edits as they were;
    // the default shows them, and `filtered=1` without `bots` hides them.
    #[test]
    fn a_link_keeps_bot_edits_shown() {
        let ask = Ask::of(Which::RecentChanges, &params(&[]), None);
        let mut with = ask.clone();
        with.kinds.push("sync".into());
        let href = with.href(None);
        assert!(href.contains("&bots=1"), "{href}");
        let back: Vec<(String, String)> =
            url::form_urlencoded::parse(href.split_once('?').map_or("", |(_, q)| q).as_bytes())
                .into_owned()
                .collect();
        assert!(!Ask::of(Which::RecentChanges, &Params::new(back), None).hide_bots);
    }

    #[test]
    fn contributions_take_the_user_from_the_subpage_or_the_form() {
        let a = Ask::of(Which::Contributions, &params(&[]), Some("Alice_bot"));
        assert_eq!(a.user.as_deref(), Some("Alice bot"));
        assert_eq!(
            a.href(None),
            "/w/index.php?title=Special:Contributions/Alice%5Fbot&filtered=1&kind=edit&kind=job&kind=log&bots=1&days=30&limit=50"
        );
        let b = Ask::of(Which::Contributions, &params(&[("target", " Bob ")]), None);
        assert_eq!(b.user.as_deref(), Some("Bob"));
        let c = Ask::of(Which::Contributions, &params(&[("target", "")]), None);
        assert_eq!(c.user, None);
    }

    #[test]
    fn new_pages_are_the_creating_edits() {
        let a = Ask::of(
            Which::NewPages,
            &params(&[("username", " Bob "), ("hidebots", "1"), ("kind", "job")]),
            None,
        );
        let q = a.query(10_000_000, "en", None);
        assert_eq!(
            q.kinds,
            ["edit", "erased"],
            "edits only, whatever the form says"
        );
        assert_eq!(q.show, ["!bot", "new"]);
        assert_eq!(q.actor.as_deref(), Some("Bob"));
        assert!(
            a.href(None)
                .starts_with("/w/index.php?title=Special:NewPages&filtered=1&username=Bob")
        );
        let b = Ask::of(Which::NewPages, &params(&[]), None);
        assert_eq!(b.query(10_000_000, "en", None).show, ["new"]);
    }

    // 0085 §4; 19 §2.8: the log narrowed to one setting keeps the filter in its links
    // and is not cut off by days.
    #[test]
    fn the_log_of_one_setting_keeps_its_filter() {
        let mut params = Params::default();
        params.map.insert("type".into(), "settings".into());
        params.map.insert("setting".into(), "site.name".into());
        let ask = Ask::of(Which::Log, &params, None);
        assert_eq!(ask.setting.as_deref(), Some("site.name"));
        assert!(
            ask.href(None)
                .contains("&type=settings&setting=site%2Ename")
        );
        let q = ask.query(1_800_000_000_000_000, "en", None);
        assert_eq!(q.setting.as_deref(), Some("site.name"));
        assert!(q.since.is_none());
        // Anything that is not shaped as a key is not a filter.
        params.map.insert("setting".into(), "site name&x=1".into());
        assert!(Ask::of(Which::Log, &params, None).setting.is_none());
    }

    #[test]
    fn times() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_758_760_000_000_000), "2025-09-25T00:26:40Z");
    }
}
