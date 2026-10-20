//! `Special:RecentChanges` and `Special:Contributions` (0010 §7; 0020 §4; 19 §3.9):
//! two feeds over one list, readable without JavaScript.
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

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use triplespace_client::activity::{ActivityPage, ActivityQuery, ActivityRow};
use triplespace_client::{ClientError, SiteInfo};

use crate::Site;
use crate::codex::{Chip, ChipKind, MessageKind};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, lang_attrs, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};

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
}

/// What the query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    which: Which,
    /// The user, for contributions.
    user: Option<String>,
    kinds: Vec<String>,
    hide_bots: bool,
    namespace: Option<String>,
    days: u32,
    limit: u32,
    cursor: Option<String>,
}

impl Ask {
    fn of(which: Which, params: &Params, subpage: Option<&str>) -> Self {
        let q = &params.map;
        let asked: Vec<String> = params.all("kind").map(str::to_string).collect();
        let kinds = if asked.is_empty() && !q.contains_key("filtered") {
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
        let user = match which {
            Which::Contributions => subpage
                .map(str::to_string)
                .or_else(|| q.get("target").cloned())
                .map(|u| u.replace('_', " ").trim().to_string())
                .filter(|u| !u.is_empty()),
            Which::RecentChanges => None,
        };
        Self {
            which,
            user,
            kinds,
            hide_bots: q.get("hidebots").is_some_and(|v| v != "0"),
            namespace: q
                .get("namespace")
                .filter(|n| NAMESPACES.iter().any(|(k, _)| k == n))
                .cloned(),
            days: pick("days", &DAYS, DEFAULT_DAYS),
            limit: pick("limit", &LIMITS, DEFAULT_LIMIT),
            cursor: q.get("continue").cloned().filter(|c| !c.is_empty()),
        }
    }

    /// The feed to fetch, from `now`.
    fn query(&self, now: u64, lang: &str) -> ActivityQuery {
        let since = now.saturating_sub(u64::from(self.days) * 86_400 * 1_000_000);
        // An erased record was an edit: its gap row goes with the edits (19 §3.4).
        let mut kinds = self.kinds.clone();
        if kinds.iter().any(|k| k == "edit") {
            kinds.push("erased".into());
        }
        ActivityQuery {
            actor: self.user.clone(),
            kinds,
            namespaces: self.namespace.iter().cloned().collect(),
            show: if self.hide_bots {
                vec!["!bot".into()]
            } else {
                Vec::new()
            },
            since: Some(iso8601(since)),
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
            (Which::Contributions, Some(u)) => format!(
                "/w/index.php?title=Special:Contributions/{}",
                utf8_percent_encode(&u.replace(' ', "_"), NON_ALPHANUMERIC)
            ),
            (Which::Contributions, None) => "/w/index.php?title=Special:Contributions".to_string(),
        };
        s.push_str("&filtered=1");
        for k in &self.kinds {
            let _ = write!(s, "&kind={k}");
        }
        if self.hide_bots {
            s.push_str("&hidebots=1");
        }
        if let Some(n) = &self.namespace {
            let _ = write!(s, "&namespace={n}");
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

/// A date from an ISO 8601 timestamp, as the reader's language writes it.
pub(crate) fn date(m: &Messages, iso: &str) -> String {
    let d = iso.split('T').next().unwrap_or(iso);
    let mut it = d.split('-');
    match (
        it.next(),
        it.next().and_then(|x| x.parse::<u32>().ok()),
        it.next().and_then(|x| x.parse::<u32>().ok()),
    ) {
        (Some(y), Some(mo), Some(day)) if (1..=12).contains(&mo) => m.with(
            "ts-time-day",
            &[&day.to_string(), &m.get(&format!("ts-month-{mo}")), y],
        ),
        _ => d.to_string(),
    }
}

/// `HH:MM` from an ISO 8601 timestamp.
pub(crate) fn clock(iso: &str) -> &str {
    iso.split('T').nth(1).and_then(|t| t.get(..5)).unwrap_or("")
}

/// The provider's code for a source such as `mirror/wikidata`.
fn provider_code<'a>(site: &'a SiteInfo, source: &str) -> Option<&'a str> {
    let slug = source.strip_prefix("mirror/")?;
    site.providers
        .iter()
        .find(|p| p.slug == slug)
        .map(|p| p.code.as_str())
}

/// The chip that says where a row came from.
fn origin_chip(m: &Messages, site: &SiteInfo, row: &ActivityRow) -> String {
    let chip = match row.kind.as_str() {
        "sync" => match provider_code(site, &row.source) {
            Some(code) => Chip::new(ChipKind::Provider(code.to_string()), code).render(),
            None => Chip::new(ChipKind::Local, &row.source).render(),
        },
        "job" => Chip::new(ChipKind::Job, &m.get("ts-changes-chip-job")).render(),
        "log" => Chip::new(ChipKind::Log, &m.get("ts-changes-chip-log")).render(),
        _ => Chip::new(ChipKind::Local, &m.get("ts-changes-chip-local")).render(),
    };
    chip.unwrap_or_default()
}

/// The target: its title linked, with its label where one is known.
fn target_html(m: &Messages, row: &ActivityRow) -> String {
    let lang = m.lang();
    let Some(t) = &row.target else {
        return String::new();
    };
    let Some(title) = &t.title else {
        return format!("<span class=\"ts-mono\">{}</span>", esc(&t.id));
    };
    let href = format!("/wiki/{}", title_url(title));
    match &t.label {
        Some(label) => format!(
            "<span class=\"ts-change__part\">{}</span><span class=\"ts-change__part ts-change__id ts-mono\">{}</span>",
            link(
                &href,
                &format!(
                    "<span{}>{}</span>",
                    lang_attrs(&label.language, lang),
                    esc(&label.value)
                ),
                " class=\"ts-change__target\""
            ),
            esc(&t.id)
        ),
        None => format!(
            "<span class=\"ts-change__part\">{}</span>",
            link(&href, &esc(title), " class=\"ts-change__target\"")
        ),
    }
}

/// The actor: a link to their contributions, or the hidden marker.
fn actor_html(m: &Messages, row: &ActivityRow) -> String {
    if row.actor.hidden {
        return format!(
            "<span class=\"ts-change__part ts-change__hidden\">{}</span>",
            esc(&m.get("ts-changes-hidden-user"))
        );
    }
    let Some(name) = row.actor.name.as_deref().or(row.actor.key.as_deref()) else {
        return String::new();
    };
    format!(
        "<span class=\"ts-change__part\">{}</span>",
        link(
            &format!("/wiki/Special:Contributions/{}", title_url(name)),
            &esc(name),
            " class=\"ts-change__actor\"",
        )
    )
}

/// The summary: the user's comment, and the automatic part muted.
fn summary_html(m: &Messages, row: &ActivityRow) -> String {
    if row.summary.hidden {
        return format!(
            "<span class=\"ts-change__hidden\">{}</span>",
            esc(&m.get("ts-changes-hidden-comment"))
        );
    }
    let mut s = String::new();
    let auto = match (&row.summary.op, &row.summary.args) {
        (Some(op), Some(args)) => Some(format!("{op}: {args}")),
        (Some(op), None) => Some(op.clone()),
        _ => None,
    };
    if row.kind == "sync" {
        let provider = row.source.strip_prefix("mirror/").unwrap_or(&row.source);
        let _ = write!(
            s,
            "<span class=\"ts-change__auto\">{}</span>",
            esc(&m.with("ts-changes-synced", &[provider]))
        );
    } else if let Some(a) = auto {
        let _ = write!(s, "<span class=\"ts-change__auto\">{}</span>", esc(&a));
    }
    if let Some(c) = &row.comment
        && !c.is_empty()
    {
        if !s.is_empty() {
            s.push(' ');
        }
        let _ = write!(s, "<span class=\"ts-change__comment\">{}</span>", esc(c));
    }
    s
}

/// The size change: `+20`, `−5`, or the size when nothing came before.
fn size_html(row: &ActivityRow) -> String {
    match (row.delta, row.size) {
        (Some(d), _) if d != 0 => {
            let class = if d > 0 { "pos" } else { "neg" };
            let sign = if d > 0 { "+" } else { "\u{2212}" };
            format!(
                "<span class=\"ts-change__delta ts-change__delta--{class}\" dir=\"ltr\">{sign}{}</span>",
                d.abs()
            )
        }
        (Some(_), _) => "<span class=\"ts-change__delta\" dir=\"ltr\">0</span>".to_string(),
        (None, Some(size)) => {
            format!("<span class=\"ts-change__delta\" dir=\"ltr\">({size})</span>")
        }
        (None, None) => String::new(),
    }
}

/// One row.
/// The diff page of a row (19 §3.8): `?diff={revid}` for a local edit, `?diff={partition}:{offset}`
/// for a mirror sync, which has no local revision ID; nothing for the other kinds.
fn diff_href(row: &ActivityRow) -> Option<String> {
    let title = row.target.as_ref()?.title.as_deref()?;
    let arg = match row.kind.as_str() {
        "edit" if row.source == "local" || row.source == "pages" => row.revid?.to_string(),
        "sync" => format!("{}:{}", row.record.partition, row.record.offset),
        _ => return None,
    };
    Some(format!("/wiki/{}?diff={arg}", title_url(title)))
}

fn row_html(m: &Messages, site: &SiteInfo, row: &ActivityRow) -> String {
    let time = format!(
        "<time class=\"ts-change__time\" datetime=\"{}\">{}</time>",
        esc(&row.time),
        esc(clock(&row.time))
    );
    let time = match diff_href(row) {
        Some(href) => link(&href, &time, " class=\"ts-change__diff\""),
        None => time,
    };
    let mut s = format!(
        "<li class=\"ts-change ts-change--{}\"><span class=\"ts-change__part\">{time}</span><span class=\"ts-change__part\">{}</span>",
        esc(&row.kind),
        origin_chip(m, site, row)
    );
    if row.kind == "erased" {
        let _ = write!(
            s,
            "<span class=\"ts-change__hidden\">{}</span></li>",
            esc(&m.get("ts-changes-erased"))
        );
        return s;
    }
    let mut flags = Vec::new();
    if row.flags.new {
        flags.push(("ts-changes-flag-new", "new"));
    }
    if row.flags.minor {
        flags.push(("ts-changes-flag-minor", "minor"));
    }
    if row.flags.bot {
        flags.push(("ts-changes-flag-bot", "bot"));
    }
    if !flags.is_empty() {
        s.push_str("<span class=\"ts-change__flags\">");
        for (msg, class) in flags {
            let _ = write!(
                s,
                "<abbr class=\"ts-change__flag ts-change__flag--{class}\" title=\"{}\">{}</abbr>",
                esc(&m.get(&format!("{msg}-title"))),
                esc(&m.get(msg))
            );
        }
        s.push_str("</span>");
    }
    let target = target_html(m, row);
    if !target.is_empty() {
        s.push_str(&target);
    } else if let Some(j) = row.job {
        let _ = write!(
            s,
            "<span class=\"ts-change__part ts-change__target\">{}</span>",
            esc(&m.with("ts-changes-job", &[&j.to_string()]))
        );
    }
    let delta = size_html(row);
    if !delta.is_empty() {
        s.push_str(&delta);
    }
    let actor = actor_html(m, row);
    if !actor.is_empty() {
        s.push_str(&actor);
    }
    let summary = summary_html(m, row);
    if !summary.is_empty() {
        let _ = write!(s, "<span class=\"ts-change__summary\">{summary}</span>");
    }
    let tags: Vec<&String> = row.tags.iter().filter(|t| *t != "bot").collect();
    if !tags.is_empty() {
        s.push_str("<span class=\"ts-change__tags\">");
        for t in tags {
            let _ = write!(s, "<span class=\"ts-change__tag\">{}</span>", esc(t));
        }
        s.push_str("</span>");
    }
    s.push_str("</li>");
    s
}

/// The rows, grouped by day.
pub(crate) fn list_html(m: &Messages, site: &SiteInfo, rows: &[ActivityRow]) -> String {
    let mut s = String::new();
    let mut day: Option<&str> = None;
    for row in rows {
        let d = row.time.split('T').next().unwrap_or("");
        if day != Some(d) {
            if day.is_some() {
                s.push_str("</ul>");
            }
            let _ = write!(
                s,
                "<h3 class=\"ts-changes__day\">{}</h3><ul class=\"ts-changes__list\">",
                esc(&date(m, &row.time))
            );
            day = Some(d);
        }
        s.push_str(&row_html(m, site, row));
    }
    if day.is_some() {
        s.push_str("</ul>");
    }
    s
}

/// The filter form.
#[allow(clippy::too_many_lines)]
fn form_html(m: &Messages, ask: &Ask) -> String {
    let title = match (ask.which, &ask.user) {
        (Which::RecentChanges, _) => "Special:RecentChanges".to_string(),
        (Which::Contributions, Some(u)) => format!("Special:Contributions/{u}"),
        (Which::Contributions, None) => "Special:Contributions".to_string(),
    };
    let mut s = format!(
        "<form class=\"ts-changes-form\" action=\"/w/index.php\" method=\"get\" aria-label=\"{}\"><input type=\"hidden\" name=\"title\" value=\"{}\"><input type=\"hidden\" name=\"filtered\" value=\"1\">",
        esc(&m.get("ts-changes-filter-label")),
        esc(&title)
    );
    if ask.which == Which::Contributions {
        let _ = write!(
            s,
            "<label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"target\" value=\"{}\"></label>",
            esc(&m.get("ts-contributions-user")),
            esc(ask.user.as_deref().unwrap_or(""))
        );
    }
    s.push_str("<fieldset class=\"ts-changes-form__kinds\"><legend>");
    s.push_str(&esc(&m.get("ts-changes-kinds")));
    s.push_str("</legend>");
    for (kind, msg, _) in KINDS {
        let _ = write!(
            s,
            "<label class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" name=\"kind\" value=\"{kind}\"{}><span class=\"cdx-checkbox__icon\"></span><span class=\"cdx-checkbox__label\">{}</span></label>",
            if ask.kinds.iter().any(|k| k == kind) {
                " checked"
            } else {
                ""
            },
            esc(&m.get(msg))
        );
    }
    let _ = write!(
        s,
        "<label class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" name=\"hidebots\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><span class=\"cdx-checkbox__label\">{}</span></label></fieldset>",
        if ask.hide_bots { " checked" } else { "" },
        esc(&m.get("ts-changes-hide-bots"))
    );
    let select = |s: &mut String,
                  name: &str,
                  label: &str,
                  options: &[(String, String)],
                  current: &str| {
        let _ = write!(
            s,
            "<label class=\"ts-changes-form__field\"><span>{}</span> <select class=\"cdx-select\" name=\"{name}\">",
            esc(label)
        );
        for (value, text) in options {
            let _ = write!(
                s,
                "<option value=\"{}\"{}>{}</option>",
                esc(value),
                if value == current { " selected" } else { "" },
                esc(text)
            );
        }
        s.push_str("</select></label>");
    };
    let mut namespaces = vec![(String::new(), m.get("ts-changes-namespace-all"))];
    namespaces.extend(
        NAMESPACES
            .iter()
            .map(|(n, msg)| ((*n).to_string(), m.get(msg))),
    );
    select(
        &mut s,
        "namespace",
        &m.get("ts-changes-namespace"),
        &namespaces,
        ask.namespace.as_deref().unwrap_or(""),
    );
    let days: Vec<(String, String)> = DAYS
        .iter()
        .map(|d| {
            (
                d.to_string(),
                m.with("ts-changes-days-n", &[&d.to_string()]),
            )
        })
        .collect();
    select(
        &mut s,
        "days",
        &m.get("ts-changes-days"),
        &days,
        &ask.days.to_string(),
    );
    let limits: Vec<(String, String)> = LIMITS
        .iter()
        .map(|l| (l.to_string(), l.to_string()))
        .collect();
    select(
        &mut s,
        "limit",
        &m.get("ts-changes-limit"),
        &limits,
        &ask.limit.to_string(),
    );
    let _ = write!(
        s,
        " <button class=\"cdx-button cdx-button--action-progressive cdx-button--weight-primary\" type=\"submit\">{}</button></form>",
        esc(&m.get("ts-changes-apply"))
    );
    s
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
fn pager_html(m: &Messages, ask: &Ask, next: Option<&str>) -> String {
    if ask.cursor.is_none() && next.is_none() {
        return String::new();
    }
    let mut s = format!(
        "<nav class=\"ts-pager\" aria-label=\"{}\">",
        esc(&m.get("ts-search-pages-label"))
    );
    if ask.cursor.is_some() {
        let _ = write!(
            s,
            "{} ",
            link(&ask.href(None), &esc(&m.get("ts-changes-newest")), "")
        );
    }
    if let Some(n) = next {
        s.push_str(&link(
            &ask.href(Some(n)),
            &esc(&m.get("ts-changes-older")),
            " rel=\"next\"",
        ));
    }
    s.push_str("</nav>");
    s
}

/// Fetches the page, adding its caching to the context.
async fn fetch(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    ask: &Ask,
) -> Result<ActivityPage, ClientError> {
    let lang = cx.m.lang().to_string();
    let f = site
        .client()
        .activity(incoming, &ask.query(now_micros(), &lang))
        .await?;
    cx.inputs.push(f.cache);
    Ok(f.value)
}

/// The sync strip of 19 §3.9: when syncs are off, says so and offers them.
fn sync_strip(m: &Messages, ask: &Ask) -> String {
    if ask.kinds.iter().any(|k| k == "sync") {
        return String::new();
    }
    let mut with = ask.clone();
    with.kinds.push("sync".into());
    with.cursor = None;
    format!(
        "<p class=\"ts-changes__strip\">{} {}</p>",
        esc(&m.get("ts-changes-syncs-hidden")),
        link(&with.href(None), &esc(&m.get("ts-changes-syncs-show")), "")
    )
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
    };
    if !cx.features.has(Feature::RecentChanges) {
        return pages::no_page(&cx, headers, title_key.into());
    }
    let ask = Ask::of(which, params, subpage);
    let mut page = None;
    let mut error = None;
    if which == Which::RecentChanges || ask.user.is_some() {
        match fetch(site, &mut cx, &incoming, &ask).await {
            Ok(p) => page = Some(p),
            Err(ClientError::Api { code, info }) if code == "bad-parameter" => {
                error = Some(info);
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    let m = &cx.m;
    let mut body = form_html(m, &ask);
    if which == Which::RecentChanges {
        body.push_str(&sync_strip(m, &ask));
    }
    if let Some(e) = &error {
        body.push_str(&pages::message(MessageKind::Error, e));
    }
    match (&page, &ask.user, which) {
        (Some(p), _, _) if p.rows.is_empty() => {
            let text = match (&ask.user, which) {
                (Some(u), Which::Contributions) => m.with("ts-contributions-none", &[u]),
                _ => m.get("ts-changes-none"),
            };
            let _ = write!(body, "<p class=\"ts-empty\">{}</p>", esc(&text));
        }
        (Some(p), _, _) => {
            body.push_str(&list_html(m, &cx.site, &p.rows));
            body.push_str(&pager_html(m, &ask, p.next.as_deref()));
        }
        (None, None, Which::Contributions) => {
            let _ = write!(
                body,
                "<p class=\"ts-search-intro\">{}</p>",
                esc(&m.get("ts-contributions-intro"))
            );
        }
        (None, _, _) => {}
    }
    let title = match (which, &ask.user) {
        (Which::RecentChanges, _) => m.get("ts-recentchanges-title"),
        (Which::Contributions, Some(u)) => m.with("ts-contributions-title", &[u]),
        (Which::Contributions, None) => m.get("ts-contributions-title-none"),
    };
    let returnto = match (which, &ask.user) {
        (Which::Contributions, Some(u)) => format!("Special:Contributions/{u}"),
        _ => title_key.to_string(),
    };
    let body = format!("<div class=\"ts-changes\">{body}</div>");
    pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title,
        returnto: Some(returnto),
        body,
        ..Page::default()
    })
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
        let q = ask.query(90 * 86_400 * 1_000_000, "en");
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

    #[test]
    fn contributions_take_the_user_from_the_subpage_or_the_form() {
        let a = Ask::of(Which::Contributions, &params(&[]), Some("Alice_bot"));
        assert_eq!(a.user.as_deref(), Some("Alice bot"));
        assert_eq!(
            a.href(None),
            "/w/index.php?title=Special:Contributions/Alice%5Fbot&filtered=1&kind=edit&kind=job&kind=log&days=30&limit=50"
        );
        let b = Ask::of(Which::Contributions, &params(&[("target", " Bob ")]), None);
        assert_eq!(b.user.as_deref(), Some("Bob"));
        let c = Ask::of(Which::Contributions, &params(&[("target", "")]), None);
        assert_eq!(c.user, None);
    }

    #[test]
    fn times() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_758_760_000_000_000), "2025-09-25T00:26:40Z");
        assert_eq!(clock("2025-09-25T00:26:40Z"), "00:26");
    }

    #[test]
    fn sizes() {
        let mut r = ActivityRow {
            delta: Some(20),
            size: Some(400),
            ..ActivityRow::default()
        };
        assert!(size_html(&r).contains("+20"));
        r.delta = Some(-5);
        assert!(size_html(&r).contains("\u{2212}5"));
        r.delta = None;
        assert!(size_html(&r).contains("(400)"));
    }
}
