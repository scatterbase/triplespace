//! The change list: activity rows as every list of changes draws them (19 §3.4), grouped
//! by day. `Special:RecentChanges` and the other feeds ([`crate::changelist`]), an
//! entity's, a document page's and a talk page's history draw the same rows: the time, a
//! chip for where the record came from, the target with its label, the actor, the
//! summary, the size change and the tags.

use serde::Serialize;
use triplespace_client::SiteInfo;
use triplespace_client::activity::{ActivityRow, UpstreamRef};

use crate::choice::Choice;
use crate::codex::{Chip, ChipKind, ChipModel};
use crate::features::Feature;
use crate::frame::title_url;
use crate::i18n::Messages;
use crate::pages::Context;

/// A date from an ISO 8601 timestamp, as the reader's language writes it.
pub fn date(m: &Messages, iso: &str) -> String {
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
pub fn clock(iso: &str) -> &str {
    iso.split('T').nth(1).and_then(|t| t.get(..5)).unwrap_or("")
}

/// A job page's URL: where a row made by a job links (`Special:Jobs/{id}`).
#[must_use]
pub fn job_href(id: u64) -> String {
    format!("/wiki/Special:Jobs/{id}")
}

/// The provider's code for a source such as `mirror/wikidata`.
fn provider_code<'a>(site: &'a SiteInfo, source: &str) -> Option<&'a str> {
    let slug = source.strip_prefix("mirror/")?;
    site.providers
        .iter()
        .find(|p| p.slug == slug)
        .map(|p| p.code.as_str())
}

/// An account, as a target or as the actor (0095 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AccountModel {
    /// The instance's own key: the Server Operator badge.
    Operator {
        /// The badge.
        badge: ChipModel,
    },
    /// A person: their name linked to their contributions.
    Named {
        /// The person icon's hidden text, where the account is a target.
        icon: Option<String>,
        /// The key, as the name's tooltip, where the wiki named the key.
        title: Option<String>,
        /// The contributions page.
        href: String,
        /// The name shown.
        shown: String,
        /// Whether it is the target (`ts-change__target`) or the actor.
        target: bool,
        /// The key beside the name, for another wiki's account the wiki has not named.
        key: Option<String>,
    },
}

/// A row's target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TargetModel {
    /// An account.
    Account {
        /// The account.
        account: AccountModel,
    },
    /// An ID with no title.
    Id {
        /// The ID.
        id: String,
    },
    /// A job, linked to its page (0104 §9).
    Job {
        /// The job's page.
        href: String,
        /// "Job n".
        text: String,
    },
    /// A title with its label.
    Labelled {
        /// The page.
        href: String,
        /// The label's language, where it differs from the page's.
        lang: Option<String>,
        /// The label.
        label: String,
        /// The ID beside it.
        id: String,
    },
    /// A title alone.
    Titled {
        /// The page.
        href: String,
        /// The title as shown.
        text: String,
    },
}

/// A row's actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActorModel {
    /// Hidden.
    Hidden {
        /// What the row says.
        text: String,
    },
    /// An account.
    Account {
        /// The account.
        account: AccountModel,
    },
    /// None known.
    None,
}

/// A row's summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummaryModel {
    /// What the row says of a hidden summary; the rest is empty then.
    pub hidden: Option<String>,
    /// The action, in its own column beside the summary: a record's operation and its
    /// arguments, a log event's type and action, a thread record in words.
    pub auto: Option<String>,
    /// Where a move went.
    pub moved: Option<String>,
    /// The user's comment, parsed as MediaWiki parses one ([`crate::comment`]).
    pub comment: Vec<crate::comment::Part>,
}

/// A flag: new, minor, bot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flag {
    /// `new`, `minor` or `bot`.
    pub class: &'static str,
    /// The tooltip.
    pub title: String,
    /// The letter.
    pub text: String,
}

/// The size change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Size {
    /// `pos` or `neg`, for a change.
    pub class: Option<&'static str>,
    /// `+20`, `−5`, `0` or `(400)`.
    pub text: String,
}

/// One row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowModel {
    /// The kind: `edit`, `sync`, `job`, `log`, `erased`.
    pub kind: String,
    /// Whether the viewer has not seen it (the watchlist).
    pub unseen: bool,
    /// ISO 8601.
    pub datetime: String,
    /// `HH:MM`.
    pub time: String,
    /// The diff page, where the row has one.
    pub diff: Option<String>,
    /// The chip that says where it came from.
    pub origin: ChipModel,
    /// What an erased row says, spanning the other columns.
    pub erased: Option<(usize, String)>,
    /// The flags.
    pub flags: Vec<Flag>,
    /// The target.
    pub target: Option<TargetModel>,
    /// The size change.
    pub size: Option<Size>,
    /// The actor.
    pub actor: ActorModel,
    /// The summary.
    pub summary: SummaryModel,
    /// The tags, but for `bot`.
    pub tags: Vec<String>,
    /// The history's compare cell, where the list has the column (0010 §5.6).
    pub pick: Option<Pick>,
    /// The job that wrote an adopted or imported revision (0008 A50): "Job n", linked to
    /// its page.
    pub job: Option<JobLink>,
    /// The row's menu, where the viewer may act on its actor or its target.
    pub menu: Option<RowMenu>,
    /// Whether the list has the menu's column, so that the row has its cell.
    pub menu_cell: bool,
}

