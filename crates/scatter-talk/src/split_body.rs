//! Splitting one thread's body into posts: comments, unsigned text, heading posts and lifted
//! wrappers, with the section fallback (0090 §1–§4).

use std::collections::BTreeSet;
use std::ops::Range;

use crate::registry::{Markup, normalize_template_name};
use crate::sig::{SignatureKind, calls_in_line, signatures};
use crate::site::Site;
use crate::split::{
    Fallback, Post, PostKind, Thread, ThreadOrigin, line_indent, new_post, trim_lines,
};
use crate::split_headings::{HeadingInfo, Role, Span};

/// Constructs open across lines: tables, template calls, block elements and comments. A comment
/// boundary must not fall inside one (0090 §4).
#[derive(Debug, Clone, Default)]
pub(crate) struct Constructs {
    table: usize,
    template: usize,
    block: usize,
    comment: bool,
    /// Registered wrappers opened and not yet closed, beyond the lifted ones.
    wrapper: usize,
}

const BLOCK_TAGS: [&str; 4] = ["div", "table", "blockquote", "center"];

impl Constructs {
    pub(crate) fn is_open(&self) -> bool {
        self.open_kind().is_some()
    }

    pub(crate) fn open_kind(&self) -> Option<&'static str> {
        if self.wrapper > 0 {
            Some("partial-wrapper")
        } else if self.comment {
            Some("open-comment")
        } else if self.table > 0 {
            Some("open-table")
        } else if self.template > 0 {
            Some("open-template")
        } else if self.block > 0 {
            Some("open-block")
        } else {
            None
        }
    }

    pub(crate) fn feed(&mut self, line: &str) {
        let mut rest = line;
        // Comments first: text inside one counts for nothing else.
        let mut visible = String::with_capacity(line.len());
        loop {
            if self.comment {
                match rest.find("-->") {
                    Some(e) => {
                        self.comment = false;
                        rest = &rest[e + 3..];
                    }
                    None => break,
                }
            } else if let Some(s) = rest.find("<!--") {
                visible.push_str(&rest[..s]);
                self.comment = true;
                rest = &rest[s + 4..];
            } else {
                visible.push_str(rest);
                break;
            }
        }
        let t = visible.trim_start_matches([':', '*', '#', ' ']);
        if t.starts_with("{|") {
            self.table += 1;
        } else if t.starts_with("|}") {
            self.table = self.table.saturating_sub(1);
        }
        let opens = visible.matches("{{").count();
        let closes = visible.matches("}}").count();
        self.template = (self.template + opens).saturating_sub(closes);
        let lower = visible.to_ascii_lowercase();
        for tag in BLOCK_TAGS {
            let open = lower.matches(&format!("<{tag}")).count();
            let close = lower.matches(&format!("</{tag}")).count();
            self.block = (self.block + open).saturating_sub(close);
        }
    }

    /// Counts the registered wrappers a line opens and closes.
    pub(crate) fn feed_wrappers(&mut self, line: &str, markup: &Markup) {
        for name in template_names(line) {
            if markup.wrapper_opening(&name).is_some() {
                self.wrapper += 1;
            } else if markup.is_wrapper_template(&name) {
                self.wrapper = self.wrapper.saturating_sub(1);
            }
        }
    }
}

/// The fallback for a construct still open at a second comment boundary.
fn construct_fallback(kind: &'static str) -> Fallback {
    if kind == "partial-wrapper" {
        Fallback::PartialWrapper
    } else {
        Fallback::Construct(kind)
    }
}

/// The normalized names of the templates a line starts calling, closed on it or not.
fn template_names(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(p) = rest.find("{{") {
        rest = &rest[p + 2..];
        if rest.starts_with('{') {
            rest = rest.trim_start_matches('{');
            continue;
        }
        let end = rest.find(['|', '}', '\n']).unwrap_or(rest.len());
        out.push(normalize_template_name(&rest[..end]));
    }
    out
}

/// A lifted wrapper: its opening call's lines and its closing line.
#[derive(Debug, Clone)]
struct Lift {
    open: Range<usize>,
    close: usize,
}

