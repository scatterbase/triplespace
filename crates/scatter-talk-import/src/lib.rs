//! Planning the thread records of a converted talk page (ADR 0090 §1, §3, §8; architecture 13
//! §5.12–§5.15).
//!
//! [`plan`] maps a [`scatter_talk::Conversion`] onto [`scatter_threads`]' operations: one
//! [`PlannedThread`] per thread, the pinned header included, each a list of
//! [`PlannedRecord`]s in the order they are to be appended. The first is the thread's `create`
//! and the rest are `post`s. A record names the post it answers, and the range a collapsing
//! post collapses, by **position in that list**, because the revision IDs those fields need
//! exist only once the earlier records are appended. An import job appends the records in
//! order and calls [`PlannedThread::op`] for each, with the revision IDs it has so far.
//!
//! What a planned thread carries:
//! - **Status:** an imported section is created `archived` (0054 §5). A closing post carries
//!   `closed` (0090 §3), so a closed discussion shows as closed. The header is created open
//!   and pinned (0069 §7).
//! - **Text:** every post is wikitext, `text/x-wiki`.
//! - **Provenance:** `imported_from` holds the comment's DiscussionTools name and ID, signature,
//!   adding revision, original time, fallback reason and wrapper calls. On `create` it also
//!   holds where the thread came from, and the section's name hash (0069 §12).
//! - **Author:** each record's author is the one [`scatter_talk::attribute`] resolved, or the
//!   signer when attribution did not run. It is empty where only the importer can be named.
//!
//! **A thread whose first post is a heading post** (a section that opens with a subsection
//! heading) gets that heading as its opening post's text, in its original `=== … ===` form,
//! since `create` carries no `heading`. It still renders as a heading, and the comments
//! under it reply to it.
//!
//! Pure: no I/O. Which talk page is the home, how upstream actors are named, and what the
//! records' attestations hold are the import job's.

#![forbid(unsafe_code)]

use scatter_talk::{Conversion, Post, PostKind, Stamp, Thread, ThreadOrigin};
use scatter_threads::{
    AddedBy, ImportedFrom, Range, SubjectError, Target, ThreadOp, WIKITEXT, normalize_subject,
};
use sha2::{Digest, Sha256};

/// The status an imported section is created with (0054 §5).
pub const ARCHIVED: &str = "archived";

/// The status a closing post sets (0090 §3).
pub const CLOSED: &str = "closed";

/// Where the converted talk page came from: the source fields of a `create`'s
/// `imported_from`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    /// The page repository or wiki.
    pub repository: Option<String>,
    /// The talk page's upstream title.
    pub page: String,
    /// The upstream revision converted.
    pub revision: Option<u64>,
    /// The archive subpage, when the page converted is one.
    pub archive: Option<String>,
}

/// What a planned thread was made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The banners above the first heading.
    Header,
    /// A level-two section.
    Section,
    /// Signed comments above the first heading.
    BeforeFirstHeading,
    /// A page that would not divide.
    Page,
}

/// One record to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRecord {
    /// The text part: the post's wikitext.
    pub text: String,
    /// The post it answers, by position in the thread's records.
    pub reply_to: Option<usize>,
    /// The first and last posts it collapses, by position.
    pub collapse: Option<(usize, usize)>,
    /// A heading post's level.
    pub heading: Option<u8>,
    /// The status it sets.
    pub status: Option<&'static str>,
    /// Whether the thread is created pinned on its home.
    pub pinned: bool,
    /// Its provenance.
    pub imported_from: ImportedFrom,
    /// Its author as the source names actors, when one is known; otherwise the importer.
    pub author: Option<String>,
    /// Its original time.
    pub time: Option<Stamp>,
}

/// One thread to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedThread {
    /// What it was made from.
    pub origin: Origin,
    /// Its subject, normalized as a thread subject.
    pub subject: String,
    /// The earliest time among its records: the title date, when there is one (0054 §5).
    pub date: Option<Stamp>,
    /// Its records, in the order they are appended; the first is the `create`.
    pub records: Vec<PlannedRecord>,
}

