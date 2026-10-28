//! The diff page and old revisions of an entity (0010 §5–6; 19 §3.7–3.8): MediaWiki's
//! addresses on the entity's title — `?diff={revid}` for one change, `?oldid={a}&diff={b}`
//! to compare two local revisions, `?oldid={revid}` to read the entity as it then was —
//! and `?diff={partition}:{offset}` for a record that has no local revision ID, a mirror
//! sync.
//!
//! **A diff is drawn as statements.** Terms and sitelinks are keyed lists with the value
//! before and after; each property the change touched is a group whose statements are
//! marked added, removed or changed, with the unchanged ones kept for context; a changed
//! statement shows both sides. Values are drawn by the entity page's own renderer, so a
//! linked entity shows its label. **Alternative views:** `view=changeset` shows the
//! record's operation as stored. **"About this edit"** lists the graph, the actor, the
//! time, the comment, the size change, the revision and the record's coordinates.
//!
//! **A sync** has no statement diff until the provider's deltas are kept (18 §5.1): the
//! page shows the upstream versions on either side and the stored summary of what
//! changed, per property, language and site.
//!
//! **An old revision** (`oldid=`) is the entity page's statements tab drawn from the
//! local graph's state after that revision, with a banner saying so and linking to the
//! current page and the history. Only local revisions have old states (19 §3.5).
//!
//! What waits for later work: the JSON view, "What this changes" (the effect block,
//! 18 §5.1), the checkpoint and inclusion proof, Undo, and compare controls on the
//! History tab.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Rank, Statement};
use serde_json::Value;
use triplespace_client::revisions::{EntityDiff, RevisionFacts};
use triplespace_client::{Change, ClientError, Comparison, Provenance, SiteInfo};

use super::view::chip;
use super::{Lookup, Render, page_title, referenced, site_roles};
use crate::Site;
use crate::codex::{ChipKind, MessageKind};
use crate::frame::{Page, Tab, title_url};
use crate::html::{esc, lang_attrs, link};
use crate::i18n::Messages;
use crate::pages::{self, Context};

/// What the query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `diff={revid}`: one local change.
    Change(u64),
    /// `diff={partition}:{offset}`: one record.
    Record(u64, u64),
    /// `oldid={a}&diff={b}`: two local revisions.
    Compare(u64, u64),
    /// `oldid={revid}`: the entity as it then was.
    Old(u64),
}

impl Ask {
    /// From the query; `None` when it asks for none of these.
    #[must_use]
    pub fn of(query: &BTreeMap<String, String>) -> Option<Self> {
        let oldid = query
            .get("oldid")
            .and_then(|v| v.trim().parse::<u64>().ok());
        let diff = query.get("diff").map(|d| d.trim());
        match (oldid, diff) {
            (Some(a), Some(b)) if b.parse::<u64>().is_ok() => {
                Some(Self::Compare(a, b.parse().ok()?))
            }
            (_, Some(d)) if d.parse::<u64>().is_ok() => Some(Self::Change(d.parse().ok()?)),
            (_, Some(d)) => {
                let (p, o) = d.split_once(':')?;
                Some(Self::Record(p.parse().ok()?, o.parse().ok()?))
            }
            (Some(a), None) => Some(Self::Old(a)),
            (None, None) => None,
        }
    }
}

/// One side of a keyed list.
fn cell(m: &Messages, value: Option<&str>, lang: Option<&str>, class: &str) -> String {
    match value {
        Some(v) => format!(
            "<td class=\"ts-diff__cell ts-diff__cell--{class}\"><span{}>{}</span></td>",
            lang.map_or_else(String::new, |l| lang_attrs(l, m.lang())),
            esc(v)
        ),
        None => "<td class=\"ts-diff__cell ts-diff__cell--empty\"></td>".to_string(),
    }
}

