//! A thread as its readers see it (0019 §9; 14 §2; 0023 §5): the body of
//! `GET /thread/{id}`, which is also the content of a `triplespace-thread` revision
//! (0041 §5; content-models.toml), so the Action API's `prop=revisions` and
//! `action=compare` serve the thread as of each revision.
//!
//! [`body`] folds the thread's records, up to a revision when one is named, and shapes
//! them: subject, title, status, home and listings, the posts as `as:Note` objects in tree
//! order, and `base`, the offset a later write names. A `record` ACL hides a post's text
//! or author from those outside its group (`texthidden`, `userhidden`, the part itself
//! omitted); erased text is `texthidden` and `erased`. A post that actors other than its
//! author edited names them in `editedBy`, `null` for one hidden from the viewer. Whether
//! the viewer may read the thread at all is the caller's gate.

use std::collections::{HashMap, HashSet};

use scatter_actors::Timestamp;
use scatter_actors::evaluate::Effective;
use scatter_log_postgres::{PgClient, log};
use scatter_threads::{Category, Statuses, Target, Thread};
use serde_json::{Value, json};
use triplespace_projections::acl_read::record_acls;
use triplespace_projections::common::partition_of;
use triplespace_projections::feed::iso8601;
use triplespace_projections::threads::{read_record, text};

/// The thread's body as the viewer may see it, as of revision `upto` (the latest when
/// `None`). `None` when no thread has the page ID, or `upto` is none of its revisions.
///
/// # Errors
///
/// A database or record failure.
pub async fn body<C: PgClient + triplespace_projections::common::Partitions>(
    client: &C,
    tenant: &str,
    who: &Effective,
    id: u64,
    upto: Option<u64>,
    at: Timestamp,
) -> Result<Option<Value>, String> {
    let Some(partition) = partition_of(client, tenant, "pages").await? else {
        return Ok(None);
    };
    let records = log::read_by_key(client, partition, &id.to_string())
        .await
        .map_err(|e| e.to_string())?;
    let mut folded = Vec::new();
    let mut texts = HashMap::new();
    let mut offsets = HashMap::new();
    let mut erased = HashSet::new();
    let mut last_offset = 0;
    let mut reached = upto.is_none();
    for (offset, r) in &records {
        if r.header().payload_type != scatter_threads::PAYLOAD_THREAD {
            continue;
        }
        if upto.is_some() && reached {
            break;
        }
        last_offset = *offset;
        if upto.is_some() && r.header().revid == upto {
            reached = true;
        }
        let Some(rec) = read_record(r)? else {
            continue;
        };
        offsets.insert(rec.revid, *offset);
        match text(r) {
            Some(t) => {
                texts.insert(rec.revid, t);
            }
            None if rec.op.has_text() => {
                erased.insert(rec.revid);
            }
            None => {}
        }
        folded.push((rec, r.header().appended_at));
    }
    if folded.is_empty() || !reached {
        return Ok(None);
    }
    let appended: HashMap<u64, u64> = folded.iter().map(|(r, t)| (r.revid, *t)).collect();
    let actors: HashMap<u64, String> = folded
        .iter()
        .map(|(r, _)| (r.revid, r.actor.clone()))
        .collect();
    let thread = Thread::replay(folded.iter().map(|(r, _)| r)).map_err(|e| e.to_string())?;
    let shown = redactions(
        client, tenant, partition, &thread, &offsets, &actors, &erased, who, at,
    )
    .await?;
    let statuses = Statuses::embedded();
    let mut attachments = attachments_json(client, tenant, &thread, &appended).await;
    let home = attachments.remove(0);
    Ok(Some(json!({
        "id": id,
        "title": thread.title,
        "subject": thread.subject,
        "status": thread.status(),
        "category": match thread.category(statuses) { Category::Open => "open", Category::Closed => "closed" },
        "home": home,
        "listings": attachments,
        "posts": notes(&thread, &texts, &appended, &shown),
        "base": last_offset,
    })))
}

/// A thread target as JSON: `{kind: "page"|"actor", id}`.
#[must_use]
pub fn target_json(t: &Target) -> Value {
    match t {
        Target::Page(id) => json!({"kind": "page", "id": id}),
        Target::Actor(key) => json!({"kind": "actor", "id": key}),
    }
}

/// One part of a post's record, as the viewer sees it (0023 §5).
#[derive(Clone, Copy)]
struct Part {
    /// A `record` ACL hides it.
    hidden: bool,
    /// The viewer may still see it.
    visible: bool,
}

impl Default for Part {
    fn default() -> Self {
        Part {
            hidden: false,
            visible: true,
        }
    }
}

/// What the viewer may see of one post: the text of the record holding its current text,
/// and the actor of its own record; and who other than its author edited it, `None` for
/// an editor a `record` ACL hides from this viewer.
#[derive(Default, Clone)]
struct Shown {
    text: Part,
    user: Part,
    erased: bool,
    edited_by: Vec<Option<String>>,
}

/// Whether a record ACL hides a part, and whether this viewer may still see it.
fn part(acl: Option<&scatter_actors::Acl>, name: &str, who: &Effective, at: Timestamp) -> Part {
    let Some(a) = acl else {
        return Part::default();
    };
    let Some(r) = a.restriction_at(scatter_actors::acl::READ, at) else {
        return Part::default();
    };
    if !(a.parts.is_empty() || a.parts.iter().any(|p| p == name)) {
        return Part::default();
    }
    Part {
        hidden: true,
        visible: r.group.as_ref().is_some_and(|g| who.in_group(g)),
    }
}

