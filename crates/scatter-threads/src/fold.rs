//! The fold from a thread's records to its state, and the validation of each operation against
//! that state (14 §1.1–§1.5, §2.1, §2.4, §3.5–§3.6, §4.2).

use std::collections::HashMap;

use crate::op::{MARKDOWN, Range, Target, ThreadOp, WIKITEXT};
use crate::status::{Category, OPEN, Statuses};
use crate::title::{SubjectError, normalize_subject};

/// Who is writing a record. Some fields are written only by imports (0090 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    /// A person, through the API or the UI.
    Editor,
    /// An import: a fork's conversion, `Special:Import` or an adoption.
    Import,
}

/// What the appending transaction minted, carried in the record's attestation map (14 §1.5).
/// Empty while a record is validated before it is appended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Minted {
    /// The title, on `create` and `rename`.
    pub title: Option<String>,
    /// The talk page ID of the record's target: the home on `create` and `move`, the page on
    /// `attach`, `detach`, `pin` and `unpin`.
    pub talk: Option<u64>,
    /// On `create`, the talk page ID of each `also` target, in order.
    pub also: Vec<u64>,
}

/// One thread record, as the fold reads it.
#[derive(Debug, Clone)]
pub struct Record {
    /// Its global revision ID (header field 7); a post's identity.
    pub revid: u64,
    /// The actor key its attestation names.
    pub actor: String,
    /// Its time, as the header gives it.
    pub time: i64,
    /// The content part.
    pub op: ThreadOp,
    /// What the server minted.
    pub minted: Minted,
    /// Whether it carries a text part.
    pub has_text: bool,
}

/// The tenant's settings a record is checked against (23 §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// `thread.max_attachments`: the home and the listings together.
    pub max_attachments: usize,
    /// `thread.max_pinned`: threads pinned on one page.
    pub max_pinned: usize,
    /// `thread.markup`, as a media type: [`MARKDOWN`] or [`WIKITEXT`]. An original `create`
    /// or `post` must have it (14 §1.6; 0019 A28).
    pub markup: &'static str,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_attachments: 5,
            max_pinned: 3,
            markup: MARKDOWN,
        }
    }
}

/// What validation needs to know about the pages a thread is attached to, which only the store
/// can answer.
pub trait Pages {
    /// Whether the target exists and is not deleted.
    fn exists(&self, target: &Target) -> bool;
    /// Whether the target may be a thread's home: false for a board whose `homes` is false
    /// (14 §3.5).
    fn takes_homes(&self, target: &Target) -> bool;
    /// How many threads are pinned on the target's talk page now.
    fn pinned_on(&self, target: &Target) -> usize;
}

/// The pages of a replay: every target exists and takes homes, and nothing else is pinned.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnyPages;

impl Pages for AnyPages {
    fn exists(&self, _: &Target) -> bool {
        true
    }
    fn takes_homes(&self, _: &Target) -> bool {
        true
    }
    fn pinned_on(&self, _: &Target) -> usize {
        0
    }
}

/// Everything a record is checked against beside the thread's own state.
#[derive(Clone, Copy)]
pub struct Check<'a> {
    /// The tenant's statuses.
    pub statuses: &'a Statuses,
    /// The pages targets name.
    pub pages: &'a (dyn Pages + Sync),
    /// The tenant's limits.
    pub limits: Limits,
    /// Who is writing.
    pub writer: Writer,
}

impl Check<'static> {
    /// The check of a replay, over records already in the log: the default statuses, any
    /// pages, no limits, and an import's freedom, so that only the thread's own structure is
    /// checked.
    #[must_use]
    pub fn replay() -> Check<'static> {
        Check {
            statuses: Statuses::embedded(),
            pages: &AnyPages,
            limits: Limits {
                max_attachments: usize::MAX,
                max_pinned: usize::MAX,
                markup: MARKDOWN,
            },
            writer: Writer::Import,
        }
    }
}

impl std::fmt::Debug for Check<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Check")
            .field("limits", &self.limits)
            .field("writer", &self.writer)
            .finish_non_exhaustive()
    }
}