/// A keyed list's table: terms or sitelinks, before and after.
fn keyed_table(
    m: &Messages,
    heading: &str,
    rows: &[(String, Option<String>, Option<String>, bool)],
) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "<section class=\"ts-diff__section\"><h3 class=\"ts-diff__heading\">{}</h3><table class=\"ts-diff__table\"><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
        esc(heading),
        esc(&m.get("ts-diff-key")),
        esc(&m.get("ts-diff-before")),
        esc(&m.get("ts-diff-after"))
    );
    for (key, before, after, is_lang) in rows {
        let lang = is_lang.then_some(key.as_str());
        let before_class = if before.is_some() && after.is_none() {
            "removed"
        } else if before != after {
            "changed"
        } else {
            "same"
        };
        let after_class = if after.is_some() && before.is_none() {
            "added"
        } else if before != after {
            "changed"
        } else {
            "same"
        };
        let _ = write!(
            s,
            "<tr><th scope=\"row\" class=\"ts-diff__key\">{}</th>{}{}</tr>",
            esc(key),
            cell(m, before.as_deref(), lang, before_class),
            cell(m, after.as_deref(), lang, after_class)
        );
    }
    s.push_str("</tbody></table></section>");
    s
}

fn sitelink_text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let title = v.get("title").and_then(Value::as_str)?;
    let badges: Vec<&str> = v
        .get("badges")
        .and_then(Value::as_array)
        .map(|b| b.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Some(if badges.is_empty() {
        title.to_string()
    } else {
        format!("{title} ({})", badges.join(", "))
    })
}

/// A statement as the diff page draws it: the value, qualifiers and references.
fn statement_html(r: &Render<'_>, st: &Statement) -> String {
    let rank = match st.rank {
        Rank::Normal => String::new(),
        Rank::Preferred => format!(" {}", chip(ChipKind::Rank, &r.m.get("ts-rank-preferred"))),
        Rank::Deprecated => format!(" {}", chip(ChipKind::Rank, &r.m.get("ts-rank-deprecated"))),
    };
    format!(
        "<div class=\"ts-diff__statement\"><div class=\"ts-statement__main\">{}{rank}</div>{}{}</div>",
        r.snak(&st.mainsnak),
        if st.qualifiers.is_empty() {
            String::new()
        } else {
            r.snak_groups(&st.qualifiers, "ts-qualifiers")
        },
        r.references(&st.references)
    )
}

fn parse_statement(v: Option<&Value>) -> Option<Statement> {
    serde_json::from_value(v?.clone()).ok()
}

/// The statement groups of a diff.
fn claims_html(r: &Render<'_>, diff: &EntityDiff) -> String {
    let m = r.m;
    let mut s = String::new();
    for g in &diff.claims {
        let _ = write!(
            s,
            "<section class=\"ts-diff__section\"><h3 class=\"ts-diff__heading\">{}</h3><table class=\"ts-diff__table ts-diff__table--statements\"><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
            r.property_link(&g.property),
            esc(&m.get("ts-diff-change")),
            esc(&m.get("ts-diff-before")),
            esc(&m.get("ts-diff-after"))
        );
        for st in &g.statements {
            let (label, before, after) = match st.change.as_str() {
                "added" => ("ts-diff-added", None, parse_statement(st.after.as_ref())),
                "removed" => ("ts-diff-removed", parse_statement(st.before.as_ref()), None),
                "changed" => (
                    "ts-diff-changed",
                    parse_statement(st.before.as_ref()),
                    parse_statement(st.after.as_ref()),
                ),
                _ => {
                    let same = parse_statement(st.statement.as_ref());
                    ("ts-diff-unchanged", same.clone(), same)
                }
            };
            let class = label.trim_start_matches("ts-diff-");
            let side = |st: Option<&Statement>, which: &str| match st {
                Some(st) => format!(
                    "<td class=\"ts-diff__cell ts-diff__cell--{which}\">{}</td>",
                    statement_html(r, st)
                ),
                None => "<td class=\"ts-diff__cell ts-diff__cell--empty\"></td>".to_string(),
            };
            let (bc, ac) = match class {
                "added" => ("empty", "added"),
                "removed" => ("removed", "empty"),
                "changed" => ("changed", "changed"),
                _ => ("same", "same"),
            };
            let _ = write!(
                s,
                "<tr class=\"ts-diff__row ts-diff__row--{class}\"><th scope=\"row\" class=\"ts-diff__mark\">{}</th>{}{}</tr>",
                esc(&m.get(label)),
                side(before.as_ref(), bc),
                side(after.as_ref(), ac)
            );
        }
        s.push_str("</tbody></table></section>");
    }
    s
}

