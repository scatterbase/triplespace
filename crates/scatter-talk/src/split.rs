//! The splitter: sections become threads, comments become posts (0090 §1–§4).

use std::ops::Range;

use crate::registry::Markup;
use crate::sig::{Signature, signatures};
use crate::site::Site;
use crate::time::Stamp;

/// Options for one conversion.
#[derive(Debug, Clone)]
pub struct Options {
    /// A page larger than this, in bytes, falls back to one post (0090 §4).
    pub max_bytes: usize,
    /// DiscussionTools' timestamp format switch, `$wgDiscussionToolsTimestampFormatSwitchTime`
    /// read in the wiki's local time and expressed in UTC (0090 §9).
    pub dt_switch: Stamp,
    /// A heading's linkable ID. The default strips links, emphasis, templates and tags and
    /// escapes the rest; a caller with the renderer passes the rendered anchor (0090 §9).
    pub anchor: fn(&str) -> String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_bytes: 8 << 20,
            dt_switch: Stamp::from_civil(2022, 7, 12, 0, 0).expect("a valid date"),
            anchor: crate::dt::default_anchor,
        }
    }
}

/// Why a section or a page was not split into comments (0090 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// The section holds no signature the splitter recognizes.
    NoSignature,
    /// A comment boundary falls inside a construct that opens before it and closes after it.
    Construct(&'static str),
    /// A registered wrapper encloses only part of the section.
    PartialWrapper,
    /// The page is larger than [`Options::max_bytes`].
    TooLarge,
}

impl Fallback {
    /// The reason as `imported_from.fallback` records it.
    pub fn reason(self) -> &'static str {
        match self {
            Fallback::NoSignature => "no-signature",
            Fallback::Construct(what) => what,
            Fallback::PartialWrapper => "partial-wrapper",
            Fallback::TooLarge => "too-large",
        }
    }
}

/// What a post is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostKind {
    /// A signed comment.
    Comment,
    /// Text with no signature of its own: trailing text, or text history credits to someone
    /// other than the comment's signer (0090 §6).
    Unsigned,
    /// A heading below level two (0090 §3). Its text is the heading's wikitext.
    Heading {
        /// 3 to 6.
        level: u8,
    },
    /// A lifted wrapper (0090 §3): its text is the opening call.
    Wrapper {
        /// The closing call.
        close: String,
    },
    /// A whole section that would not split (0090 §4).
    Section(Fallback),
    /// A whole page that would not divide (0090 §4).
    Page(Fallback),
}

/// The DiscussionTools name and ID of a comment or heading (0090 §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtIds {
    /// `c-{author}-{timestamp}` or `h-{author}-{timestamp}`, the same on every page.
    pub name: String,
    /// Unique within the page.
    pub id: String,
    /// The legacy ID, where it differs.
    pub legacy_id: Option<String>,
}

/// One post of a thread.
#[derive(Debug, Clone)]
pub struct Post {
    /// What it is.
    pub kind: PostKind,
    /// Its wikitext, with its own indentation removed (0090 §1).
    pub text: String,
    /// The lines of the page it came from.
    pub lines: Range<usize>,
    /// The indentation of its first line.
    pub depth: usize,
    /// The post it answers, as an index into the thread's posts.
    pub parent: Option<usize>,
    /// Its signature: for a comment, the one that ended it; for a wrapper, its closer's.
    pub signature: Option<Signature>,
    /// Later signatures on its signature line: notes inside it.
    pub notes: Vec<Signature>,
    /// Its DiscussionTools name and ID, for a dated comment or a heading.
    pub dt: Option<DtIds>,
    /// Who wrote it, once [`crate::attribute`] has run.
    pub attribution: Option<crate::blame::Attribution>,
}

/// Where a thread came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadOrigin {
    /// A level-two section (or a level-one heading read as one).
    Section,
    /// Signed comments above the first heading: "*{title}*: before the first heading".
    BeforeFirstHeading,
    /// The whole page, as one post (0090 §4).
    Page,
}

/// One thread.
#[derive(Debug, Clone)]
pub struct Thread {
    /// Where it came from.
    pub origin: ThreadOrigin,
    /// The heading's wikitext, the thread's subject; empty for the other origins.
    pub subject: String,
    /// The heading's line, for a section.
    pub heading_line: Option<usize>,
    /// The posts, in page order; the first is the thread's `create`.
    pub posts: Vec<Post>,
    /// Set when the section would not split and its one post holds it whole.
    pub fallback: Option<Fallback>,
    /// The heading's DiscussionTools name and ID.
    pub dt: Option<DtIds>,
    /// The anchor of a level-one heading above it that was dropped, which DiscussionTools still
    /// counts as its parent.
    pub(crate) dropped_parent_anchor: Option<String>,
}

/// The text above the first heading that stays the pinned, open header thread (0090 §1).
#[derive(Debug, Clone)]
pub struct Header {
    /// Its wikitext.
    pub text: String,
    /// Its lines.
    pub lines: Range<usize>,
}

/// A converted talk page.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// The pinned header, when there is one.
    pub header: Option<Header>,
    /// The threads, in page order.
    pub threads: Vec<Thread>,
    /// Set when the page would not divide and its one thread holds it whole.
    pub fallback: Option<Fallback>,
}

impl Conversion {
    /// Every post, with its thread's index.
    pub fn posts(&self) -> impl Iterator<Item = (usize, &Post)> {
        self.threads
            .iter()
            .enumerate()
            .flat_map(|(t, th)| th.posts.iter().map(move |p| (t, p)))
    }
}