/// A row's menu: its accessible name and its links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowMenu {
    /// The button's accessible name.
    pub label: String,
    /// The links.
    pub items: Vec<Choice>,
}

/// "Job n", linked to `Special:Jobs/{n}` (0104 §9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobLink {
    /// The job's page.
    pub href: String,
    /// "Job n".
    pub text: String,
}

impl JobLink {
    /// The link to a job.
    #[must_use]
    pub fn to(m: &Messages, job: u64) -> Self {
        Self {
            href: job_href(job),
            text: m.with("ts-changes-job", &[&job.to_string()]),
        }
    }
}

/// A history row's compare cell: the radio buttons that choose its revision as the older
/// or the newer side, either absent where the row cannot be that side (19 §3.7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Pick {
    /// The older side: `oldid`.
    pub old: Option<Radio>,
    /// The newer side: `diff`.
    pub new: Option<Radio>,
}

/// One radio button of a compare cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Radio {
    /// The revision.
    pub revid: u64,
    /// The accessible name.
    pub label: String,
    /// Whether it is chosen to begin with.
    pub checked: bool,
}

/// A day's rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Day {
    /// The date, as the reader's language writes it.
    pub date: String,
    /// The rows.
    pub rows: Vec<RowModel>,
}

/// The changes table: the columns and the rows by day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListModel {
    /// The columns: class and heading.
    pub columns: Vec<(&'static str, String)>,
    /// The days.
    pub days: Vec<Day>,
}

/// The chip that says where a row came from.
fn origin_chip(m: &Messages, site: &SiteInfo, row: &ActivityRow) -> ChipModel {
    match row.kind.as_str() {
        "sync" => match provider_code(site, &row.source) {
            Some(code) => Chip::new(ChipKind::Provider(code.to_string()), code).model(),
            None => Chip::new(ChipKind::Local, &row.source).model(),
        },
        "job" => Chip::new(ChipKind::Job, &m.get("ts-changes-chip-job")).model(),
        "log" => Chip::new(ChipKind::Log, &m.get("ts-changes-chip-log")).model(),
        // An adopted or imported revision (0008 A50): the chip names the upstream
        // revision and the job in its tooltip.
        _ => match &row.upstream {
            Some(up) => imported_chip(m, up, row.job),
            None => Chip::new(ChipKind::Local, &m.get("ts-changes-chip-local")).model(),
        },
    }
}

/// The chip of an adopted or imported revision (0008 A50).
pub fn imported_chip(m: &Messages, up: &UpstreamRef, job: Option<u64>) -> ChipModel {
    let text = m.get("ts-changes-chip-imported");
    let title = match job {
        Some(j) => m.with(
            "ts-changes-chip-imported-job-title",
            &[&up.revid.to_string(), &up.time, &j.to_string()],
        ),
        None => m.with(
            "ts-changes-chip-imported-title",
            &[&up.revid.to_string(), &up.time],
        ),
    };
    Chip {
        kind: ChipKind::Imported,
        text: &text,
        title: Some(&title),
    }
    .model()
}

/// The instance's own actor key, `instance:{farm code}`, which the site shows as the
/// **Server Operator** (0095 §3).
fn operator_key(site: &SiteInfo) -> Option<String> {
    (!site.farm_code.is_empty()).then(|| format!("instance:{}", site.farm_code))
}