/// The whole diff: terms, aliases, sitelinks, then the statement groups.
fn diff_html(r: &Render<'_>, diff: &EntityDiff) -> String {
    let m = r.m;
    if diff.is_empty() {
        return format!(
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-diff-nothing"))
        );
    }
    let mut s = String::new();
    let terms = |changes: &[triplespace_client::revisions::TermChange]| {
        changes
            .iter()
            .map(|t| (t.language.clone(), t.before.clone(), t.after.clone(), true))
            .collect::<Vec<_>>()
    };
    s.push_str(&keyed_table(
        m,
        &m.get("ts-diff-labels"),
        &terms(&diff.labels),
    ));
    s.push_str(&keyed_table(
        m,
        &m.get("ts-diff-descriptions"),
        &terms(&diff.descriptions),
    ));
    let aliases: Vec<_> = diff
        .aliases
        .iter()
        .map(|a| {
            let mut before: Vec<&str> = a.kept.iter().map(String::as_str).collect();
            before.extend(a.removed.iter().map(String::as_str));
            let mut after: Vec<&str> = a.kept.iter().map(String::as_str).collect();
            after.extend(a.added.iter().map(String::as_str));
            (
                a.language.clone(),
                (!before.is_empty()).then(|| before.join(" | ")),
                (!after.is_empty()).then(|| after.join(" | ")),
                true,
            )
        })
        .collect();
    s.push_str(&keyed_table(m, &m.get("ts-diff-aliases"), &aliases));
    let sitelinks: Vec<_> = diff
        .sitelinks
        .iter()
        .map(|l| {
            (
                l.site.clone(),
                sitelink_text(l.before.as_ref()),
                sitelink_text(l.after.as_ref()),
                false,
            )
        })
        .collect();
    s.push_str(&keyed_table(m, &m.get("ts-diff-sitelinks"), &sitelinks));
    s.push_str(&claims_html(r, diff));
    s
}

/// A revision's line in the header: time, actor, comment.
fn revision_line(m: &Messages, facts: &RevisionFacts, title: &str) -> String {
    let mut s = String::new();
    if let Some(r) = facts.revid {
        let _ = write!(
            s,
            "{} ",
            link(
                &format!("/wiki/{}?oldid={r}", title_url(title)),
                &esc(&m.with("ts-diff-revision", &[&r.to_string()])),
                ""
            )
        );
    }
    let _ = write!(
        s,
        "<time datetime=\"{}\">{}</time>",
        esc(&facts.time),
        esc(facts.time.replace('T', " ").trim_end_matches('Z'))
    );
    if let Some(a) = &facts.actor {
        let _ = write!(
            s,
            " · {}",
            link(
                &format!("/wiki/Special:Contributions/{}", title_url(a)),
                &esc(a),
                ""
            )
        );
    }
    if let Some(c) = &facts.comment
        && !c.is_empty()
    {
        let _ = write!(s, " · <span class=\"ts-change__comment\">{}</span>", esc(c));
    }
    s
}

