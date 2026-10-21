//! DiscussionTools names and IDs for imported comments and headings (0090 §9), as
//! `CommentParser::computeName` and `computeId` derive them.

use std::collections::{BTreeMap, HashSet};

use scatter_wikitext::html::{decode_entities, escape_id};

use crate::split::{Conversion, DtIds, Options, PostKind, Thread};
use crate::split_headings::HeadingInfo;
use crate::time::Stamp;

/// A heading's linkable ID without a renderer: links read as their labels, and emphasis,
/// templates and tags removed, then escaped as MediaWiki escapes an anchor.
pub fn default_anchor(heading: &str) -> String {
    let mut out = String::with_capacity(heading.len());
    let mut rest = heading;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(close) = after.find("]]")
        {
            let inner = &after[..close];
            out.push_str(inner.rsplit('|').next().unwrap_or(inner));
            rest = &after[close + 2..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("{{")
            && let Some(close) = after.find("}}")
        {
            rest = &after[close + 2..];
            continue;
        }
        if rest.starts_with('<')
            && let Some(close) = rest.find('>')
        {
            rest = &rest[close + 1..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("''") {
            rest = after;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    escape_id(&decode_entities(&out))
}

/// `truncateForId`: at most 80 bytes, cut on a character boundary; underscores trimmed unless
/// the legacy form is wanted.
fn truncate(s: &str, legacy: bool) -> String {
    let mut end = s.len().min(80);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let t = &s[..end];
    if legacy {
        t.to_string()
    } else {
        t.trim_matches('_').to_string()
    }
}

/// `getTimestampString`: ISO with milliseconds before the switch, `YmdHis` from it on.
pub fn timestamp_string(when: Stamp, switch: Stamp) -> String {
    let (y, m, d, h, mi) = when.civil();
    if when < switch {
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:00.000Z")
    } else {
        format!("{y:04}{m:02}{d:02}{h:02}{mi:02}00")
    }
}

fn author_ts(user: &str, when: Stamp, switch: Stamp, legacy: bool) -> String {
    format!(
        "{}-{}",
        truncate(&user.replace(' ', "_"), legacy),
        timestamp_string(when, switch)
    )
}

/// Linkable IDs of every heading, by line, with MediaWiki's `_2`, `_3` for repeats.
fn anchors(headings: &[HeadingInfo], opts: &Options) -> BTreeMap<usize, String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for h in headings {
        let a = (opts.anchor)(&h.text);
        let n = seen.entry(a.clone()).or_insert(0);
        *n += 1;
        out.insert(h.line, if *n == 1 { a } else { format!("{a}_{n}") });
    }
    out
}

/// A post that is a dated comment for DiscussionTools: (user, time).
fn dated(t: &Thread, k: usize) -> Option<(&str, Stamp)> {
    let p = &t.posts[k];
    if !matches!(p.kind, PostKind::Comment | PostKind::Wrapper { .. }) {
        return None;
    }
    let s = p.signature.as_ref()?;
    Some((s.user.as_str(), s.when?))
}

/// The oldest dated comment among posts `range` of a thread.
fn oldest(t: &Thread, range: std::ops::Range<usize>) -> Option<(&str, Stamp)> {
    range.filter_map(|k| dated(t, k)).min_by_key(|(_, w)| *w)
}

struct Ids {
    taken: HashSet<String>,
    taken_legacy: HashSet<String>,
}

impl Ids {
    fn unique(set: &mut HashSet<String>, id: String) -> String {
        let id = if set.contains(&id) {
            // There are more numbers than items on a page, so this always finds one.
            (1..=set.len() + 1)
                .map(|n| format!("{id}-{n}"))
                .find(|c| !set.contains(c))
                .unwrap_or(id)
        } else {
            id
        };
        set.insert(id.clone());
        id
    }

    fn make(&mut self, name: String, id: &dyn Fn(bool) -> String) -> DtIds {
        let current = Self::unique(&mut self.taken, id(false));
        let legacy = Self::unique(&mut self.taken_legacy, id(true));
        DtIds {
            name,
            legacy_id: (legacy != current).then_some(legacy),
            id: current,
        }
    }
}

/// Assigns names and IDs in page order, headings and comments interleaved, as DiscussionTools
/// numbers collisions.
pub(crate) fn assign(
    conv: &mut Conversion,
    _lines: &[&str],
    headings: &[HeadingInfo],
    opts: &Options,
) {
    let anchors = anchors(headings, opts);
    let sw = opts.dt_switch;
    let mut ids = Ids {
        taken: HashSet::new(),
        taken_legacy: HashSet::new(),
    };
    for t in &mut conv.threads {
        let thread_anchor = t.heading_line.and_then(|l| anchors.get(&l)).cloned();
        if let Some(anchor) = &thread_anchor {
            let old = oldest(t, 0..t.posts.len()).map(|(u, w)| (u.to_string(), w));
            let parent = t.dropped_parent_anchor.clone();
            let name = old.as_ref().map_or("h-".into(), |(u, w)| {
                format!("h-{}", author_ts(u, *w, sw, false))
            });
            let id = |legacy: bool| {
                let mut id = format!("h-{}", truncate(anchor, legacy));
                if let Some(p) = &parent {
                    id.push('-');
                    id.push_str(&truncate(p, legacy));
                }
                if let Some((_, w)) = &old {
                    id.push('-');
                    id.push_str(&timestamp_string(*w, sw));
                }
                id
            };
            t.dt = Some(ids.make(name, &id));
        }
        for k in 0..t.posts.len() {
            let dt = post_ids(t, k, thread_anchor.as_deref(), &anchors, sw, &mut ids);
            t.posts[k].dt = dt;
        }
    }
}

/// The heading or comment a post's ID names as its parent: (anchor) or (user, time).
enum ParentRef {
    Heading(String),
    Comment(String, Stamp),
}

fn parent_ref(
    t: &Thread,
    k: usize,
    thread_anchor: Option<&str>,
    anchors: &BTreeMap<usize, String>,
) -> Option<ParentRef> {
    let mut p = t.posts[k].parent;
    while let Some(q) = p {
        if let PostKind::Heading { .. } = t.posts[q].kind {
            return anchors
                .get(&t.posts[q].lines.start)
                .map(|a| ParentRef::Heading(a.clone()));
        }
        if let Some((u, w)) = dated(t, q) {
            return Some(ParentRef::Comment(u.to_string(), w));
        }
        p = t.posts[q].parent;
    }
    thread_anchor.map(|a| ParentRef::Heading(a.to_string()))
}

fn post_ids(
    t: &Thread,
    k: usize,
    thread_anchor: Option<&str>,
    anchors: &BTreeMap<usize, String>,
    sw: Stamp,
    ids: &mut Ids,
) -> Option<DtIds> {
    let parent = parent_ref(t, k, thread_anchor, anchors);
    let suffix = |legacy: bool| match &parent {
        Some(ParentRef::Heading(a)) => format!("-{}", truncate(a, legacy)),
        Some(ParentRef::Comment(u, w)) => format!("-{}", author_ts(u, *w, sw, legacy)),
        None => String::new(),
    };
    if let PostKind::Heading { level } = t.posts[k].kind {
        let anchor = anchors.get(&t.posts[k].lines.start)?.clone();
        let end = (k + 1..t.posts.len())
            .find(|j| matches!(t.posts[*j].kind, PostKind::Heading { level: l } if l <= level))
            .unwrap_or(t.posts.len());
        let old = oldest(t, k + 1..end).map(|(u, w)| (u.to_string(), w));
        let name = old.as_ref().map_or("h-".into(), |(u, w)| {
            format!("h-{}", author_ts(u, *w, sw, false))
        });
        let id = |legacy: bool| {
            let mut id = format!("h-{}{}", truncate(&anchor, legacy), suffix(legacy));
            if let Some((_, w)) = &old {
                id.push('-');
                id.push_str(&timestamp_string(*w, sw));
            }
            id
        };
        return Some(ids.make(name, &id));
    }
    let (user, when) = dated(t, k)?;
    let name = format!("c-{}", author_ts(user, when, sw, false));
    let id = |legacy: bool| format!("c-{}{}", author_ts(user, when, sw, legacy), suffix(legacy));
    Some(ids.make(name, &id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_switch_format() {
        let sw = Stamp::from_civil(2022, 7, 12, 0, 0).unwrap();
        assert_eq!(
            timestamp_string(Stamp::from_civil(2006, 7, 10, 7, 50).unwrap(), sw),
            "2006-07-10T07:50:00.000Z"
        );
        assert_eq!(
            timestamp_string(Stamp::from_civil(2026, 10, 7, 11, 34).unwrap(), sw),
            "20261007113400"
        );
    }

    #[test]
    fn anchors_strip_markup() {
        assert_eq!(
            default_anchor("Proposal 1:  [[WP:BAN|Site-Ban]]"),
            "Proposal_1:_Site-Ban"
        );
        assert_eq!(
            default_anchor("''Youtube candidate'' {{tl|x}}"),
            "Youtube_candidate"
        );
    }
}