/// An account, as a target or as the actor (0095 §3): the instance's own key as the
/// Server Operator badge; else a person icon, marking it as the account and not its user
/// page, and its name linked to its contributions. The name is the account's username
/// where the row gives it, else the wiki's name for the key (`Interface:Name/{key}`), with
/// the key as its tooltip, else the key. The key is shown beside the name only for an
/// account of another wiki that the wiki has not named. A key of this wiki's issuer, or
/// `local`, is this wiki's.
fn account_model(
    m: &Messages,
    site: &SiteInfo,
    key: Option<&str>,
    username: Option<&str>,
    icon: bool,
) -> Option<AccountModel> {
    if let (Some(k), Some(op)) = (key, operator_key(site))
        && k == op
    {
        return Some(AccountModel::Operator {
            badge: Chip {
                kind: ChipKind::Operator,
                text: &m.get("ts-server-operator"),
                title: Some(k),
            }
            .model(),
        });
    }
    let named = key.and_then(|k| m.name_of(k));
    let shown = username
        .map(str::to_string)
        .or(named.clone())
        .or(key.map(str::to_string))?;
    // An API that reports no issuer code (a stock Wikibase) is taken to be naming its own.
    let ours = site.issuer.is_empty()
        || key
            .and_then(|k| k.split_once(':'))
            .is_none_or(|(issuer, _)| issuer == "local" || issuer == site.issuer);
    let show_key = key.filter(|_| !ours && named.is_none() && username.is_some());
    // A name the wiki gave the key carries the key as its tooltip.
    let title = key
        .filter(|_| username.is_none() && named.is_some())
        .map(str::to_string);
    let who = username.or(key).unwrap_or(&shown);
    Some(AccountModel::Named {
        icon: icon.then(|| m.get("ts-changes-account")),
        title,
        href: format!("/wiki/Special:Contributions/{}", title_url(who)),
        shown: shown.clone(),
        target: icon,
        key: show_key.map(str::to_string),
    })
}

/// The target: its title linked, with its label where one is known. An account is
/// [`account_model`]'s. In a discussion's history (`subject_only`), a thread is named by
/// its subject, the date and namespace being the same on every row.
fn target_model(
    m: &Messages,
    site: &SiteInfo,
    row: &ActivityRow,
    subject_only: bool,
) -> Option<TargetModel> {
    let lang = m.lang();
    let t = row.target.as_ref()?;
    if t.kind == "actor" {
        return account_model(
            m,
            site,
            Some(t.id.as_str()).filter(|k| !k.is_empty()),
            t.label.as_ref().map(|l| l.value.as_str()),
            true,
        )
        .map(|account| TargetModel::Account { account });
    }
    let Some(title) = &t.title else {
        return Some(TargetModel::Id { id: t.id.clone() });
    };
    if let Some(n) = title.strip_prefix("Special:Jobs/") {
        return Some(TargetModel::Job {
            href: format!("/wiki/{}", title_url(title)),
            text: m.with("ts-changes-job", &[n]),
        });
    }
    let href = format!("/wiki/{}", title_url(title));
    if let Some(label) = &t.label {
        return Some(TargetModel::Labelled {
            href,
            lang: (!label.language.is_empty() && label.language != lang)
                .then(|| label.language.clone()),
            label: label.value.clone(),
            id: t.id.clone(),
        });
    }
    let shown = if subject_only && is_thread_row(row) {
        title.split_once('/').map_or(title.as_str(), |(_, s)| s)
    } else {
        title.as_str()
    };
    Some(TargetModel::Titled {
        href,
        text: shown.to_string(),
    })
}

/// The actor: a link to their contributions, the Server Operator badge for the instance,
/// or the hidden marker.
fn actor_model(m: &Messages, site: &SiteInfo, row: &ActivityRow) -> ActorModel {
    if row.actor.hidden {
        return ActorModel::Hidden {
            text: m.get("ts-changes-hidden-user"),
        };
    }
    account_model(
        m,
        site,
        row.actor.key.as_deref(),
        row.actor.name.as_deref(),
        false,
    )
    .map_or(ActorModel::None, |account| ActorModel::Account { account })
}

/// Whether a row is a thread's: its target is a page in the `Thread` namespace.
fn is_thread_row(row: &ActivityRow) -> bool {
    row.target
        .as_ref()
        .and_then(|t| t.title.as_deref())
        .is_some_and(|t| t.starts_with("Thread:"))
}

/// A thread record's automatic summary in words (16 §6.1): what the record did, and for a
/// listing or a move, relative to the talk page whose history this is (`here`), so that a
/// history reads "listed the thread here" or "moved the thread away".
fn thread_summary(m: &Messages, row: &ActivityRow, here: Option<u64>) -> Option<String> {
    let op = row.summary.op.as_deref()?;
    let param = |k: &str| {
        row.log
            .as_ref()
            .and_then(|l| l.params.as_ref())
            .and_then(|p| p.get(k))
            .and_then(serde_json::Value::as_u64)
    };
    let is_here = |k: &str| here.is_some() && param(k) == here;
    let key = match op {
        "create" => "ts-changes-thread-create",
        "post" => "ts-changes-thread-post",
        "edit" => "ts-changes-thread-edit",
        "rename" => {
            return Some(m.with(
                "ts-changes-thread-rename",
                &[row.summary.args.as_deref().unwrap_or_default()],
            ));
        }
        "move" if is_here("from_talk") => "ts-changes-thread-move-away",
        "move" if is_here("talk") => "ts-changes-thread-move-here",
        "move" => "ts-changes-thread-move",
        "attach" if is_here("talk") => "ts-changes-thread-attach-here",
        "attach" => "ts-changes-thread-attach",
        "detach" if is_here("talk") => "ts-changes-thread-detach-here",
        "detach" => "ts-changes-thread-detach",
        "pin" => "ts-changes-thread-pin",
        "unpin" => "ts-changes-thread-unpin",
        _ => return None,
    };
    Some(m.get(key))
}