/// "About this edit".
fn about_html(m: &Messages, site: &SiteInfo, change: &Change) -> String {
    let mut rows = Vec::new();
    let graph = match change.source.strip_prefix("mirror/") {
        Some(slug) => site
            .providers
            .iter()
            .find(|p| p.slug == slug)
            .map_or_else(|| change.source.clone(), |p| p.name.clone()),
        None => m.get("ts-history-made-here"),
    };
    rows.push((m.get("ts-diff-about-graph"), esc(&graph)));
    if let Some(r) = change.revision.revid {
        rows.push((m.get("ts-diff-about-revision"), r.to_string()));
    }
    if let Some(op) = &change.revision.op {
        rows.push((m.get("ts-diff-about-operation"), esc(op)));
    }
    if let (Some(size), Some(delta)) = (change.size, change.delta) {
        let sign = if delta >= 0 { "+" } else { "\u{2212}" };
        rows.push((
            m.get("ts-diff-about-size"),
            format!("<span dir=\"ltr\">{size} ({sign}{})</span>", delta.abs()),
        ));
    } else if let Some(size) = change.size {
        rows.push((
            m.get("ts-diff-about-size"),
            format!("<span dir=\"ltr\">{size}</span>"),
        ));
    }
    rows.push((
        m.get("ts-diff-about-record"),
        format!(
            "<span class=\"ts-mono\" dir=\"ltr\">{}:{}</span>",
            change.revision.partition, change.revision.offset
        ),
    ));
    if !change.revision.tags.is_empty() {
        rows.push((
            m.get("ts-diff-about-tags"),
            esc(&change.revision.tags.join(", ")),
        ));
    }
    let mut s = format!(
        "<aside class=\"ts-diff__about\"><h3>{}</h3><dl>",
        esc(&m.get("ts-diff-about"))
    );
    for (k, v) in rows {
        let _ = write!(s, "<div><dt>{}</dt><dd>{v}</dd></div>", esc(&k));
    }
    s.push_str("</dl></aside>");
    s
}

/// A sync's summary of what changed, per property, language and site.
fn changes_summary_html(m: &Messages, r: &Render<'_>, changes: &Value) -> String {
    let mut s = format!(
        "<section class=\"ts-diff__section\"><h3 class=\"ts-diff__heading\">{}</h3><table class=\"ts-diff__table\"><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
        esc(&m.get("ts-diff-sync-summary")),
        esc(&m.get("ts-diff-key")),
        esc(&m.get("ts-diff-added")),
        esc(&m.get("ts-diff-removed")),
        esc(&m.get("ts-diff-changed"))
    );
    let mut any = false;
    for (section, is_property) in [
        ("labels", false),
        ("descriptions", false),
        ("aliases", false),
        ("sitelinks", false),
        ("claims", true),
    ] {
        let Some(map) = changes.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (key, counts) in map {
            any = true;
            let n = |k: &str| counts.get(k).and_then(Value::as_u64).unwrap_or(0);
            let key_html = if is_property {
                r.property_link(key)
            } else {
                format!(
                    "{}: {}",
                    esc(&m.get(&format!("ts-diff-{section}"))),
                    esc(key)
                )
            };
            let _ = write!(
                s,
                "<tr><th scope=\"row\" class=\"ts-diff__key\">{key_html}</th><td dir=\"ltr\">{}</td><td dir=\"ltr\">{}</td><td dir=\"ltr\">{}</td></tr>",
                n("added"),
                n("removed"),
                n("changed")
            );
        }
    }
    s.push_str("</tbody></table></section>");
    if any {
        s
    } else {
        format!(
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-diff-sync-no-summary"))
        )
    }
}

/// The IDs a diff refers to, for the label lookup: both sides' statements.
fn referenced_in(diff: &EntityDiff, entity: &Entity, site: &SiteInfo) -> Vec<String> {
    let mut probe = Entity::new(entity.id.clone(), entity.entity_type.clone());
    for g in &diff.claims {
        let Ok(property) = EntityId::parse(&g.property) else {
            continue;
        };
        let mut sts = Vec::new();
        for st in &g.statements {
            for v in [&st.before, &st.after, &st.statement] {
                if let Some(parsed) = parse_statement(v.as_ref()) {
                    sts.push(parsed);
                }
            }
        }
        probe.statements.insert(property, sts);
    }
    referenced(&probe, site)
}