/// The wrapper enclosing the whole scope, if its opening call starts the scope's first line and
/// its closing call is alone on the last.
fn lift_wrapper(lines: &[&str], scope: Range<usize>, markup: &Markup) -> Option<Lift> {
    let scope = trim_lines(lines, scope);
    if scope.len() < 2 {
        return None;
    }
    let first = lines[scope.start].trim_start();
    if !first.starts_with("{{") {
        return None;
    }
    let w = markup.wrapper_opening(template_names(first).first()?)?;
    // The opening call may run over several lines; it must end a line.
    let mut depth = 0usize;
    let mut open_end = None;
    for i in scope.clone() {
        let l = lines[i];
        depth += l.matches("{{").count();
        depth = depth.saturating_sub(l.matches("}}").count());
        if depth == 0 {
            if !l.trim_end().ends_with("}}") {
                return None;
            }
            open_end = Some(i + 1);
            break;
        }
    }
    let open_end = open_end?;
    let last = scope.end - 1;
    if last < open_end {
        return None;
    }
    let calls = calls_in_line(lines[last].trim());
    let only = calls.len() == 1 && calls[0].start == 0 && calls[0].end == lines[last].trim().len();
    (only && markup.wrappers[w].close.contains(&calls[0].name)).then_some(Lift {
        open: scope.start..open_end,
        close: last,
    })
}

struct Walk<'a> {
    lines: &'a [&'a str],
    markup: &'a Markup,
    site: &'a Site,
    posts: Vec<Post>,
    pending: Vec<usize>,
    outdent_next: bool,
    /// Heading posts open at this point: (level, post index).
    stack: Vec<(u8, usize)>,
    cons: Constructs,
    /// A comment whose signature fell inside a construct: it runs on to the line that closes
    /// the construct (0090 §4).
    extending: Option<usize>,
    fallback: Option<Fallback>,
}

impl Walk<'_> {
    /// Posts of the current subsection start after the innermost heading post.
    fn scope_start(&self) -> usize {
        self.stack.last().map_or(0, |(_, p)| p + 1)
    }

    fn heading_parent(&self) -> Option<usize> {
        self.stack.last().map(|(_, p)| *p)
    }

    fn parent_for(&self, depth: usize, outdented: bool) -> Option<usize> {
        let start = self.scope_start();
        let replyable = |p: &Post| matches!(p.kind, PostKind::Comment | PostKind::Unsigned);
        if outdented
            && let Some(k) = (start..self.posts.len())
                .rev()
                .find(|k| replyable(&self.posts[*k]))
        {
            return Some(k);
        }
        (start..self.posts.len())
            .rev()
            .find(|k| replyable(&self.posts[*k]) && self.posts[*k].depth < depth)
            .or_else(|| self.heading_parent())
    }

    fn flush_unsigned(&mut self) {
        let (Some(&a), Some(&b)) = (self.pending.first(), self.pending.last()) else {
            return;
        };
        self.pending.clear();
        let r = trim_lines(self.lines, a..b + 1);
        if r.is_empty() {
            return;
        }
        let depth = line_indent(self.lines[r.start]);
        let mut post = new_post(PostKind::Unsigned, self.lines, r, depth);
        post.parent = self.parent_for(depth, false);
        self.posts.push(post);
    }

    fn heading_post(&mut self, i: usize, level: u8, text: &str) {
        self.flush_unsigned();
        self.outdent_next = false;
        if let Some(kind) = self.cons.open_kind() {
            self.fallback.get_or_insert(construct_fallback(kind));
        }
        self.extending = None;
        while self.stack.last().is_some_and(|(l, _)| *l >= level) {
            self.stack.pop();
        }
        let mut post = new_post(PostKind::Heading { level }, self.lines, i..i + 1, 0);
        post.text = text.to_string();
        post.parent = self.heading_parent();
        self.posts.push(post);
        self.stack.push((level, self.posts.len() - 1));
    }

    fn wrapper_post(&mut self, lift: &Lift) {
        let mut post = new_post(
            PostKind::Wrapper {
                close: self.lines[lift.close].trim().to_string(),
            },
            self.lines,
            lift.open.clone(),
            0,
        );
        post.signature = lift
            .open
            .clone()
            .flat_map(|i| signatures(self.lines[i], self.markup, self.site))
            .find(|s| s.kind == SignatureKind::Dated);
        post.parent = self.heading_parent();
        self.posts.push(post);
    }

    fn feed(&mut self, line: &str) {
        self.cons.feed(line);
        self.cons.feed_wrappers(line, self.markup);
    }

    fn line(&mut self, i: usize) {
        let line = self.lines[i];
        if self.cons.comment {
            self.feed(line);
            match self.extending {
                Some(k) => self.extend(k, i),
                None => self.pending.push(i),
            }
            return;
        }
        let sigs = signatures(line, self.markup, self.site);
        if let Some(k) = self.extending {
            if sigs.is_empty() {
                self.feed(line);
                self.extend(k, i);
                return;
            }
            // A second boundary inside the construct: the section will not split.
            let kind = self.cons.open_kind().unwrap_or("open-construct");
            self.fallback.get_or_insert(construct_fallback(kind));
            self.extending = None;
        }
        self.feed(line);
        let outdent = calls_in_line(line)
            .iter()
            .any(|c| self.markup.outdent.contains(&c.name));
        if sigs.is_empty() {
            if outdent {
                self.outdent_next = true;
            }
            self.pending.push(i);
            return;
        }
        let first = self
            .pending
            .iter()
            .copied()
            .find(|k| !self.lines[*k].trim().is_empty())
            .unwrap_or(i);
        self.pending.clear();
        let depth = line_indent(self.lines[first]);
        let mut post = new_post(PostKind::Comment, self.lines, first..i + 1, depth);
        post.parent = self.parent_for(depth, self.outdent_next || outdent);
        let mut sigs = sigs.into_iter();
        post.signature = sigs.next();
        post.notes = sigs.collect();
        self.posts.push(post);
        self.outdent_next = false;
        if self.cons.is_open() {
            self.extending = Some(self.posts.len() - 1);
        }
    }

    /// Runs a comment on through line `i`, until its construct closes.
    fn extend(&mut self, k: usize, i: usize) {
        let post = &mut self.posts[k];
        post.lines.end = i + 1;
        post.text = crate::split::dedent(&self.lines[post.lines.clone()], post.depth);
        if !self.cons.is_open() {
            self.extending = None;
        }
    }
}