impl PlannedThread {
    /// Record `k`'s content part, with `home` as the thread's home and `revids` the revision
    /// IDs of the records already appended, in order.
    ///
    /// # Panics
    ///
    /// When `revids` holds fewer than `k` IDs: records are appended in order.
    #[must_use]
    pub fn op(&self, k: usize, home: &Target, revids: &[u64]) -> ThreadOp {
        let r = &self.records[k];
        assert!(
            revids.len() >= k,
            "record {k} needs the IDs of the {k} before it"
        );
        let id = |i: usize| revids[i];
        if k == 0 {
            return ThreadOp::Create {
                name: self.subject.clone(),
                target: home.clone(),
                also: Vec::new(),
                status: r.status.map(str::to_string),
                pinned: r.pinned,
                media_type: Some(WIKITEXT.to_string()),
                imported_from: Some(r.imported_from.clone()),
            };
        }
        ThreadOp::Post {
            in_reply_to: r.reply_to.map(id),
            status: r.status.map(str::to_string),
            media_type: Some(WIKITEXT.to_string()),
            heading: r.heading,
            collapse: r.collapse.map(|(a, b)| Range {
                first: id(a),
                last: id(b),
            }),
            uncollapse: None,
            imported_from: Some(r.imported_from.clone()),
        }
    }
}

/// Plans every thread of a converted talk page titled `title`: the header first, then the
/// threads in page order.
#[must_use]
pub fn plan(conv: &Conversion, title: &str, source: &Source) -> Vec<PlannedThread> {
    let mut out = Vec::new();
    if let Some(header) = &conv.header {
        out.push(PlannedThread {
            origin: Origin::Header,
            subject: subject(&format!("{title}: talk page header")),
            date: None,
            records: vec![PlannedRecord {
                text: header.text.clone(),
                reply_to: None,
                collapse: None,
                heading: None,
                status: None,
                pinned: true,
                imported_from: source_fields(source, None),
                author: None,
                time: None,
            }],
        });
    }
    for thread in &conv.threads {
        out.push(plan_thread(thread, title, source));
    }
    out
}

fn plan_thread(thread: &Thread, title: &str, source: &Source) -> PlannedThread {
    let (origin, subject_text) = match thread.origin {
        ThreadOrigin::Section => (Origin::Section, plain_text(&thread.subject)),
        ThreadOrigin::BeforeFirstHeading => (
            Origin::BeforeFirstHeading,
            format!("{title}: before the first heading"),
        ),
        ThreadOrigin::Page => (Origin::Page, format!("{title}: imported talk page")),
    };
    let heading_first = matches!(
        thread.posts.first().map(|p| &p.kind),
        Some(PostKind::Heading { .. })
    );
    let mut records: Vec<PlannedRecord> = thread
        .posts
        .iter()
        .map(|p| record(p, heading_first))
        .collect();
    if let Some(first) = records.first_mut() {
        let hash = thread.dt.as_ref().map(|d| section_hash(&d.name));
        let mut from = source_fields(source, hash);
        let own = std::mem::take(&mut first.imported_from);
        from.name = own.name;
        from.id = own.id;
        from.signed_as = own.signed_as;
        from.added_by = own.added_by;
        from.timestamp = own.timestamp;
        from.fallback = own.fallback;
        from.wrapper = own.wrapper;
        first.imported_from = from;
        first.status = Some(ARCHIVED);
        if let (
            true,
            Some(Post {
                text,
                kind: PostKind::Heading { level },
                ..
            }),
        ) = (heading_first, thread.posts.first())
        {
            let marks = "=".repeat(usize::from(*level));
            first.text = format!("{marks} {text} {marks}");
            first.heading = None;
        }
    }
    PlannedThread {
        origin,
        subject: subject(&subject_text),
        date: records.iter().filter_map(|r| r.time).min(),
        records,
    }
}