/// Why a record is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// The first record of a thread must be a `create`, and only the first.
    Create,
    /// The text part is missing where the operation carries one, or present where it does not.
    Text,
    /// The subject is not a valid subject.
    Subject(SubjectError),
    /// The status is not one the tenant has, or is retired.
    Status(String),
    /// The media type is not markdown or wikitext, or an edit's is not its post's.
    MediaType(String),
    /// A field only an import writes: `imported_from` or `heading`.
    ImportOnly(&'static str),
    /// A heading level outside 3 to 6.
    Heading(u8),
    /// The revision ID names no post of this thread.
    NotAPost(u64),
    /// A collapse's first post comes after its last.
    CollapseOrder,
    /// A collapse would overlap the range of this collapsing post without nesting in it.
    CollapseCrosses(u64),
    /// An editor's collapsing post is not a reply (14 §2.4).
    CollapseReply,
    /// A post both collapses and uncollapses.
    CollapseAndUncollapse,
    /// The revision ID names no collapsing post whose collapse stands.
    NotCollapsing(u64),
    /// The target does not exist or is deleted.
    NoTarget(Target),
    /// The target is a board that takes no homes.
    NoHomes(Target),
    /// A user talk page can only be a home.
    ActorListing,
    /// The thread is already attached there.
    Attached(Target),
    /// The target is not one of the thread's listings.
    NotAListing(Target),
    /// The thread is not attached there.
    NotAttached(Target),
    /// The move's target is already the home.
    SameHome,
    /// More attachments than `thread.max_attachments`.
    TooManyAttachments,
    /// The thread is already pinned there.
    Pinned(Target),
    /// The thread is not pinned there.
    NotPinned(Target),
    /// More pinned threads on the page than `thread.max_pinned`.
    TooManyPinned(Target),
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Create => f.write_str("a thread starts with exactly one `create`"),
            Invalid::Text => f.write_str("the text part does not match the operation"),
            Invalid::Subject(e) => write!(f, "{e}"),
            Invalid::Status(s) => write!(f, "`{s}` is not a status this wiki sets"),
            Invalid::MediaType(m) => write!(f, "`{m}` is not a media type this post may have"),
            Invalid::ImportOnly(what) => write!(f, "only an import writes `{what}`"),
            Invalid::Heading(l) => write!(f, "heading level {l} is not 3 to 6"),
            Invalid::NotAPost(r) => write!(f, "revision {r} is not a post of this thread"),
            Invalid::CollapseOrder => f.write_str("a collapse's first post comes after its last"),
            Invalid::CollapseCrosses(r) => {
                write!(f, "the range overlaps the one post {r} collapses")
            }
            Invalid::CollapseReply => f.write_str("a collapsing post is not a reply"),
            Invalid::CollapseAndUncollapse => {
                f.write_str("a post may collapse or uncollapse, not both")
            }
            Invalid::NotCollapsing(r) => write!(f, "post {r} collapses nothing"),
            Invalid::NoTarget(t) => write!(f, "{t:?} does not exist"),
            Invalid::NoHomes(t) => write!(f, "{t:?} is a board that homes no threads"),
            Invalid::ActorListing => f.write_str("a user talk page can only be a thread's home"),
            Invalid::Attached(t) => write!(f, "the thread is already attached to {t:?}"),
            Invalid::NotAListing(t) => write!(f, "{t:?} is not one of the thread's listings"),
            Invalid::NotAttached(t) => write!(f, "the thread is not attached to {t:?}"),
            Invalid::SameHome => f.write_str("the thread already lives there"),
            Invalid::TooManyAttachments => f.write_str("the thread has too many attachments"),
            Invalid::Pinned(t) => write!(f, "the thread is already pinned on {t:?}"),
            Invalid::NotPinned(t) => write!(f, "the thread is not pinned on {t:?}"),
            Invalid::TooManyPinned(t) => write!(f, "{t:?} has too many pinned threads"),
        }
    }
}

impl std::error::Error for Invalid {}

/// One page a thread is attached to (14 §3.5, §4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The page or actor.
    pub target: Target,
    /// Its talk page ID, once minted.
    pub talk: Option<u64>,
    /// The record that attached the thread there.
    pub since: u64,
    /// When the thread was pinned there, if it is.
    pub pinned: Option<i64>,
}

/// One post: the opening post, a reply, a heading post or a collapsing post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// Its revision ID.
    pub revid: u64,
    /// The post it answers.
    pub parent: Option<u64>,
    /// Its author.
    pub actor: String,
    /// When it was posted.
    pub time: i64,
    /// Its text's media type.
    pub media_type: String,
    /// The record holding its current text: its own, or its latest `edit`.
    pub text: u64,
    /// Its `edit` records, oldest first.
    pub edits: Vec<u64>,
    /// The status it set.
    pub status: Option<String>,
    /// Its heading level, for a heading post.
    pub heading: Option<u8>,
    /// The range it collapses, while its collapse stands.
    pub collapse: Option<Range>,
    /// Whether an import wrote it.
    pub imported: bool,
}

