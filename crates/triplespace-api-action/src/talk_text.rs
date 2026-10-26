//! A talk page's content as of a revision (16 §6.1; 10 §4.3; 0019 A29): the threads attached
//! to it now, each as it stood after that revision, written as a MediaWiki talk page is.
//!
//! - Each thread is a `== Subject ==` section, in the order the talk page shows them:
//!   pinned first, in the order they were pinned, then in the order they were attached.
//!   A thread that did not exist yet at the revision is left out.
//! - Each post's lines are indented with one `:` per level of depth, and the post ends with
//!   its author's signature and time, as `~~~~` would have written it:
//!   `[[User:Alice|Alice]] ([[User talk:Alice|talk]]) 16:22, 25 October 2026 (UTC)`.
//! - A post keeps the markup it was written in; markdown is not converted. An imported post
//!   already carries its signature, and a heading post is a heading at its level.
//! - What the viewer may not see is written as MediaWiki writes it: `(comment removed)` for
//!   hidden or erased text, `(username removed)` for a hidden author or a hidden name.
//!
//! The text is read-only: it is a serialization for `prop=revisions` and `action=compare`,
//! never a page source. The content model stays `triplespace-talk`; the format is
//! `text/x-wiki`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::BuildHasher;

use scatter_actors::Timestamp;
use scatter_actors::evaluate::Effective;
use scatter_actors::{ReadDecision, read_decision};
use serde_json::Value;
use tokio_postgres::Client;

use crate::pages::Site;
use crate::response::ApiError;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `16:22, 25 October 2026 (UTC)` from `2026-10-25T16:22:00Z`.
fn sig_time(iso: &str) -> Option<String> {
    let (date, time) = iso.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day) = (d.next()?, d.next()?, d.next()?);
    let month = MONTHS.get(m.parse::<usize>().ok()?.checked_sub(1)?)?;
    let day: u32 = day.parse().ok()?;
    Some(format!("{}, {day} {month} {y} (UTC)", time.get(..5)?))
}

/// The names a signature uses: the User and User talk namespaces' local names, and the
/// word of the talk link.
pub struct Names<'a> {
    /// The User namespace.
    pub user: &'a str,
    /// The User talk namespace.
    pub user_talk: &'a str,
}

/// A post's signature.
fn signature(names: &Names<'_>, author: Option<&str>, published: Option<&str>) -> String {
    let who = author.map_or_else(
        || "(username removed)".to_string(),
        |a| {
            format!(
                "[[{}:{a}|{a}]] ([[{}:{a}|talk]])",
                names.user, names.user_talk
            )
        },
    );
    match published.and_then(sig_time) {
        Some(t) => format!("{who} {t}"),
        None => who,
    }
}