/// A `settings/change` event in words (16 §2.3): each setting it changed with its value
/// before and after, "site.name: Old → New", an unset value as a dash.
fn settings_summary(m: &Messages, row: &ActivityRow) -> Option<String> {
    let log = row.log.as_ref().filter(|l| l.r#type == "settings")?;
    let changes = log.params.as_ref()?.get("changes")?.as_array()?;
    let shown = |v: &serde_json::Value| match v {
        serde_json::Value::Null => m.get("ts-log-settings-unset"),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let parts: Vec<String> = changes
        .iter()
        .filter_map(|c| {
            let key = c.get("key")?.as_str()?;
            Some(m.with(
                "ts-log-settings-change",
                &[
                    key,
                    &shown(c.get("old").unwrap_or(&serde_json::Value::Null)),
                    &shown(c.get("new").unwrap_or(&serde_json::Value::Null)),
                ],
            ))
        })
        .collect();
    parts
        .into_iter()
        .reduce(|list, next| m.with("ts-log-settings-list", &[&list, &next]))
}

/// The summary: the user's comment, and the automatic part muted.
fn summary_model(m: &Messages, row: &ActivityRow, here: Option<u64>) -> SummaryModel {
    let mut out = SummaryModel {
        hidden: None,
        auto: None,
        moved: None,
        comment: Vec::new(),
    };
    if row.summary.hidden {
        out.hidden = Some(m.get("ts-changes-hidden-comment"));
        return out;
    }
    let auto = match (&row.summary.op, &row.summary.args) {
        (Some(op), Some(args)) => Some(format!("{op}: {args}")),
        (Some(op), None) => Some(op.clone()),
        _ => None,
    };
    out.auto = if row.kind == "sync" {
        let provider = row.source.strip_prefix("mirror/").unwrap_or(&row.source);
        Some(m.with("ts-changes-synced", &[provider]))
    } else if let Some(t) = is_thread_row(row)
        .then(|| thread_summary(m, row, here))
        .flatten()
    {
        Some(t)
    } else if let Some(t) = settings_summary(m, row) {
        Some(t)
    } else {
        auto
    };
    // A move names where the page went (10 §2.5).
    out.moved = row
        .log
        .as_ref()
        .filter(|l| l.r#type == "move" && !is_thread_row(row))
        .and_then(|l| {
            let p = l.params.as_ref()?;
            Some((
                p.get("ts-from-title")?.as_str()?,
                p.get("target_title")?.as_str()?,
            ))
        })
        .map(|(from, to)| m.with("ts-changes-moved-to", &[from, to]));
    // An automatic comment in a page's summary names one of its sections.
    let page = row
        .target
        .as_ref()
        .filter(|t| t.kind == "page")
        .and_then(|t| t.title.as_deref());
    out.comment = row
        .comment
        .as_deref()
        .map(|c| crate::comment::parts(m, c, page))
        .unwrap_or_default();
    out
}

/// The size change: `+20`, `−5`, or the size when nothing came before.
fn size_model(row: &ActivityRow) -> Option<Size> {
    match (row.delta, row.size) {
        (Some(d), _) if d != 0 => Some(Size {
            class: Some(if d > 0 { "pos" } else { "neg" }),
            text: format!("{}{}", if d > 0 { "+" } else { "\u{2212}" }, d.abs()),
        }),
        (Some(_), _) => Some(Size {
            class: None,
            text: "0".into(),
        }),
        (None, Some(size)) => Some(Size {
            class: None,
            text: format!("({size})"),
        }),
        (None, None) => None,
    }
}

/// The revision a row can be compared by, where it has one: a local edit's (19 §3.7). A
/// mirror's observed states have no revision ID to compare yet.
pub fn comparable(row: &ActivityRow) -> Option<u64> {
    if is_thread_row(row) || row.kind != "edit" {
        return None;
    }
    if row.source == "local" || row.source == "pages" {
        row.revid
    } else {
        None
    }
}

/// The diff page of a row (19 §3.8): `?diff={revid}` for a local edit, `?diff={partition}:{offset}`
/// for a mirror sync, which has no local revision ID; nothing for the other kinds.
fn diff_href(row: &ActivityRow) -> Option<String> {
    let title = row.target.as_ref()?.title.as_deref()?;
    // A thread's rows go to the post they wrote, or to the thread: a thread has no diffs
    // yet (`compare` refuses its content).
    if is_thread_row(row) {
        let href = format!("/wiki/{}", title_url(title));
        return Some(match (row.summary.op.as_deref(), row.revid) {
            (Some("create" | "post"), Some(r)) => format!("{href}#post-{r}"),
            _ => href,
        });
    }
    let arg = match row.kind.as_str() {
        "edit" if row.source == "local" || row.source == "pages" => row.revid?.to_string(),
        "sync" => format!("{}:{}", row.record.partition, row.record.offset),
        _ => return None,
    };
    Some(format!("/wiki/{}?diff={arg}", title_url(title)))
}

/// The columns of the changes table, by the message that heads each. Every row has a
/// cell in each, empty where the row has nothing for it, so the fields line up.
const COLUMNS: [(&str, &str); 8] = [
    ("ts-changes-col-time", "time"),
    ("ts-changes-col-origin", "origin"),
    ("ts-changes-col-flags", "flags"),
    ("ts-changes-col-target", "target"),
    ("ts-changes-col-size", "size"),
    ("ts-changes-col-user", "user"),
    ("ts-changes-col-action", "action"),
    ("ts-changes-col-summary", "summary"),
];

/// What a row's menu may offer the viewer (19 §3.4): each a moderation page the viewer's
/// rights open and the wiki serves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowActions {
    /// `Special:Block`, for a holder of `block`.
    pub block: bool,
    /// The Protect form, for a holder of `protect`.
    pub protect: bool,
    /// The Delete form, for a holder of `delete`.
    pub delete: bool,
}

impl RowActions {
    /// What the page's viewer may be offered.
    #[must_use]
    pub fn of(cx: &Context) -> Self {
        let tabs = cx.moderation_tabs();
        Self {
            block: cx.holds("block") && cx.features.has(Feature::Block),
            protect: tabs.protect,
            delete: tabs.delete,
        }
    }

    fn any(self) -> bool {
        self.block || self.protect || self.delete
    }
}

/// A row's menu (19 §3.4): links to the moderation pages with the row's actor and target
/// filled in, for those whose rights open them. Empty where the row has nothing to act
/// on: a hidden or nameless actor, the instance's own key, a target without a title.
fn menu_model(m: &Messages, row: &ActivityRow, actions: RowActions) -> Option<RowMenu> {
    let mut items = Vec::new();
    let actor = row
        .actor
        .name
        .as_deref()
        .filter(|_| !row.actor.hidden && row.actor.kind.as_deref() != Some("instance"));
    if let (true, Some(name)) = (actions.block, actor) {
        items.push(Choice {
            href: format!("/wiki/Special:Block/{}", title_url(name)),
            label: m.with("ts-changes-menu-block", &[name]),
            current: false,
        });
    }
    // A page or an entity: what has a title to protect or delete. A thread is moderated
    // from its own page.
    let title = row
        .target
        .as_ref()
        .filter(|t| t.kind == "page" || t.kind == "entity")
        .and_then(|t| t.title.as_deref())
        .filter(|_| !is_thread_row(row));
    if let Some(title) = title {
        let base = format!("/wiki/{}", title_url(title));
        if actions.protect {
            items.push(Choice {
                href: format!("{base}?action=protect"),
                label: m.with("ts-changes-menu-protect", &[title]),
                current: false,
            });
        }
        if actions.delete {
            items.push(Choice {
                href: format!("{base}?action=delete"),
                label: m.with("ts-changes-menu-delete", &[title]),
                current: false,
            });
        }
    }
    (!items.is_empty()).then(|| RowMenu {
        label: m.get("ts-changes-menu"),
        items,
    })
}

fn row_model(
    m: &Messages,
    site: &SiteInfo,
    row: &ActivityRow,
    here: Option<u64>,
    actions: RowActions,
) -> RowModel {
    let origin = origin_chip(m, site, row);
    if row.kind == "erased" {
        return RowModel {
            kind: row.kind.clone(),
            unseen: row.unseen,
            datetime: row.time.clone(),
            time: clock(&row.time).to_string(),
            diff: diff_href(row),
            origin,
            erased: Some((
                COLUMNS.len() - 2 + usize::from(actions.any()),
                m.get("ts-changes-erased"),
            )),
            flags: Vec::new(),
            target: None,
            size: None,
            actor: ActorModel::None,
            summary: SummaryModel {
                hidden: None,
                auto: None,
                moved: None,
                comment: Vec::new(),
            },
            tags: Vec::new(),
            pick: None,
            job: None,
            menu: None,
            menu_cell: actions.any(),
        };
    }
    let mut flags = Vec::new();
    for (on, msg, class) in [
        (row.flags.new, "ts-changes-flag-new", "new"),
        (row.flags.minor, "ts-changes-flag-minor", "minor"),
        (row.flags.bot, "ts-changes-flag-bot", "bot"),
    ] {
        if on {
            flags.push(Flag {
                class,
                title: m.get(&format!("{msg}-title")),
                text: m.get(msg),
            });
        }
    }
    let target = target_model(m, site, row, here.is_some()).or_else(|| {
        row.job.map(|j| TargetModel::Job {
            href: job_href(j),
            text: m.with("ts-changes-job", &[&j.to_string()]),
        })
    });
    RowModel {
        kind: row.kind.clone(),
        unseen: row.unseen,
        datetime: row.time.clone(),
        time: clock(&row.time).to_string(),
        diff: diff_href(row),
        origin,
        erased: None,
        flags,
        target,
        job: row
            .job
            .filter(|_| row.kind == "edit" && row.upstream.is_some())
            .map(|j| JobLink::to(m, j)),
        size: size_model(row),
        actor: actor_model(m, site, row),
        summary: summary_model(m, row, here),
        tags: row.tags.iter().filter(|t| *t != "bot").cloned().collect(),
        pick: None,
        menu: menu_model(m, row, actions),
        menu_cell: actions.any(),
    }
}

/// The rows, grouped by day.
pub fn list_model(
    m: &Messages,
    site: &SiteInfo,
    rows: &[ActivityRow],
    actions: RowActions,
) -> Option<ListModel> {
    list_model_here(m, site, rows, None, actions)
}

/// The rows as one table, a row group per day headed by its date, as the history of the
/// talk page `here` reads them: a thread listed on it or moved to or from it says so
/// (16 §6.1).
pub fn list_model_here(
    m: &Messages,
    site: &SiteInfo,
    rows: &[ActivityRow],
    here: Option<u64>,
    actions: RowActions,
) -> Option<ListModel> {
    if rows.is_empty() {
        return None;
    }
    let mut days: Vec<Day> = Vec::new();
    let mut day: Option<&str> = None;
    for row in rows {
        let d = row.time.split('T').next().unwrap_or("");
        if day != Some(d) {
            days.push(Day {
                date: date(m, &row.time),
                rows: Vec::new(),
            });
            day = Some(d);
        }
        if let Some(last) = days.last_mut() {
            last.rows.push(row_model(m, site, row, here, actions));
        }
    }
    Some(ListModel {
        columns: COLUMNS
            .iter()
            .map(|(msg, col)| (*col, m.get(msg)))
            .chain(
                actions
                    .any()
                    .then(|| ("menu", m.get("ts-changes-col-menu"))),
            )
            .collect(),
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0085 §4; 19 §2.8: a settings change reads as what it changed, each setting with its
    // value before and after.
    #[test]
    fn a_settings_change_names_each_setting_with_its_values() {
        let m = Messages::for_language("en");
        let row: ActivityRow = serde_json::from_value(serde_json::json!({
            "kind": "log", "time": "2026-11-09T10:00:00Z", "source": "config",
            "record": {"partition": 1, "offset": 2}, "logid": 9,
            "actor": {"key": "local:7", "name": "Alice", "kind": "registered"},
            "summary": {"op": "settings/change"},
            "log": {"type": "settings", "action": "change", "params": {"scope": "tenant", "changes": [
                {"scope": "tenant", "key": "site.name", "old": "librarybase", "new": "Library Base"},
                {"scope": "tenant", "key": "site.language", "old": null, "new": "de"},
                {"scope": "tenant", "key": "search.limit", "old": 20, "new": null}
            ]}},
        }))
        .unwrap();
        let s = summary_model(&m, &row, None);
        assert_eq!(
            s.auto.as_deref(),
            Some(
                "site.name: librarybase → Library Base; site.language: — → de; search.limit: 20 → —"
            )
        );
    }

    // 19 §3.4; 0010 A55: a row's menu links to the moderation pages with its actor and
    // target filled in, each for a holder of its right; the action has its own column,
    // and the summary is parsed as MediaWiki parses a comment.
    #[test]
    fn a_row_has_a_menu_an_action_and_a_parsed_summary() {
        let m = Messages::for_language("en");
        let row: ActivityRow = serde_json::from_value(serde_json::json!({
            "kind": "edit", "time": "2026-11-09T10:00:00Z", "source": "pages",
            "record": {"partition": 1, "offset": 2}, "revid": 9,
            "target": {"kind": "page", "id": "4", "ns": 4, "title": "Project:Home"},
            "actor": {"key": "local:8", "name": "Mallory", "kind": "registered"},
            "summary": {"op": "move"},
            "comment": "/* News */ see [[Help:Moving|help]]",
        }))
        .unwrap();
        let all = RowActions {
            block: true,
            protect: true,
            delete: true,
        };
        let r = row_model(&m, &SiteInfo::default(), &row, None, all);
        let items: Vec<(String, String)> = r
            .menu
            .expect("a menu")
            .items
            .into_iter()
            .map(|c| (c.label, c.href))
            .collect();
        assert_eq!(
            items,
            [
                (
                    "Block Mallory".to_string(),
                    "/wiki/Special:Block/Mallory".to_string()
                ),
                (
                    "Protect Project:Home".to_string(),
                    "/wiki/Project:Home?action=protect".to_string()
                ),
                (
                    "Delete Project:Home".to_string(),
                    "/wiki/Project:Home?action=delete".to_string()
                ),
            ]
        );
        assert!(r.menu_cell);
        assert_eq!(r.summary.auto.as_deref(), Some("move"));
        assert_eq!(
            r.summary.comment,
            vec![
                crate::comment::Part::Auto {
                    text: "\u{2192}News".into(),
                    href: Some("/wiki/Project:Home#News".into()),
                    more: true,
                },
                crate::comment::Part::Text {
                    text: "see ".into()
                },
                crate::comment::Part::Link {
                    href: "/wiki/Help:Moving".into(),
                    text: "help".into(),
                },
            ]
        );
        // Only what the viewer's rights open; nothing for a viewer with none, and the
        // list then has no menu column.
        let only_delete = RowActions {
            delete: true,
            ..RowActions::default()
        };
        let r = row_model(&m, &SiteInfo::default(), &row, None, only_delete);
        assert_eq!(r.menu.unwrap().items.len(), 1);
        let r = row_model(&m, &SiteInfo::default(), &row, None, RowActions::default());
        assert!(r.menu.is_none() && !r.menu_cell);
        let list = list_model(&m, &SiteInfo::default(), std::slice::from_ref(&row), all).unwrap();
        assert_eq!(list.columns.last().map(|c| c.0), Some("menu"));
        assert_eq!(list.columns[list.columns.len() - 3].0, "action");
        // A hidden actor is not offered for blocking.
        let mut hidden = row.clone();
        hidden.actor.hidden = true;
        let r = row_model(&m, &SiteInfo::default(), &hidden, None, all);
        assert_eq!(r.menu.unwrap().items.len(), 2);
    }

    #[test]
    fn times() {
        assert_eq!(clock("2025-09-25T00:26:40Z"), "00:26");
    }

    #[test]
    fn sizes() {
        let mut r = ActivityRow {
            delta: Some(20),
            size: Some(400),
            ..ActivityRow::default()
        };
        assert_eq!(size_model(&r).map(|s| s.text).as_deref(), Some("+20"));
        r.delta = Some(-5);
        assert_eq!(size_model(&r).map(|s| s.text).as_deref(), Some("\u{2212}5"));
        r.delta = None;
        assert_eq!(size_model(&r).map(|s| s.text).as_deref(), Some("(400)"));
    }

    #[test]
    fn a_threads_rows_read_from_the_talk_page_they_are_shown_on() {
        // 16 §6.1: listings and moves relative to the page whose history this is.
        let m = Messages::for_language("en");
        let row = |op: &str, params: serde_json::Value| ActivityRow {
            kind: "edit".into(),
            time: "2026-10-25T20:27:00Z".into(),
            source: "pages".into(),
            revid: Some(930),
            target: Some(triplespace_client::activity::ActivityTarget {
                kind: "page".into(),
                id: "29".into(),
                title: Some("Thread:2026-10-25/Why is P31 wrong here?".into()),
                ..Default::default()
            }),
            summary: triplespace_client::activity::Summary {
                op: Some(op.into()),
                ..Default::default()
            },
            log: Some(triplespace_client::activity::LogInfo {
                r#type: "thread".into(),
                action: op.into(),
                params: Some(params),
            }),
            ..ActivityRow::default()
        };
        let say = |r: &ActivityRow, here| thread_summary(&m, r, here).unwrap();
        let attach = row("attach", serde_json::json!({"talk": 7}));
        assert_eq!(say(&attach, Some(7)), "listed the thread here");
        assert_eq!(say(&attach, Some(8)), "listed the thread on another page");
        let mv = row("move", serde_json::json!({"talk": 9, "from_talk": 7}));
        assert_eq!(say(&mv, Some(7)), "moved the thread to another page");
        assert_eq!(say(&mv, Some(9)), "moved the thread here");
        assert_eq!(say(&row("post", serde_json::Value::Null), None), "replied");

        // A post links to itself, on the thread page; the subject names the thread.
        let post = row("post", serde_json::Value::Null);
        assert_eq!(
            diff_href(&post).as_deref(),
            Some("/wiki/Thread:2026-10-25/Why_is_P31_wrong_here%3F#post-930")
        );
        let shown =
            |here| match row_model(&m, &SiteInfo::default(), &post, here, RowActions::default())
                .target
            {
                Some(TargetModel::Titled { text, .. }) => text,
                other => panic!("{other:?}"),
            };
        assert_eq!(shown(Some(7)), "Why is P31 wrong here?");
        assert_eq!(shown(None), "Thread:2026-10-25/Why is P31 wrong here?");
    }

    /// A log row whose target is an account, as `rights/rights` writes them.
    fn rights_row(
        target_key: &str,
        label: &str,
        actor_key: &str,
        actor: Option<&str>,
    ) -> ActivityRow {
        serde_json::from_value(serde_json::json!({
            "kind": "log", "time": "2026-10-28T05:31:00Z", "source": "local",
            "record": {"partition": 1, "offset": 1},
            "target": {"kind": "actor", "id": target_key, "ns": 2, "title": format!("User:{label}"),
                       "label": {"value": label, "language": "en"}},
            "actor": {"key": actor_key, "name": actor},
            "summary": {"op": "rights/rights"},
        }))
        .unwrap()
    }

    #[test]
    fn accounts_are_marked_and_the_instance_is_the_server_operator() {
        let m = Messages::for_language("en");
        let site = SiteInfo {
            issuer: "wwq".into(),
            farm_code: "gpu".into(),
            ..SiteInfo::default()
        };
        let target = |m: &Messages, row: &ActivityRow| match row_model(
            m,
            &site,
            row,
            None,
            RowActions::default(),
        )
        .target
        {
            Some(TargetModel::Account { account }) => account,
            other => panic!("{other:?}"),
        };
        // An account of this wiki: the icon, its name to its contributions, no key.
        let a = target(
            &m,
            &rights_row("wwq:25", "Harej bot 2", "wwq:7", Some("Harej")),
        );
        let AccountModel::Named {
            icon, href, key, ..
        } = &a
        else {
            panic!("{a:?}");
        };
        assert_eq!(icon.as_deref(), Some("Account"));
        assert_eq!(
            href, "/wiki/Special:Contributions/Harej_bot_2",
            "not its user page"
        );
        assert_eq!(key, &None, "no key for this wiki's account");
        let actor = |m: &Messages, row: &ActivityRow| match row_model(
            m,
            &site,
            row,
            None,
            RowActions::default(),
        )
        .actor
        {
            ActorModel::Account { account } => account,
            other => panic!("{other:?}"),
        };
        // The instance acting: the badge, the key only as its tooltip.
        let a = actor(
            &m,
            &rights_row("wwq:1", "Librarybase", "instance:gpu", None),
        );
        let AccountModel::Operator { badge } = &a else {
            panic!("{a:?}");
        };
        assert_eq!(badge.text, "Server Operator");
        assert_eq!(badge.title.as_deref(), Some("instance:gpu"));
        // Another wiki's account keeps its key beside its name; a named one does not.
        let a = target(&m, &rights_row("other:3", "Ann", "wwq:7", Some("Harej")));
        let AccountModel::Named { key, .. } = &a else {
            panic!("{a:?}");
        };
        assert_eq!(key.as_deref(), Some("other:3"));
        let named = Messages::for_language("en").with_overrides(
            serde_json::from_value(serde_json::json!({"name/instance:far": {"": "Far Farm"}}))
                .unwrap(),
            "en",
        );
        let a = actor(&named, &rights_row("wwq:1", "X", "instance:far", None));
        let AccountModel::Named { shown, title, .. } = &a else {
            panic!("{a:?}");
        };
        assert_eq!(shown, "Far Farm");
        assert_eq!(title.as_deref(), Some("instance:far"));
    }

    #[test]
    fn entities_keep_their_ids() {
        let m = Messages::for_language("en");
        let row: ActivityRow = serde_json::from_value(serde_json::json!({
            "kind": "edit", "time": "2026-10-28T05:50:00Z", "source": "local",
            "record": {"partition": 1, "offset": 2},
            "target": {"kind": "entity", "id": "Q100001", "ns": 120, "title": "Item:Q100001",
                       "label": {"value": "The Supreme Court", "language": "en"}},
            "actor": {"key": "wwq:7", "name": "Harej"},
            "summary": {},
        }))
        .unwrap();
        let t = row_model(&m, &SiteInfo::default(), &row, None, RowActions::default()).target;
        let Some(TargetModel::Labelled { id, label, .. }) = t else {
            panic!("{t:?}");
        };
        assert_eq!(id, "Q100001");
        assert_eq!(label, "The Supreme Court");
    }
}