/// A thread's state: the fold of its records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// The `create` record's revision ID, which is also the opening post's.
    pub created: u64,
    /// The subject.
    pub subject: String,
    /// The title, as minted.
    pub title: Option<String>,
    /// The home.
    pub home: Attachment,
    /// The listings, in the order they were attached.
    pub listings: Vec<Attachment>,
    /// The latest status set, and the post that set it.
    pub status: Option<(String, u64)>,
    /// The latest record folded.
    pub last: u64,
    posts: Vec<Post>,
    position: HashMap<u64, usize>,
}

fn media_type(given: Option<&String>) -> &str {
    given.map_or(MARKDOWN, String::as_str)
}

/// An import writes either markup; an editor's original post has the tenant's
/// `thread.markup` (14 §1.6; 0019 A28).
fn check_media(given: Option<&String>, check: &Check) -> Result<(), Invalid> {
    match (media_type(given), check.writer) {
        (MARKDOWN | WIKITEXT, Writer::Import) => Ok(()),
        (m, Writer::Editor) if m == check.limits.markup => Ok(()),
        (other, _) => Err(Invalid::MediaType(other.to_string())),
    }
}

fn check_status(status: Option<&String>, check: &Check) -> Result<(), Invalid> {
    match status {
        Some(s) if check.statuses.get(s).is_none_or(|st| st.retired) => {
            Err(Invalid::Status(s.clone()))
        }
        _ => Ok(()),
    }
}

impl Thread {
    /// Starts a thread from its `create` record.
    pub fn create(rec: &Record, check: &Check) -> Result<Thread, Invalid> {
        let ThreadOp::Create {
            name,
            target,
            also,
            status,
            pinned,
            media_type: mt,
            imported_from,
        } = &rec.op
        else {
            return Err(Invalid::Create);
        };
        if !rec.has_text {
            return Err(Invalid::Text);
        }
        let subject = normalize_subject(name).map_err(Invalid::Subject)?;
        check_media(mt.as_ref(), check)?;
        check_status(status.as_ref(), check)?;
        if imported_from.is_some() && check.writer != Writer::Import {
            return Err(Invalid::ImportOnly("imported_from"));
        }
        if !check.pages.exists(target) {
            return Err(Invalid::NoTarget(target.clone()));
        }
        if !check.pages.takes_homes(target) {
            return Err(Invalid::NoHomes(target.clone()));
        }
        if *pinned && check.pages.pinned_on(target) >= check.limits.max_pinned {
            return Err(Invalid::TooManyPinned(target.clone()));
        }
        let mut thread = Thread {
            created: rec.revid,
            subject,
            title: rec.minted.title.clone(),
            home: Attachment {
                target: target.clone(),
                talk: rec.minted.talk,
                since: rec.revid,
                pinned: pinned.then_some(rec.time),
            },
            listings: Vec::new(),
            status: status.clone().map(|s| (s, rec.revid)),
            last: rec.revid,
            posts: Vec::new(),
            position: HashMap::new(),
        };
        for (k, t) in also.iter().enumerate() {
            thread.check_attach(t, check)?;
            thread.listings.push(Attachment {
                target: t.clone(),
                talk: rec.minted.also.get(k).copied(),
                since: rec.revid,
                pinned: None,
            });
        }
        thread.push_post(Post {
            revid: rec.revid,
            parent: None,
            actor: rec.actor.clone(),
            time: rec.time,
            media_type: media_type(mt.as_ref()).to_string(),
            text: rec.revid,
            edits: Vec::new(),
            status: status.clone(),
            heading: None,
            collapse: None,
            imported: imported_from.is_some(),
        });
        Ok(thread)
    }

