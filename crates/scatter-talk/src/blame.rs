//! Attribution from history: a line-level blame over a page's revisions, and each post's
//! author resolved from its signature and the revision that added it (0090 §6–§7).
//!
//! The blame keeps only what attribution needs: the current lines with the revision that
//! first added each, and every line ever seen with its origin. The texts themselves are not
//! kept beyond the current revision (0090 §7).

use std::collections::{HashMap, HashSet};

use similar::{Algorithm, DiffTag, capture_diff_slices};

use crate::registry::Markup;
use crate::sig::{SignatureKind, signatures};
use crate::site::Site;
use crate::split::{Conversion, Post, PostKind, Thread, line_indent, new_post};
use crate::time::Stamp;

/// Lines this long or shorter are not remembered across removals: too many are alike.
const MEMORY_MIN: usize = 20;

/// One revision of the page, as the history source gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// The upstream revision ID.
    pub id: u64,
    /// Its actor, as the source names it.
    pub actor: String,
    /// Its time.
    pub timestamp: Stamp,
}

/// A line-level blame over a page's history (0090 §7).
pub struct Blame<'a> {
    markup: &'a Markup,
    site: &'a Site,
    revisions: Vec<Revision>,
    lines: Vec<String>,
    origin: Vec<usize>,
    first_seen: HashMap<String, usize>,
    seen_signatures: HashSet<(String, Stamp)>,
    /// (actor, signed name) → how many signatures the actor added under that name.
    signed: HashMap<(String, String), u32>,
    /// actor → signatures added under any name but the actor's own.
    signed_other: HashMap<String, u32>,
    renames: Vec<(String, String)>,
}

impl<'a> Blame<'a> {
    /// An empty blame, before the first revision.
    pub fn new(markup: &'a Markup, site: &'a Site) -> Self {
        Blame {
            markup,
            site,
            revisions: Vec::new(),
            lines: Vec::new(),
            origin: Vec::new(),
            first_seen: HashMap::new(),
            seen_signatures: HashSet::new(),
            signed: HashMap::new(),
            signed_other: HashMap::new(),
            renames: Vec::new(),
        }
    }

    /// Records a rename from the repository's rename log: `old` is a former name of `new`.
    pub fn add_rename(&mut self, old: &str, new: &str) {
        self.renames.push((old.to_string(), new.to_string()));
    }

    /// How many revisions have been read.
    pub fn len(&self) -> usize {
        self.revisions.len()
    }

    /// Whether no revision has been read.
    pub fn is_empty(&self) -> bool {
        self.revisions.is_empty()
    }

    /// Reads the next revision, oldest first.
    pub fn push(&mut self, revision: Revision, text: &str) {
        let r = self.revisions.len();
        let new: Vec<&str> = text.split('\n').collect();
        let old: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        let mut origin = Vec::with_capacity(new.len());
        let mut added = Vec::new();
        for op in capture_diff_slices(Algorithm::Myers, &old, &new) {
            let (tag, old_range, new_range) = op.as_tag_tuple();
            match tag {
                DiffTag::Equal => origin.extend(old_range.map(|i| self.origin[i])),
                DiffTag::Delete => {}
                DiffTag::Insert | DiffTag::Replace => {
                    for i in new_range {
                        let line = new[i];
                        if let Some(o) = self
                            .first_seen
                            .get(line)
                            .filter(|_| line.trim().len() > MEMORY_MIN)
                        {
                            origin.push(*o);
                        } else {
                            origin.push(r);
                            added.push(i);
                        }
                    }
                }
            }
        }
        for (i, line) in new.iter().enumerate() {
            if line.trim().len() > MEMORY_MIN {
                self.first_seen
                    .entry((*line).to_string())
                    .or_insert(origin[i]);
            }
        }
        for i in added {
            for s in signatures(new[i], self.markup, self.site) {
                let Some(when) = s.when else { continue };
                if s.kind != SignatureKind::Dated
                    || !self.seen_signatures.insert((s.user.clone(), when))
                {
                    continue;
                }
                *self
                    .signed
                    .entry((revision.actor.clone(), s.user.clone()))
                    .or_insert(0) += 1;
                if s.user != revision.actor {
                    *self.signed_other.entry(revision.actor.clone()).or_insert(0) += 1;
                }
            }
        }
        self.lines = new.into_iter().map(str::to_string).collect();
        self.origin = origin;
        self.revisions.push(revision);
    }

