//! Splitting one thread's body into posts: comments, unsigned text, heading posts, and closing
//! and collapsing posts for wrappers, with the section fallback (0090 §1–§4).

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
    /// Registered wrappers opened inside a comment's text and not yet closed.
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

/// A wrapper's opening call alone on its lines, from line `i`: the lines it covers (it may run
/// over several, and must end a line).
fn wrapper_open(lines: &[&str], i: usize, end: usize, markup: &Markup) -> Option<Range<usize>> {
    let first = lines[i].trim_start();
    if !first.starts_with("{{") {
        return None;
    }
    markup.wrapper_opening(template_names(first).first()?)?;
    let mut depth = 0usize;
    for (k, l) in lines.iter().enumerate().take(end).skip(i) {
        depth += l.matches("{{").count();
        depth = depth.saturating_sub(l.matches("}}").count());
        if depth == 0 {
            return l.trim_end().ends_with("}}").then_some(i..k + 1);
        }
    }
    None
}

/// Whether a line is a wrapper's closing call and nothing else.
fn wrapper_close(line: &str, markup: &Markup) -> bool {
    let t = line.trim();
    let calls = calls_in_line(t);
    calls.len() == 1
        && calls[0].start == 0
        && calls[0].end == t.len()
        && markup.is_wrapper_template(&calls[0].name)
        && markup.wrapper_opening(&calls[0].name).is_none()
}

/// Splits a template call's inner text at its top-level pipes.
fn top_level_parts(inner: &str) -> Vec<&str> {
    let (mut parts, mut start, mut depth) = (Vec::new(), 0usize, 0usize);
    let b = inner.as_bytes();
    let mut k = 0;
    while k < b.len() {
        match (b[k], b.get(k + 1)) {
            (b'{', Some(b'{')) | (b'[', Some(b'[')) => {
                depth += 1;
                k += 2;
            }
            (b'}', Some(b'}')) | (b']', Some(b']')) => {
                depth = depth.saturating_sub(1);
                k += 2;
            }
            (b'|', _) if depth == 0 => {
                parts.push(&inner[start..k]);
                start = k + 1;
                k += 1;
            }
            _ => k += 1,
        }
    }
    parts.push(&inner[start..]);
    parts
}

/// A wrapper's statement (0090 §3): its `result`, `reason`, `text`, `1` or `title` parameter, or
/// its first positional one; empty when it has none.
pub(crate) fn wrapper_statement(call: &str) -> String {
    let inner = call.trim().trim_start_matches("{{");
    let inner = inner.strip_suffix("}}").unwrap_or(inner);
    let mut named: Vec<(String, &str)> = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    for part in top_level_parts(inner).into_iter().skip(1) {
        let depthless = part.split("{{").next().unwrap_or(part);
        match depthless.find('=') {
            Some(eq) => named.push((
                part[..eq].trim().to_ascii_lowercase(),
                part[eq + 1..].trim(),
            )),
            None => positional.push(part.trim()),
        }
    }
    ["result", "reason", "text", "1", "title"]
        .iter()
        .find_map(|key| {
            named
                .iter()
                .find(|(k, v)| k == key && !v.is_empty())
                .map(|(_, v)| (*v).to_string())
        })
        .or_else(|| {
            positional
                .iter()
                .find(|p| !p.is_empty())
                .map(|p| (*p).to_string())
        })
        .unwrap_or_default()
}

/// A wrapper opened on its own lines and not yet closed (0090 §3).
#[derive(Debug, Clone)]
struct OpenWrapper {
    open: Range<usize>,
    /// The heading level whose scope it was opened in; 2 for the thread itself.
    level: u8,
    /// The heading post of that scope.
    parent: Option<usize>,
    /// The number of posts when it opened: the first one it encloses.
    first_post: usize,
    /// Whether it opened before anything else in the thread.
    at_start: bool,
    /// Whether unsigned text before it was made a post when it opened.
    flushed: bool,
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
    /// Wrappers opened on their own lines and not yet closed, innermost last.
    wrappers: Vec<OpenWrapper>,
    /// A collapsing post that enclosed the thread from its start: a closure, if nothing follows.
    closing: Option<usize>,
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
        self.close_scopes(level);
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