    /// Folds records, the first a `create`, checking each as a replay does.
    pub fn replay<'r>(records: impl IntoIterator<Item = &'r Record>) -> Result<Thread, Invalid> {
        let check = Check::replay();
        let mut records = records.into_iter();
        let first = records.next().ok_or(Invalid::Create)?;
        let mut thread = Thread::create(first, &check)?;
        for rec in records {
            thread.apply(rec, &check)?;
        }
        Ok(thread)
    }

    /// Checks a record against the thread, without changing it: what the write path asks
    /// before it appends.
    pub fn validate(&self, rec: &Record, check: &Check) -> Result<(), Invalid> {
        self.clone().apply(rec, check)
    }

    /// Checks a record and folds it in. On an error the thread is unchanged.
    pub fn apply(&mut self, rec: &Record, check: &Check) -> Result<(), Invalid> {
        if rec.has_text != rec.op.has_text() {
            return Err(Invalid::Text);
        }
        match &rec.op {
            ThreadOp::Create { .. } => return Err(Invalid::Create),
            ThreadOp::Post { .. } => self.apply_post(rec, check)?,
            ThreadOp::Edit {
                object,
                media_type: mt,
            } => {
                let k = self.position_of(*object)?;
                let given = media_type(mt.as_ref());
                if given != self.posts[k].media_type {
                    return Err(Invalid::MediaType(given.to_string()));
                }
                let post = &mut self.posts[k];
                post.text = rec.revid;
                post.edits.push(rec.revid);
            }
            ThreadOp::Rename { name } => {
                self.subject = normalize_subject(name).map_err(Invalid::Subject)?;
                if rec.minted.title.is_some() {
                    self.title.clone_from(&rec.minted.title);
                }
            }
            ThreadOp::Move { target, keep } => self.apply_move(rec, target, *keep, check)?,
            ThreadOp::Attach { target } => {
                self.check_attach(target, check)?;
                self.listings.push(Attachment {
                    target: target.clone(),
                    talk: rec.minted.talk,
                    since: rec.revid,
                    pinned: None,
                });
            }
            ThreadOp::Detach { target } => {
                let k = self
                    .listings
                    .iter()
                    .position(|a| &a.target == target)
                    .ok_or_else(|| Invalid::NotAListing(target.clone()))?;
                self.listings.remove(k);
            }
            ThreadOp::Pin { target } => {
                if check.pages.pinned_on(target) >= check.limits.max_pinned {
                    return Err(Invalid::TooManyPinned(target.clone()));
                }
                let a = self.attachment_mut(target)?;
                if a.pinned.is_some() {
                    return Err(Invalid::Pinned(target.clone()));
                }
                a.pinned = Some(rec.time);
            }
            ThreadOp::Unpin { target } => {
                let a = self.attachment_mut(target)?;
                if a.pinned.take().is_none() {
                    return Err(Invalid::NotPinned(target.clone()));
                }
            }
        }
        self.last = rec.revid;
        Ok(())
    }

    fn apply_post(&mut self, rec: &Record, check: &Check) -> Result<(), Invalid> {
        let ThreadOp::Post {
            in_reply_to,
            status,
            media_type: mt,
            heading,
            collapse,
            uncollapse,
            imported_from,
        } = &rec.op
        else {
            return Err(Invalid::Create);
        };
        check_media(mt.as_ref(), check)?;
        check_status(status.as_ref(), check)?;
        if check.writer != Writer::Import {
            if imported_from.is_some() {
                return Err(Invalid::ImportOnly("imported_from"));
            }
            if heading.is_some() {
                return Err(Invalid::ImportOnly("heading"));
            }
        }
        if let Some(l) = heading
            && !(3..=6).contains(l)
        {
            return Err(Invalid::Heading(*l));
        }
        if let Some(p) = in_reply_to {
            self.position_of(*p)?;
        }
        if collapse.is_some() && uncollapse.is_some() {
            return Err(Invalid::CollapseAndUncollapse);
        }
        if let Some(range) = collapse {
            self.check_collapse(*range, *in_reply_to, check)?;
        }
        if let Some(c) = uncollapse {
            let k = self.position_of(*c)?;
            if self.posts[k].collapse.is_none() {
                return Err(Invalid::NotCollapsing(*c));
            }
            self.posts[k].collapse = None;
        }
        if let Some(s) = status {
            self.status = Some((s.clone(), rec.revid));
        }
        self.push_post(Post {
            revid: rec.revid,
            parent: *in_reply_to,
            actor: rec.actor.clone(),
            time: rec.time,
            media_type: media_type(mt.as_ref()).to_string(),
            text: rec.revid,
            edits: Vec::new(),
            status: status.clone(),
            heading: *heading,
            collapse: *collapse,
            imported: imported_from.is_some(),
        });
        Ok(())
    }

    fn push_post(&mut self, post: Post) {
        self.position.insert(post.revid, self.posts.len());
        self.posts.push(post);
    }

    fn position_of(&self, revid: u64) -> Result<usize, Invalid> {
        self.position
            .get(&revid)
            .copied()
            .ok_or(Invalid::NotAPost(revid))
    }

    fn check_collapse(
        &self,
        range: Range,
        in_reply_to: Option<u64>,
        check: &Check,
    ) -> Result<(), Invalid> {
        let (lo, hi) = (
            self.position_of(range.first)?,
            self.position_of(range.last)?,
        );
        if lo > hi {
            return Err(Invalid::CollapseOrder);
        }
        match (check.writer, in_reply_to) {
            (_, None) => {}
            (Writer::Import, Some(p)) if self.posts[self.position_of(p)?].heading.is_some() => {}
            _ => return Err(Invalid::CollapseReply),
        }
        for other in &self.posts {
            let Some(theirs) = other.collapse else {
                continue;
            };
            let (their_lo, their_hi) = (self.position[&theirs.first], self.position[&theirs.last]);
            let disjoint = hi < their_lo || their_hi < lo;
            let nested = (lo <= their_lo && their_hi <= hi) || (their_lo <= lo && hi <= their_hi);
            if !disjoint && !nested {
                return Err(Invalid::CollapseCrosses(other.revid));
            }
        }
        Ok(())
    }

    fn attachment_count(&self) -> usize {
        1 + self.listings.len()
    }

    fn check_attach(&self, target: &Target, check: &Check) -> Result<(), Invalid> {
        if matches!(target, Target::Actor(_)) {
            return Err(Invalid::ActorListing);
        }
        if !check.pages.exists(target) {
            return Err(Invalid::NoTarget(target.clone()));
        }
        if self.is_attached(target) {
            return Err(Invalid::Attached(target.clone()));
        }
        if self.attachment_count() + 1 > check.limits.max_attachments {
            return Err(Invalid::TooManyAttachments);
        }
        Ok(())
    }

    fn apply_move(
        &mut self,
        rec: &Record,
        target: &Target,
        keep: bool,
        check: &Check,
    ) -> Result<(), Invalid> {
        if &self.home.target == target {
            return Err(Invalid::SameHome);
        }
        if !check.pages.exists(target) {
            return Err(Invalid::NoTarget(target.clone()));
        }
        if !check.pages.takes_homes(target) {
            return Err(Invalid::NoHomes(target.clone()));
        }
        let promoted = self.listings.iter().position(|a| &a.target == target);
        if keep {
            if matches!(self.home.target, Target::Actor(_)) {
                return Err(Invalid::ActorListing);
            }
            let count = self.attachment_count() + usize::from(promoted.is_none());
            if count > check.limits.max_attachments {
                return Err(Invalid::TooManyAttachments);
            }
        }
        let new_home = match promoted {
            Some(k) => {
                let mut a = self.listings.remove(k);
                a.since = rec.revid;
                if rec.minted.talk.is_some() {
                    a.talk = rec.minted.talk;
                }
                a
            }
            None => Attachment {
                target: target.clone(),
                talk: rec.minted.talk,
                since: rec.revid,
                pinned: None,
            },
        };
        let mut old = std::mem::replace(&mut self.home, new_home);
        if keep {
            old.since = rec.revid;
            self.listings.push(old);
        }
        Ok(())
    }

    fn attachment_mut(&mut self, target: &Target) -> Result<&mut Attachment, Invalid> {
        if &self.home.target == target {
            return Ok(&mut self.home);
        }
        self.listings
            .iter_mut()
            .find(|a| &a.target == target)
            .ok_or_else(|| Invalid::NotAttached(target.clone()))
    }

    /// Whether the thread is attached to the target, as home or listing.
    #[must_use]
    pub fn is_attached(&self, target: &Target) -> bool {
        &self.home.target == target || self.listings.iter().any(|a| &a.target == target)
    }

    /// The home, then the listings.
    pub fn attachments(&self) -> impl Iterator<Item = &Attachment> {
        std::iter::once(&self.home).chain(&self.listings)
    }

    /// The posts, in the thread's order: the order they were appended.
    #[must_use]
    pub fn posts(&self) -> &[Post] {
        &self.posts
    }

    /// A post by revision ID.
    #[must_use]
    pub fn post(&self, revid: u64) -> Option<&Post> {
        self.position.get(&revid).map(|k| &self.posts[*k])
    }

    /// The thread's status: that of its latest status-bearing post, or `open`.
    #[must_use]
    pub fn status(&self) -> &str {
        self.status.as_ref().map_or(OPEN, |(s, _)| s.as_str())
    }

    /// Whether the status leaves the thread open or closes it; a status the registry no longer
    /// lists reads as open.
    #[must_use]
    pub fn category(&self, statuses: &Statuses) -> Category {
        statuses
            .get(self.status())
            .map_or(Category::Open, |s| s.category)
    }

    /// The collapses that stand: each collapsing post with its range.
    pub fn collapses(&self) -> impl Iterator<Item = (u64, Range)> + '_ {
        self.posts
            .iter()
            .filter_map(|p| p.collapse.map(|r| (p.revid, r)))
    }

    /// Whether a post is inside a standing collapse.
    #[must_use]
    pub fn is_collapsed(&self, revid: u64) -> bool {
        let Some(&k) = self.position.get(&revid) else {
            return false;
        };
        self.collapses()
            .any(|(_, r)| self.position[&r.first] <= k && k <= self.position[&r.last])
    }
}