    /// The revision that first added a line: by position when the converted text is the last
    /// revision read, otherwise by content (an archive's lines, matched against the parent's
    /// history).
    pub fn origin_of(&self, index: usize, text: &str) -> Option<&Revision> {
        if self.lines.get(index).is_some_and(|l| l == text) {
            return self.revisions.get(self.origin[index]);
        }
        if text.trim().len() > MEMORY_MIN {
            return self
                .first_seen
                .get(text)
                .and_then(|o| self.revisions.get(*o));
        }
        None
    }

    /// Whether `name` is a name `actor` signs with: a former name in the rename log, or a name
    /// it signed with in its own additions at least twice, and for at least half of its
    /// signatures under names not its own. The share keeps a delivery bot, which signs as many
    /// senders, from taking any of their names (0090 §6).
    pub fn is_alias(&self, actor: &str, name: &str) -> bool {
        if self
            .renames
            .iter()
            .any(|(old, new)| old == name && new == actor)
        {
            return true;
        }
        let n = self
            .signed
            .get(&(actor.to_string(), name.to_string()))
            .copied()
            .unwrap_or(0);
        let others = self.signed_other.get(actor).copied().unwrap_or(0);
        n >= 2 && 2 * n >= others
    }
}

/// How a post's author was decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorSource {
    /// History and signature agree: the account that added it.
    History,
    /// The signature, where history names someone else or there is no history.
    Signature,
    /// No signature: the account whose revision added the text.
    Revision,
    /// Nothing else: the person who started the job.
    Importer,
}

/// Who wrote a post, and the evidence (0090 §6, §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    /// The author, as the source names actors.
    pub author: String,
    /// How the author was decided.
    pub source: AuthorSource,
    /// The user the post's signature names.
    pub signed_as: Option<String>,
    /// The upstream revision that added the post's signature line (or first line), and its
    /// actor.
    pub added_by: Option<Revision>,
    /// The post's original time: its signature's, or its adding revision's.
    pub timestamp: Option<Stamp>,
}

fn by_signature(
    user: &str,
    when: Option<Stamp>,
    added: Option<&Revision>,
    blame: Option<&Blame>,
) -> Attribution {
    let agrees = added
        .zip(blame)
        .is_some_and(|(r, b)| r.actor == user || b.is_alias(&r.actor, user));
    Attribution {
        author: if agrees {
            added.map_or(user, |r| r.actor.as_str()).to_string()
        } else {
            user.to_string()
        },
        source: if agrees {
            AuthorSource::History
        } else {
            AuthorSource::Signature
        },
        signed_as: Some(user.to_string()),
        added_by: added.cloned(),
        timestamp: when.or(added.map(|r| r.timestamp)),
    }
}

fn by_revision(added: Option<&Revision>, importer: &str) -> Attribution {
    match added {
        Some(r) => Attribution {
            author: r.actor.clone(),
            source: AuthorSource::Revision,
            signed_as: None,
            added_by: Some(r.clone()),
            timestamp: Some(r.timestamp),
        },
        None => Attribution {
            author: importer.to_string(),
            source: AuthorSource::Importer,
            signed_as: None,
            added_by: None,
            timestamp: None,
        },
    }
}