/// The view switch: statements (the default) or the change set.
fn view_switch(
    m: &Messages,
    title: &str,
    query: &BTreeMap<String, String>,
    current: &str,
) -> String {
    let mut base = format!("/wiki/{}?", title_url(title));
    for key in ["oldid", "diff"] {
        if let Some(v) = query.get(key) {
            let _ = write!(base, "{key}={}&", esc(v));
        }
    }
    let mut s = format!(
        "<nav class=\"ts-search-filter ts-diff__views\" aria-label=\"{}\"><ul>",
        esc(&m.get("ts-diff-views-label"))
    );
    for (view, msg) in [
        ("statements", "ts-diff-view-statements"),
        ("changeset", "ts-diff-view-changeset"),
    ] {
        let _ = write!(
            s,
            "<li>{}</li>",
            link(
                &format!("{base}view={view}"),
                &esc(&m.get(msg)),
                if view == current {
                    " aria-current=\"page\""
                } else {
                    ""
                }
            )
        );
    }
    s.push_str("</ul></nav>");
    s
}

/// The page's tabs: the entity's, with History current.
fn tabs(m: &Messages, title: &str, items: bool) -> Vec<Tab> {
    super::tabs(m, title, super::TabName::History, items, true)
}

/// The label lookup for a diff's values.
async fn lookup_for(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    entity: &Entity,
    diff: Option<&EntityDiff>,
) -> Result<Lookup, ClientError> {
    let chain = cx.m.term_chain();
    let mut ids = referenced(entity, &cx.site);
    if let Some(d) = diff {
        ids.extend(referenced_in(d, entity, &cx.site));
    }
    ids.sort();
    ids.dedup();
    let mut lookup = if ids.is_empty() {
        Lookup::default()
    } else {
        let f = site
            .client()
            .entities(incoming, &ids, "labels|datatype", &chain.join("|"))
            .await?;
        cx.inputs.push(f.cache);
        Lookup::from_entities(&f.value.found, &chain)
    };
    lookup.add_self(entity, &chain);
    Ok(lookup)
}

/// What a diff address resolved to.
type Found = Option<(Option<Change>, Option<Comparison>)>;