/// One thread, as a section.
fn thread_text<S: BuildHasher>(
    names: &Names<'_>,
    thread: &Value,
    authors: &HashMap<String, Option<String>, S>,
) -> String {
    let mut out = format!("== {} ==\n", thread["subject"].as_str().unwrap_or_default());
    for p in thread["posts"].as_array().into_iter().flatten() {
        let depth = usize::try_from(p["depth"].as_u64().unwrap_or(0)).unwrap_or(0);
        let indent = ":".repeat(depth);
        if let Some(level) = p["heading"].as_u64() {
            let marks = "=".repeat(usize::try_from(level).unwrap_or(3));
            let text = p["content"].as_str().unwrap_or_default().trim();
            let _ = writeln!(out, "{marks} {text} {marks}");
            continue;
        }
        let text = match p["content"].as_str() {
            Some(t) => t.trim_end().to_string(),
            None => "(comment removed)".to_string(),
        };
        let mut lines: Vec<String> = text.lines().map(|l| format!("{indent}{l}")).collect();
        if lines.is_empty() {
            lines.push(indent.clone());
        }
        let imported = p["imported"].as_bool().unwrap_or(false);
        if !imported {
            let author = p["attributedTo"]
                .as_str()
                .and_then(|k| match authors.get(k) {
                    Some(name) => name.as_deref(),
                    None => Some(k),
                });
            let sig = signature(names, author, p["published"].as_str());
            if let Some(last) = lines.last_mut() {
                if last.trim_start_matches(':').is_empty() {
                    last.push_str(&sig);
                } else {
                    last.push(' ');
                    last.push_str(&sig);
                }
            }
        }
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    out
}

/// The talk page's text from its threads' bodies, in order.
#[must_use]
pub fn render<S: BuildHasher>(
    names: &Names<'_>,
    threads: &[Value],
    authors: &HashMap<String, Option<String>, S>,
) -> String {
    threads
        .iter()
        .map(|t| thread_text(names, t, authors))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The threads attached to a talk page now, readable by the viewer, in the talk page's
/// order: pinned first, then by attachment.
async fn attached(
    db: &Client,
    tenant: &str,
    who: &Effective,
    talk: u64,
    at: Timestamp,
) -> Result<Vec<u64>, ApiError> {
    let rows = db
        .query(
            "SELECT a.thread_id FROM view.thread_attachment a
             JOIN view.page p ON p.tenant = a.tenant AND p.page_id = a.thread_id AND NOT p.deleted
             WHERE a.tenant = $1 AND a.talk_page_id = $2
             ORDER BY a.pinned IS NULL, a.pinned, a.attached, a.thread_id",
            &[&tenant, &i64::try_from(talk).unwrap_or(i64::MAX)],
        )
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let id = u64::try_from(r.get::<_, i64>(0)).unwrap_or(0);
        let chain = triplespace_projections::acl_read::thread_chain(db, tenant, id)
            .await
            .map_err(ApiError::internal)?
            .unwrap_or_default();
        if read_decision(who, &chain, at) == ReadDecision::Readable {
            out.push(id);
        }
    }
    Ok(out)
}

/// A talk page's content after revision `upto` (the latest when `None`), as this viewer may
/// see it.
///
/// # Errors
///
/// A database failure.
pub async fn text_at(
    db: &Client,
    tenant: &str,
    who: &Effective,
    talk: u64,
    upto: Option<u64>,
) -> Result<String, ApiError> {
    let at = Timestamp(crate::edit::now_micros());
    let mut bodies = Vec::new();
    for thread in attached(db, tenant, who, talk, at).await? {
        // The thread's latest revision at or before `upto`; none means it did not exist yet.
        let revid: Option<i64> = db
            .query_one(
                "SELECT max(revid) FROM view.activity
                 WHERE tenant = $1 AND target_kind = 2 AND target_id = $2 AND revid IS NOT NULL
                   AND ($3::bigint IS NULL OR revid <= $3)",
                &[
                    &tenant,
                    &thread.to_string(),
                    &upto.and_then(|u| i64::try_from(u).ok()),
                ],
            )
            .await?
            .get(0);
        let Some(revid) = revid.and_then(|r| u64::try_from(r).ok()) else {
            continue;
        };
        if let Some(body) = crate::thread_view::body(db, tenant, who, thread, Some(revid), at)
            .await
            .map_err(ApiError::internal)?
        {
            bodies.push(body);
        }
    }
    let keys: Vec<String> = bodies
        .iter()
        .flat_map(|b| b["posts"].as_array().cloned().unwrap_or_default())
        .filter_map(|p| p["attributedTo"].as_str().map(str::to_string))
        .collect();
    let authors = crate::thread_html::names(db, tenant, who, &keys).await?;
    let site = Site::load_for(db, tenant).await?;
    let ns = |n: i32| site.prefixed(n, "x").trim_end_matches(":x").to_string();
    let (user, user_talk) = (ns(2), ns(3));
    Ok(render(
        &Names {
            user: &user,
            user_talk: &user_talk,
        },
        &bodies,
        &authors,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_talk_page_reads_as_wikitext() {
        let names = Names {
            user: "User",
            user_talk: "User talk",
        };
        let authors: HashMap<String, Option<String>> = [
            ("lb:7".to_string(), Some("Alice".to_string())),
            ("lb:8".to_string(), Some("Bob".to_string())),
        ]
        .into_iter()
        .collect();
        let t = json!({
            "subject": "Coverage",
            "posts": [
                {"depth": 0, "attributedTo": "lb:7", "published": "2026-10-25T16:22:00Z",
                 "content": "Do we cover **maps**?\nAnd atlases?"},
                {"depth": 1, "attributedTo": "lb:8", "published": "2026-10-25T16:30:00Z",
                 "content": "Yes."},
                {"depth": 2, "attributedTo": null, "published": "2026-10-25T16:31:00Z",
                 "content": null, "texthidden": true},
                {"depth": 1, "attributedTo": "lb:9", "published": "2026-10-25T16:40:00Z",
                 "content": "Old comment [[User:Ed|Ed]] 12:00, 1 May 2020 (UTC)", "imported": true},
                {"depth": 1, "heading": 3, "content": "Subsection", "imported": true},
            ]
        });
        let text = render(&names, &[t], &authors);
        assert_eq!(
            text,
            "== Coverage ==\n\
             Do we cover **maps**?\n\
             And atlases? [[User:Alice|Alice]] ([[User talk:Alice|talk]]) 16:22, 25 October 2026 (UTC)\n\
             :Yes. [[User:Bob|Bob]] ([[User talk:Bob|talk]]) 16:30, 25 October 2026 (UTC)\n\
             ::(comment removed) (username removed) 16:31, 25 October 2026 (UTC)\n\
             :Old comment [[User:Ed|Ed]] 12:00, 1 May 2020 (UTC)\n\
             === Subsection ===\n"
        );
    }

    #[test]
    fn a_post_without_words_is_its_signature() {
        let names = Names {
            user: "User",
            user_talk: "User talk",
        };
        let t = json!({"subject": "S", "posts": [
            {"depth": 1, "attributedTo": "k", "published": "2026-01-02T03:04:05Z", "content": ""}
        ]});
        assert_eq!(
            render(&names, &[t], &HashMap::new()),
            "== S ==\n:[[User:k|k]] ([[User talk:k|talk]]) 03:04, 2 January 2026 (UTC)\n"
        );
    }
}