/// Splits one thread (0090 §1–§4). `headings` is the page's heading plan; a span with no
/// heading is the comments before the first heading.
pub(crate) fn split_thread(
    lines: &[&str],
    span: &Span,
    headings: &[HeadingInfo],
    markup: &Markup,
    site: &Site,
) -> Thread {
    let subject = span
        .heading
        .map(|k| headings[k].text.clone())
        .unwrap_or_default();
    let subs: Vec<&HeadingInfo> = headings
        .iter()
        .filter(|h| span.body.contains(&h.line) && matches!(h.role, Role::Sub(_)))
        .collect();
    // Scopes a wrapper may enclose: the whole body, and each subsection.
    let mut lifts = Vec::new();
    if let Some(l) = lift_wrapper(lines, span.body.clone(), markup) {
        lifts.push(l);
    }
    for (k, h) in subs.iter().enumerate() {
        let Role::Sub(level) = h.role else { continue };
        let end = subs[k + 1..]
            .iter()
            .find(|n| matches!(n.role, Role::Sub(l) if l <= level))
            .map_or(span.body.end, |n| n.line);
        if let Some(l) = lift_wrapper(lines, h.line + 1..end, markup) {
            lifts.push(l);
        }
    }
    let lifted: BTreeSet<usize> = lifts
        .iter()
        .flat_map(|l| l.open.clone().chain(std::iter::once(l.close)))
        .collect();
    let mut walk = Walk {
        lines,
        markup,
        site,
        posts: Vec::new(),
        pending: Vec::new(),
        outdent_next: false,
        stack: Vec::new(),
        cons: Constructs::default(),
        extending: None,
        fallback: None,
    };
    for i in span.body.clone() {
        if let Some(lift) = lifts.iter().find(|l| l.open.start == i) {
            walk.flush_unsigned();
            walk.wrapper_post(lift);
            continue;
        }
        if lifted.contains(&i) {
            continue;
        }
        match subs.iter().find(|h| h.line == i) {
            Some(h) => {
                let Role::Sub(level) = h.role else {
                    unreachable!()
                };
                walk.heading_post(i, level, &h.text);
            }
            None => walk.line(i),
        }
    }
    walk.flush_unsigned();
    if !walk.posts.iter().any(|p| p.kind == PostKind::Comment) {
        walk.fallback.get_or_insert(Fallback::NoSignature);
    }
    let mut thread = Thread {
        origin: ThreadOrigin::Section,
        subject,
        heading_line: span.heading.map(|k| headings[k].line),
        posts: walk.posts,
        fallback: walk.fallback,
        dt: None,
        dropped_parent_anchor: None,
    };
    if let Some(why) = thread.fallback {
        let r = trim_lines(lines, span.body.clone());
        let r = if r.is_empty() {
            span.body.start..span.body.start
        } else {
            r
        };
        thread.posts = vec![new_post(PostKind::Section(why), lines, r, 0)];
    }
    thread
}