/// A talk page's namespace and title.
async fn talk_title<C: PgClient>(client: &C, tenant: &str, talk: Option<u64>) -> Value {
    let Some(id) = talk.and_then(|t| i64::try_from(t).ok()) else {
        return Value::Null;
    };
    client
        .query_opt(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .ok()
        .flatten()
        .map_or(
            Value::Null,
            |r| json!({"ns": r.get::<_, i32>(0), "title": r.get::<_, String>(1)}),
        )
}

/// The home and the listings, each with its talk page and times.
async fn attachments_json<C: PgClient>(
    client: &C,
    tenant: &str,
    thread: &Thread,
    appended: &HashMap<u64, u64>,
) -> Vec<Value> {
    let mut attachments = Vec::new();
    for a in thread.attachments() {
        attachments.push(json!({
            "target": target_json(&a.target),
            "talk_page_id": a.talk,
            "talk_page": talk_title(client, tenant, a.talk).await,
            "attached": appended.get(&a.since).map(|t| iso8601(*t)),
            "pinned": a.pinned.and_then(|t| u64::try_from(t).ok()).map(iso8601),
        }));
    }
    attachments
}

/// What the viewer may see of each post, from the `record` ACLs of the thread's records.
#[allow(clippy::too_many_arguments)]
async fn redactions<C: PgClient>(
    client: &C,
    tenant: &str,
    partition: u64,
    thread: &Thread,
    offsets: &HashMap<u64, u64>,
    actors: &HashMap<u64, String>,
    erased: &HashSet<u64>,
    who: &Effective,
    at: Timestamp,
) -> Result<HashMap<u64, Shown>, String> {
    let all: Vec<u64> = offsets.values().copied().collect();
    let acls = record_acls(client, tenant, partition, &all).await?;
    let acl_of = |revid: u64| offsets.get(&revid).and_then(|o| acls.get(o));
    Ok(thread
        .posts()
        .iter()
        .map(|p| {
            (
                p.revid,
                Shown {
                    text: part(acl_of(p.text), "text", who, at),
                    user: part(acl_of(p.revid), "attestation", who, at),
                    erased: erased.contains(&p.text),
                    edited_by: editors(p, actors, |r| {
                        part(acl_of(r), "attestation", who, at).visible
                    }),
                },
            )
        })
        .collect())
}

/// The actors other than a post's author who edited it, in the order of their first edit;
/// `None` for one the viewer may not see.
fn editors(
    p: &scatter_threads::Post,
    actors: &HashMap<u64, String>,
    visible: impl Fn(u64) -> bool,
) -> Vec<Option<String>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in &p.edits {
        let Some(a) = actors.get(e) else { continue };
        if *a == p.actor || !seen.insert(a.clone()) {
            continue;
        }
        out.push(visible(*e).then(|| a.clone()));
    }
    out
}

/// The posts as `as:Note` objects in tree order: each post, then its replies.
fn notes(
    thread: &Thread,
    texts: &HashMap<u64, String>,
    appended: &HashMap<u64, u64>,
    shown: &HashMap<u64, Shown>,
) -> Vec<Value> {
    let posts = thread.posts();
    let mut children: HashMap<Option<u64>, Vec<usize>> = HashMap::new();
    for (k, p) in posts.iter().enumerate() {
        let parent = p.parent.filter(|r| thread.post(*r).is_some());
        children.entry(parent).or_default().push(k);
    }
    let mut out = Vec::with_capacity(posts.len());
    let mut stack: Vec<(usize, usize)> = children
        .get(&None)
        .map(|v| v.iter().rev().map(|k| (*k, 0)).collect())
        .unwrap_or_default();
    while let Some((k, depth)) = stack.pop() {
        let p = &posts[k];
        let mut note = json!({
            "type": "Note",
            "id": p.revid,
            "inReplyTo": p.parent,
            "attributedTo": p.actor,
            "published": appended.get(&p.revid).map(|t| iso8601(*t)),
            "updated": p.edits.last().and_then(|r| appended.get(r)).map(|t| iso8601(*t)),
            "mediaType": p.media_type,
            "content": texts.get(&p.text),
            "depth": depth,
            "collapsed": thread.is_collapsed(p.revid),
        });
        let sh = shown.get(&p.revid).cloned().unwrap_or_default();
        if !sh.edited_by.is_empty() {
            note["editedBy"] = json!(sh.edited_by);
        }
        if sh.erased {
            note["texthidden"] = json!(true);
            note["erased"] = json!(true);
        } else if sh.text.hidden {
            note["texthidden"] = json!(true);
        }
        if sh.erased || !sh.text.visible {
            note["content"] = Value::Null;
        }
        if sh.user.hidden {
            note["userhidden"] = json!(true);
            if !sh.user.visible {
                note["attributedTo"] = Value::Null;
            }
        }
        if let Some(s) = &p.status {
            note["status"] = json!(s);
        }
        if let Some(h) = p.heading {
            note["heading"] = json!(h);
        }
        if let Some(r) = p.collapse {
            note["collapse"] = json!({"first": r.first, "last": r.last});
        }
        if p.imported {
            note["imported"] = json!(true);
        }
        out.push(note);
        if let Some(kids) = children.get(&Some(p.revid)) {
            stack.extend(kids.iter().rev().map(|c| (*c, depth + 1)));
        }
    }
    out
}