/// Splits off the leading lines of each comment that history credits to someone other than
/// the comment's signer, as unsigned posts of their own (0090 §6).
fn split_by_history(t: &mut Thread, lines: &[&str], blame: &Blame) {
    let mut out: Vec<Post> = Vec::with_capacity(t.posts.len());
    // Where each old post's outputs start, and where the post itself landed.
    let mut starts = Vec::with_capacity(t.posts.len());
    let mut map = Vec::with_capacity(t.posts.len());
    for post in std::mem::take(&mut t.posts) {
        let r = post.lines.clone();
        starts.push(out.len());
        let split_at = if post.kind == PostKind::Comment && r.len() > 1 {
            let sig_line = signature_line(&post, lines, blame.markup, blame.site);
            blame.origin_of(sig_line, lines[sig_line]).map(|signer| {
                let user = post.signature.as_ref().map_or("", |s| s.user.as_str());
                (r.start..sig_line)
                    .find(|&i| {
                        lines[i].trim().is_empty()
                            || blame.origin_of(i, lines[i]).is_none_or(|o| {
                                o.actor == signer.actor
                                    || o.actor == user
                                    || blame.is_alias(&o.actor, user)
                            })
                    })
                    .unwrap_or(sig_line)
            })
        } else {
            None
        };
        match split_at {
            Some(b) if b > r.start => {
                let mut unsigned = new_post(
                    PostKind::Unsigned,
                    lines,
                    r.start..b,
                    line_indent(lines[r.start]),
                );
                unsigned.parent = post.parent;
                out.push(unsigned);
                let depth = line_indent(lines[b]);
                let mut rest = new_post(PostKind::Comment, lines, b..r.end, depth);
                rest.parent = post.parent;
                rest.signature = post.signature;
                rest.notes = post.notes;
                rest.dt = post.dt;
                map.push(out.len());
                out.push(rest);
            }
            _ => {
                map.push(out.len());
                out.push(post);
            }
        }
    }
    for p in &mut out {
        p.parent = p.parent.map(|q| map[q]);
        if let PostKind::Collapse { first, last, .. } = &mut p.kind {
            *first = starts[*first];
            *last = map[*last];
        }
    }
    t.posts = out;
}

/// The line of a post that carries its signature.
fn signature_line(post: &Post, lines: &[&str], markup: &Markup, site: &Site) -> usize {
    match post.kind {
        PostKind::Comment => post
            .lines
            .clone()
            .rev()
            .find(|&i| !signatures(lines[i], markup, site).is_empty())
            .unwrap_or(post.lines.end - 1),
        _ => post
            .lines
            .clone()
            .find(|&i| !signatures(lines[i], markup, site).is_empty())
            .unwrap_or(post.lines.start),
    }
}

/// Resolves every post's author (0090 §6), splitting off text history credits to someone else
/// when `blame` is given. `lines` is the converted text's lines; `importer` names the person who
/// started the job.
pub fn attribute(
    conv: &mut Conversion,
    text: &str,
    blame: Option<&Blame>,
    importer: &str,
    markup: &Markup,
    site: &Site,
) {
    let lines: Vec<&str> = text.split('\n').collect();
    for t in &mut conv.threads {
        if let Some(b) = blame
            && t.fallback.is_none()
        {
            split_by_history(t, &lines, b);
        }
        for k in 0..t.posts.len() {
            let a = attribute_post(t, k, &lines, blame, importer, markup, site);
            t.posts[k].attribution = Some(a);
        }
    }
}

fn attribute_post(
    t: &Thread,
    k: usize,
    lines: &[&str],
    blame: Option<&Blame>,
    importer: &str,
    markup: &Markup,
    site: &Site,
) -> Attribution {
    let post = &t.posts[k];
    let origin = |i: usize| blame.and_then(|b| b.origin_of(i, lines[i]));
    match &post.kind {
        PostKind::Comment | PostKind::Closing { .. } | PostKind::Collapse { .. } => {
            let line = signature_line(post, lines, markup, site);
            match &post.signature {
                Some(s) => by_signature(&s.user, s.when, origin(line), blame),
                None => by_revision(origin(line), importer),
            }
        }
        PostKind::Unsigned => by_revision(origin(post.lines.start), importer),
        PostKind::Heading { .. } => match origin(post.lines.start) {
            Some(r) => by_revision(Some(r), importer),
            None => t.posts[k + 1..]
                .iter()
                .find(|p| p.parent == Some(k) && p.signature.is_some())
                .and_then(|p| p.signature.as_ref())
                .map_or_else(
                    || by_revision(None, importer),
                    |s| by_signature(&s.user, s.when, None, None),
                ),
        },
        PostKind::Section(_) => {
            let first = post.lines.clone().find_map(|i| {
                signatures(lines[i], markup, site)
                    .into_iter()
                    .next()
                    .map(|s| (i, s))
            });
            match first {
                Some((i, s)) => by_signature(&s.user, s.when, origin(i), blame),
                None => by_revision(None, importer),
            }
        }
        PostKind::Page(_) => by_revision(None, importer),
    }
}