/// Serves a diff, a comparison or an old revision of the entity.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn serve(
    site: &Site,
    mut cx: Context,
    incoming: &triplespace_client::Incoming,
    headers: &HeaderMap,
    entity: &Entity,
    provenance: Option<&Provenance>,
    ask: &Ask,
    query: &BTreeMap<String, String>,
) -> Response {
    let client = site.client();
    let title = page_title(entity.id.as_str(), entity.entity_type.name());
    let items = entity.entity_type.name() == "item";
    let roles = site_roles(&cx.site);
    let view = query.get("view").map_or("statements", String::as_str);

    // An old revision: the statements tab as it then was.
    if let Ask::Old(revid) = ask {
        let rev = match client.revision(incoming, *revid).await {
            Ok(f) => {
                cx.inputs.push(f.cache);
                f.value
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        };
        let old = rev
            .as_ref()
            .filter(|r| r.title == title)
            .and_then(|r| r.content.as_deref())
            .and_then(|c| serde_json::from_str::<Value>(c).ok())
            .and_then(|v| Entity::from_value(v).ok())
            .map(|p| p.entity);
        let Some(old) = old else {
            return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
                title: title.clone(),
                returnto: Some(title.clone()),
                body: pages::message(
                    MessageKind::Warning,
                    &m.with("ts-oldid-missing", &[&revid.to_string()]),
                ),
                ..Page::default()
            });
        };
        let lookup = match lookup_for(site, &mut cx, incoming, &old, None).await {
            Ok(l) => l,
            Err(e) => return pages::api_failed(&cx, headers, &e),
        };
        let r = Render {
            m: &cx.m,
            site: &cx.site,
            lookup: &lookup,
            entity: &old,
            provenance,
            roles: &roles,
            editable: false,
            notes: super::Endnotes::default(),
        };
        let facts = rev.as_ref().expect("checked above");
        let banner = pages::message(
            MessageKind::Notice,
            &cx.m.with(
                "ts-oldid-banner",
                &[
                    &revid.to_string(),
                    facts.user.as_deref().unwrap_or(""),
                    facts.timestamp.replace('T', " ").trim_end_matches('Z'),
                ],
            ),
        );
        let links = format!(
            "<p class=\"ts-oldid__links\">{} · {}</p>",
            link(
                &format!("/wiki/{}", title_url(&title)),
                &esc(&cx.m.get("ts-oldid-current")),
                ""
            ),
            link(
                &format!("/wiki/{}?action=history", title_url(&title)),
                &esc(&cx.m.get("ts-tab-history")),
                ""
            )
        );
        let body = format!("{banner}{links}{}", r.statements_tab());
        let (display, identity, terms, doc_title) = (
            r.display_title(),
            r.identity(),
            r.terms(),
            r.document_title(),
        );
        let tabs = super::tabs(&cx.m, &title, super::TabName::Statements, items, true);
        return pages::respond(&cx, headers, StatusCode::OK, move |_| Page {
            title: display,
            doc_title: Some(doc_title),
            returnto: Some(title),
            identity_html: Some(identity),
            subtitle_html: Some(terms),
            tabs,
            body,
            ..Page::default()
        });
    }

    // A change or a comparison.
    let fetched: Result<Found, ClientError> =
        match ask {
            Ask::Change(revid) => match client.revision(incoming, *revid).await {
                Ok(f) => {
                    cx.inputs.push(f.cache);
                    match f.value.and_then(|r| r.record) {
                        Some((p, o)) => client
                            .change(incoming, entity.id.as_str(), p, o)
                            .await
                            .map(|f| {
                                cx.inputs.push(f.cache);
                                f.value.map(|c| (Some(c), None))
                            }),
                        None => Ok(None),
                    }
                }
                Err(e) => Err(e),
            },
            Ask::Record(p, o) => client
                .change(incoming, entity.id.as_str(), *p, *o)
                .await
                .map(|f| {
                    cx.inputs.push(f.cache);
                    f.value.map(|c| (Some(c), None))
                }),
            Ask::Compare(a, b) => client
                .compare(incoming, entity.id.as_str(), *a, *b)
                .await
                .map(|f| {
                    cx.inputs.push(f.cache);
                    f.value.map(|c| (None, Some(c)))
                }),
            Ask::Old(_) => unreachable!("served above"),
        };
    let found = match fetched {
        Ok(f) => f,
        Err(ClientError::Api { code, info })
            if code == "ts-compare-source" || code == "bad-parameter" =>
        {
            return pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |_| Page {
                title: title.clone(),
                returnto: Some(title.clone()),
                body: pages::message(MessageKind::Error, &info),
                ..Page::default()
            });
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let Some((change, comparison)) = found else {
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: title.clone(),
            returnto: Some(title.clone()),
            body: pages::message(MessageKind::Warning, &m.get("ts-diff-missing")),
            ..Page::default()
        });
    };
    let diff: Option<&EntityDiff> = change
        .as_ref()
        .and_then(|c| c.diff.as_ref())
        .or_else(|| comparison.as_ref().map(|c| &c.diff));
    let lookup = match lookup_for(site, &mut cx, incoming, entity, diff).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let r = Render {
        m: &cx.m,
        site: &cx.site,
        lookup: &lookup,
        entity,
        provenance,
        roles: &roles,
        editable: false,
        notes: super::Endnotes::default(),
    };
    let m = &cx.m;
    let mut body = String::from("<div class=\"ts-diff\">");
    // The header: which revisions.
    match (&change, &comparison) {
        (Some(c), _) => {
            let prev = c.previous.as_ref().and_then(|p| p["revid"].as_u64());
            let _ = write!(
                body,
                "<p class=\"ts-diff__head\">{}</p>",
                revision_line(m, &c.revision, &title)
            );
            if let (Some(p), Some(r)) = (prev, c.revision.revid) {
                let _ = write!(
                    body,
                    "<p class=\"ts-diff__nav\">{} · {}</p>",
                    link(
                        &format!("/wiki/{}?diff={p}", title_url(&title)),
                        &esc(&m.get("ts-diff-previous")),
                        " rel=\"prev\""
                    ),
                    link(
                        &format!("/wiki/{}?oldid={p}&diff={r}", title_url(&title)),
                        &esc(&m.get("ts-diff-compare-previous")),
                        ""
                    )
                );
            }
        }
        (None, Some(c)) => {
            let _ = write!(
                body,
                "<table class=\"ts-diff__revisions\"><tr><th>{}</th><td>{}</td></tr><tr><th>{}</th><td>{}</td></tr></table>",
                esc(&m.get("ts-diff-before")),
                revision_line(m, &c.from, &title),
                esc(&m.get("ts-diff-after")),
                revision_line(m, &c.to, &title)
            );
        }
        (None, None) => {}
    }
    // The content.
    let main = match (&change, &comparison, view) {
        (Some(c), _, "changeset") => {
            body.push_str(&view_switch(m, &title, query, view));
            format!(
                "<pre class=\"ts-diff__changeset\" dir=\"ltr\">{}</pre>",
                esc(
                    &serde_json::to_string_pretty(c.operation.as_ref().unwrap_or(&Value::Null))
                        .unwrap_or_default()
                )
            )
        }
        (Some(c), _, _) if c.kind == "sync" => {
            let upstream = |v: Option<&Value>| {
                v.and_then(|u| {
                    u.get("revid")
                        .and_then(Value::as_u64)
                        .map(|n| n.to_string())
                        .or_else(|| u.get("version").and_then(Value::as_str).map(str::to_string))
                })
                .unwrap_or_else(|| "–".into())
            };
            let mut s = format!(
                "<p class=\"ts-diff__sync\">{}</p>",
                esc(&m.with(
                    "ts-diff-sync-line",
                    &[
                        &upstream(c.revision.prev_upstream.as_ref()),
                        &upstream(c.revision.upstream.as_ref()),
                    ]
                ))
            );
            match &c.changes {
                Some(ch) if !ch.is_null() => s.push_str(&changes_summary_html(m, &r, ch)),
                _ => {
                    let _ = write!(
                        s,
                        "<p class=\"ts-empty\">{}</p>",
                        esc(&m.get("ts-diff-sync-no-summary"))
                    );
                }
            }
            s
        }
        (Some(_), _, _) | (None, Some(_), _) => {
            if change.is_some() {
                body.push_str(&view_switch(m, &title, query, "statements"));
            }
            diff.map_or_else(String::new, |d| diff_html(&r, d))
        }
        (None, None, _) => String::new(),
    };
    // One grid cell for the sections, one for the panel: the sections are several
    // elements, and placed in the grid directly they would take a column each.
    body.push_str("<div class=\"ts-diff__body\"><div class=\"ts-diff__main\">");
    body.push_str(&main);
    body.push_str("</div>");
    if let Some(c) = &change {
        body.push_str(&about_html(m, &cx.site, c));
    }
    body.push_str("</div></div>");
    let (display, identity, terms, doc_title) = (
        r.display_title(),
        r.identity(),
        r.terms(),
        r.document_title(),
    );
    let tabs = tabs(m, &title, items);
    pages::respond(&cx, headers, StatusCode::OK, move |_| Page {
        title: display,
        doc_title: Some(doc_title),
        returnto: Some(title),
        identity_html: Some(identity),
        subtitle_html: Some(terms),
        tabs,
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_follow_mediawikis_addresses() {
        let q = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        };
        assert_eq!(Ask::of(&q(&[])), None);
        assert_eq!(Ask::of(&q(&[("diff", "907")])), Some(Ask::Change(907)));
        assert_eq!(
            Ask::of(&q(&[("diff", "8194:41")])),
            Some(Ask::Record(8194, 41))
        );
        assert_eq!(
            Ask::of(&q(&[("oldid", "900"), ("diff", "907")])),
            Some(Ask::Compare(900, 907))
        );
        assert_eq!(Ask::of(&q(&[("oldid", "900")])), Some(Ask::Old(900)));
        assert_eq!(
            Ask::of(&q(&[("diff", "prev")])),
            None,
            "MediaWiki's keywords are not served yet"
        );
    }

    #[test]
    fn sitelinks_show_their_badges() {
        assert_eq!(
            sitelink_text(Some(
                &serde_json::json!({"site": "enwiki", "title": "Six", "badges": ["Q17437796"]})
            ))
            .as_deref(),
            Some("Six (Q17437796)")
        );
        assert_eq!(sitelink_text(None), None);
    }
}