/// A heading line: (level, wikitext).
pub(crate) fn heading(line: &str) -> Option<(u8, String)> {
    let mut s = line.trim_end();
    while let Some(stripped) = s.strip_suffix("-->") {
        let start = stripped.rfind("<!--")?;
        s = stripped[..start].trim_end();
    }
    let lead = s.bytes().take_while(|b| *b == b'=').count();
    let trail = s.bytes().rev().take_while(|b| *b == b'=').count();
    if lead == 0 || trail == 0 || lead + trail >= s.len() {
        return None;
    }
    let level = lead.min(trail).min(6);
    let text = s[level..s.len() - level].trim();
    if text.is_empty() {
        return None;
    }
    Some((u8::try_from(level).ok()?, text.to_string()))
}

pub(crate) fn line_indent(line: &str) -> usize {
    line.bytes()
        .take_while(|b| matches!(b, b':' | b'*' | b'#'))
        .count()
}

/// Removes up to `depth` indentation characters from the start of each line (0090 §1).
pub(crate) fn dedent(lines: &[&str], depth: usize) -> String {
    let mut out = String::new();
    for (k, line) in lines.iter().enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let n = line_indent(line).min(depth);
        out.push_str(line[n..].trim_start_matches(|c| c == ' ' && n > 0));
    }
    out
}

/// Trims blank lines from both ends of a range.
pub(crate) fn trim_lines(lines: &[&str], r: Range<usize>) -> Range<usize> {
    let mut start = r.start;
    let mut end = r.end;
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    start..end
}

/// Splits a talk page's wikitext (0090 §1–§4).
pub fn convert(text: &str, markup: &Markup, site: &Site, opts: &Options) -> Conversion {
    let lines: Vec<&str> = text.split('\n').collect();
    if text.len() > opts.max_bytes {
        return page_fallback(&lines, Fallback::TooLarge);
    }
    let headings = crate::split_headings::plan(&lines);
    let first = headings
        .iter()
        .find(|h| h.role != crate::split_headings::Role::Dropped)
        .map_or(lines.len(), |h| h.line);
    let first_any = headings.first().map_or(lines.len(), |h| h.line);
    let mut conv = Conversion {
        header: None,
        threads: Vec::new(),
        fallback: None,
    };
    front_matter(&lines, 0..first_any.min(first), markup, site, &mut conv);
    for span in crate::split_headings::thread_spans(&headings, lines.len()) {
        let mut thread = crate::split_body::split_thread(&lines, &span, &headings, markup, site);
        thread.dropped_parent_anchor = span
            .dropped_parent
            .map(|l| (opts.anchor)(&headings[l].text));
        conv.threads.push(thread);
    }
    crate::dt::assign(&mut conv, &lines, &headings, opts);
    conv
}

fn page_fallback(lines: &[&str], why: Fallback) -> Conversion {
    let post = Post {
        kind: PostKind::Page(why),
        text: lines.join("\n"),
        lines: 0..lines.len(),
        depth: 0,
        parent: None,
        signature: None,
        notes: Vec::new(),
        dt: None,
        attribution: None,
    };
    Conversion {
        header: None,
        threads: vec![Thread {
            origin: ThreadOrigin::Page,
            subject: String::new(),
            heading_line: None,
            posts: vec![post],
            fallback: Some(why),
            dt: None,
            dropped_parent_anchor: None,
        }],
        fallback: Some(why),
    }
}

/// Whether a front-matter line is part of the banners: blank, a template or a table, a
/// comment, a behaviour switch or a category.
fn banner_like(line: &str) -> bool {
    let t = line.trim();
    t.is_empty()
        || t.starts_with("{{")
        || t.starts_with("}}")
        || t.starts_with('|')
        || t.starts_with("{|")
        || t.starts_with('!')
        || t.starts_with("<!--")
        || (t.starts_with("__") && t.ends_with("__"))
        || t.starts_with("[[Category:")
}

/// The text above the first heading: the pinned header, and, where signed comments follow the
/// banners, a thread of them (0090 §1).
fn front_matter(
    lines: &[&str],
    r: Range<usize>,
    markup: &Markup,
    site: &Site,
    conv: &mut Conversion,
) {
    let r = trim_lines(lines, r);
    if r.is_empty() {
        return;
    }
    // Where the banners end: the first line outside a template, a table or a comment that is
    // not itself banner-like.
    let mut depth = crate::split_body::Constructs::default();
    let mut split_at = r.end;
    for i in r.clone() {
        if !depth.is_open() && !banner_like(lines[i]) {
            split_at = i;
            break;
        }
        depth.feed(lines[i]);
    }
    let rest = split_at..r.end;
    let signed = rest
        .clone()
        .any(|i| !signatures(lines[i], markup, site).is_empty());
    let header_end = if signed { split_at } else { r.end };
    let header = trim_lines(lines, r.start..header_end);
    if !header.is_empty() {
        conv.header = Some(Header {
            text: lines[header.clone()].join("\n"),
            lines: header,
        });
    }
    if signed {
        let span = crate::split_headings::Span {
            heading: None,
            body: rest,
            dropped_parent: None,
        };
        let mut thread = crate::split_body::split_thread(lines, &span, &[], markup, site);
        thread.origin = ThreadOrigin::BeforeFirstHeading;
        conv.threads.push(thread);
    }
}

pub(crate) fn new_post(kind: PostKind, lines: &[&str], r: Range<usize>, depth: usize) -> Post {
    let text = dedent(&lines[r.clone()], depth);
    Post {
        kind,
        text,
        lines: r,
        depth,
        parent: None,
        signature: None,
        notes: Vec::new(),
        dt: None,
        attribution: None,
    }
}