    /// Opens a wrapper. Unsigned text before it becomes a post of its own, unless the wrapper
    /// turns out to be inside one comment.
    fn open_wrapper(&mut self, open: Range<usize>) {
        let before = self.posts.len();
        self.flush_unsigned();
        let flushed = self.posts.len() > before;
        self.wrappers.push(OpenWrapper {
            level: self.stack.last().map_or(2, |(l, _)| *l),
            parent: self.heading_parent(),
            first_post: self.posts.len(),
            at_start: self.posts.is_empty() && self.stack.is_empty(),
            flushed,
            open,
        });
    }

    /// Closes the innermost wrapper, at line `close` or, when it was never closed, where its
    /// scope ends. A wrapper that enclosed no complete post was inside one comment: its lines,
    /// and the unsigned text before it, go back into that comment's text.
    fn close_wrapper(&mut self, close: Option<usize>) {
        let Some(w) = self.wrappers.pop() else { return };
        if self.posts.len() == w.first_post {
            if w.flushed
                && let Some(before) = self.posts.pop()
            {
                self.pending.extend(before.lines);
            }
            self.pending.extend(w.open.clone().chain(close));
            self.pending.sort_unstable();
            return;
        }
        self.flush_unsigned();
        let lines = self.lines;
        let call = lines[w.open.clone()].join("\n");
        let close_text = close
            .map(|i| lines[i].trim().to_string())
            .unwrap_or_default();
        let mut post = new_post(
            PostKind::Collapse {
                first: w.first_post,
                last: self.posts.len() - 1,
                wrapper: [call.clone(), close_text],
            },
            lines,
            w.open.clone(),
            0,
        );
        post.text = wrapper_statement(&call);
        post.signature = w
            .open
            .clone()
            .flat_map(|i| signatures(lines[i], self.markup, self.site))
            .find(|s| s.kind == SignatureKind::Dated);
        post.parent = w.parent;
        if w.at_start {
            self.closing = Some(self.posts.len());
        }
        self.posts.push(post);
    }

    /// Closes the wrappers of scopes a heading of this level ends.
    fn close_scopes(&mut self, level: u8) {
        while self.wrappers.last().is_some_and(|w| w.level >= level) {
            self.close_wrapper(None);
        }
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
        wrappers: Vec::new(),
        closing: None,
    };
    let mut i = span.body.start;
    while i < span.body.end {
        if let Some(h) = subs.iter().find(|h| h.line == i) {
            let Role::Sub(level) = h.role else {
                unreachable!()
            };
            walk.heading_post(i, level, &h.text);
            i += 1;
            continue;
        }
        if walk.extending.is_none() && !walk.cons.comment {
            if let Some(open) = wrapper_open(lines, i, span.body.end, markup) {
                i = open.end;
                walk.open_wrapper(open);
                continue;
            }
            if !walk.wrappers.is_empty() && wrapper_close(lines[i], markup) {
                walk.close_wrapper(Some(i));
                i += 1;
                continue;
            }
        }
        walk.line(i);
        i += 1;
    }
    walk.flush_unsigned();
    walk.close_scopes(0);
    walk.flush_unsigned();
    // A collapse that enclosed the whole thread, with nothing after it, is a closure (0090 §3).
    if let Some(k) = walk.closing.filter(|k| *k + 1 == walk.posts.len()) {
        let post = &mut walk.posts[k];
        if let PostKind::Collapse { wrapper, .. } = &post.kind {
            post.kind = PostKind::Closing {
                wrapper: wrapper.clone(),
            };
        }
        post.parent = None;
    }
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