/// One post's record. `heading_first`: the thread's first post, a heading, becomes the
/// `create`, which no collapsing post may reply to.
fn record(post: &Post, heading_first: bool) -> PlannedRecord {
    let attribution = post.attribution.as_ref();
    let signed_as = attribution
        .and_then(|a| a.signed_as.clone())
        .or_else(|| post.signature.as_ref().map(|s| s.user.clone()));
    let time = attribution
        .and_then(|a| a.timestamp)
        .or_else(|| post.signature.as_ref().and_then(|s| s.when));
    let author = attribution
        .map(|a| a.author.clone())
        .or_else(|| signed_as.clone());
    let mut from = ImportedFrom {
        name: post.dt.as_ref().map(|d| d.name.clone()),
        id: post.dt.as_ref().map(|d| d.id.clone()),
        signed_as,
        added_by: attribution
            .and_then(|a| a.added_by.as_ref())
            .map(|r| AddedBy {
                revision: r.id,
                actor: r.actor.clone(),
            }),
        timestamp: time.map(Stamp::iso),
        ..ImportedFrom::default()
    };
    let (mut heading, mut collapse, mut status, mut reply_to) = (None, None, None, post.parent);
    match &post.kind {
        PostKind::Comment | PostKind::Unsigned => {}
        PostKind::Heading { level } => heading = Some(*level),
        PostKind::Closing { wrapper } => {
            from.wrapper = Some(wrapper.clone());
            status = Some(CLOSED);
            reply_to = None;
        }
        PostKind::Collapse {
            first,
            last,
            wrapper,
        } => {
            from.wrapper = Some(wrapper.clone());
            collapse = Some((*first, *last));
            if heading_first && reply_to == Some(0) {
                reply_to = None;
            }
        }
        PostKind::Section(why) | PostKind::Page(why) => {
            from.fallback = Some(why.reason().to_string());
        }
    }
    PlannedRecord {
        text: post.text.clone(),
        reply_to,
        collapse,
        heading,
        status,
        pinned: false,
        imported_from: from,
        author: author.filter(|a| !a.is_empty()),
        time,
    }
}

fn source_fields(source: &Source, section_hash: Option<String>) -> ImportedFrom {
    ImportedFrom {
        repository: source.repository.clone(),
        page: Some(source.page.clone()),
        revision: source.revision,
        archive: source.archive.clone(),
        section_hash,
        ..ImportedFrom::default()
    }
}

/// The section's name hash (0069 §12): SHA-256 of its DiscussionTools name, truncated to 16
/// bytes, in hex.
#[must_use]
pub fn section_hash(name: &str) -> String {
    use std::fmt::Write;
    Sha256::digest(name.as_bytes())[..16]
        .iter()
        .fold(String::with_capacity(32), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// A heading's wikitext as plain text: links read as their labels; templates, tags and
/// emphasis removed; the characters a title may not hold dropped.
#[must_use]
pub fn plain_text(heading: &str) -> String {
    let mut out = String::with_capacity(heading.len());
    let mut rest = heading;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(close) = after.find("]]")
        {
            let inner = &after[..close];
            out.push_str(inner.rsplit('|').next().unwrap_or(inner));
            rest = &after[close + 2..];
        } else if let Some(after) = rest.strip_prefix("{{")
            && let Some(close) = after.find("}}")
        {
            rest = &after[close + 2..];
        } else if rest.starts_with('<')
            && let Some(close) = rest.find('>')
        {
            rest = &rest[close + 1..];
        } else if let Some(after) = rest.strip_prefix("''") {
            rest = after;
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            if !matches!(c, '#' | '<' | '>' | '[' | ']' | '|' | '{' | '}') {
                out.push(c);
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// A thread subject from any text: normalized, shortened to fit, or "Untitled section".
fn subject(text: &str) -> String {
    let mut s: String = text.chars().filter(|c| !c.is_control()).collect();
    loop {
        match normalize_subject(&s) {
            Ok(n) => return n,
            Err(SubjectError::TooLong) => {
                s.pop();
            }
            Err(_) => return "Untitled section".to_string(),
        }
    }
}
