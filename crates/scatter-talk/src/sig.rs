//! Signatures: a user link followed by a timestamp, an undated signature, or an unsigned
//! template (0090 §2).

use scatter_wikitext::html::decode_entities;

use crate::registry::{Markup, normalize_template_name};
use crate::site::{Site, same_name};
use crate::time::{Stamp, find_timestamps};

/// DiscussionTools' limit on the rendered characters between a signature's user link and its
/// timestamp.
pub const SIGNATURE_SCAN_LIMIT: usize = 100;

/// How a signature was recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureKind {
    /// A user link followed by a timestamp.
    Dated,
    /// A dash and a user link ending the line, the form before about 2005.
    Undated,
    /// An unsigned template naming the user, with or without a time.
    Unsigned,
}

/// A signature found in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The user the signature names, normalized as MediaWiki normalizes a username.
    pub user: String,
    /// Its time, when it has one.
    pub when: Option<Stamp>,
    /// How it was recognized.
    pub kind: SignatureKind,
}

/// A user link in a line: its byte span (in the decoded line) and the user it names.
struct UserLink {
    start: usize,
    end: usize,
    user: String,
}

/// A username as MediaWiki stores it: underscores as spaces, the first letter capitalized.
pub fn normalize_user(name: &str) -> String {
    let name = name.replace('_', " ");
    let name = name.trim();
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The user a link target names, when it is a user page, a user talk page or a contributions
/// page. Subpages do not count.
fn link_user(target: &str, site: &Site) -> Option<String> {
    let target = target.trim().trim_start_matches(':').trim();
    let (ns, rest) = target.split_once(':')?;
    let rest = rest.trim();
    if site
        .user
        .iter()
        .chain(&site.user_talk)
        .any(|n| same_name(n, ns))
    {
        let name = rest.split('#').next()?;
        if name.contains('/') || name.trim().is_empty() {
            return None;
        }
        return Some(normalize_user(name));
    }
    if site.special.iter().any(|n| same_name(n, ns)) {
        let (page, name) = rest.split_once('/')?;
        if site.contributions.iter().any(|n| same_name(n, page)) && !name.trim().is_empty() {
            return Some(normalize_user(name.split('#').next()?));
        }
    }
    None
}

/// Every user link in a line.
fn user_links(line: &str, site: &Site) -> Vec<UserLink> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = line[from..].find("[[") {
        let start = from + open;
        let Some(close) = line[start + 2..].find("]]") else {
            break;
        };
        let end = start + 2 + close + 2;
        let inner = &line[start + 2..end - 2];
        let target = inner.split('|').next().unwrap_or("");
        if let Some(user) = link_user(target, site) {
            out.push(UserLink { start, end, user });
        }
        from = end;
    }
    out
}

/// Roughly how many characters wikitext reads as once rendered: link targets, tags and quote
/// marks removed, character references decoded.
pub fn visible_len(s: &str) -> usize {
    let mut text = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(close) = after.find("]]")
        {
            let inner = &after[..close];
            text.push_str(inner.rsplit('|').next().unwrap_or(inner));
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
        text.push(c);
        rest = &rest[c.len_utf8()..];
    }
    decode_entities(&text).chars().count()
}

/// Strips HTML tags, for looking at the text before an undated signature.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        if let Some(gt) = rest[lt..].find('>') {
            rest = &rest[lt + gt + 1..];
        } else {
            rest = &rest[lt..];
            break;
        }
    }
    out.push_str(rest);
    out
}

/// A template call on one line: its normalized name and its positional parameters.
pub(crate) struct Call {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub params: Vec<String>,
}

/// The template calls that open and close on this line, outermost only.
pub(crate) fn calls_in_line(line: &str) -> Vec<Call> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'{' && b[i + 1] == b'{' && b.get(i + 2) != Some(&b'{') {
            let mut depth = 0usize;
            let mut j = i;
            let mut end = None;
            while j + 1 < b.len() {
                if b[j] == b'{' && b[j + 1] == b'{' {
                    depth += 1;
                    j += 2;
                } else if b[j] == b'}' && b[j + 1] == b'}' {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            let Some(end) = end else { break };
            let inner = &line[i + 2..end - 2];
            let mut parts = inner.split('|');
            let name = normalize_template_name(parts.next().unwrap_or(""));
            let params = parts
                .filter(|p| !p.contains('='))
                .map(|p| p.trim().to_string())
                .collect();
            out.push(Call {
                start: i,
                end,
                name,
                params,
            });
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// The signatures in one line, in order. The first ends a comment; later ones are notes inside
/// it (0090 §2). Character references are decoded first.
pub fn signatures(line: &str, markup: &Markup, site: &Site) -> Vec<Signature> {
    let line = decode_entities(line);
    let Some(months) = markup.months_for(&site.lang) else {
        return Vec::new();
    };
    let links = user_links(&line, site);
    let mut out = Vec::new();
    let mut floor = 0;
    for ts in find_timestamps(&line, &markup.formats, months, &site.zone) {
        let nearest = links
            .iter()
            .rev()
            .find(|l| l.start >= floor && l.end <= ts.start);
        if let Some(l) = nearest
            && visible_len(&line[l.end..ts.start]) <= SIGNATURE_SCAN_LIMIT
        {
            out.push(Signature {
                user: l.user.clone(),
                when: Some(ts.when),
                kind: SignatureKind::Dated,
            });
            floor = ts.end;
        }
    }
    if !out.is_empty() {
        return out;
    }
    if let Some(sig) = undated(&line, &links, markup) {
        return vec![sig];
    }
    unsigned_template(&line, markup, site).into_iter().collect()
}

fn undated(line: &str, links: &[UserLink], markup: &Markup) -> Option<Signature> {
    let last = links.last()?;
    if visible_len(&line[last.end..]) > 30 {
        return None;
    }
    let before = strip_tags(&line[..last.start]);
    let before = before.trim_end();
    let marked = markup
        .undated_marks
        .iter()
        .any(|m| before.ends_with(m.as_str()));
    let alone = markup.undated_alone && before.trim_start_matches([':', '*', '#', ' ']).is_empty();
    (marked || alone).then(|| Signature {
        user: last.user.clone(),
        when: None,
        kind: SignatureKind::Undated,
    })
}

fn unsigned_template(line: &str, markup: &Markup, site: &Site) -> Option<Signature> {
    let call = calls_in_line(line)
        .into_iter()
        .rev()
        .find(|c| markup.unsigned.contains(&c.name))?;
    let user = normalize_user(call.params.first()?);
    if user.is_empty() {
        return None;
    }
    let when = call.params.get(1).and_then(|p| {
        let months = markup.months_for(&site.lang)?;
        find_timestamps(p, &markup.formats, months, &site.zone)
            .first()
            .map(|f| f.when)
    });
    Some(Signature {
        user,
        when,
        kind: SignatureKind::Unsigned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sigs(line: &str) -> Vec<(String, Option<String>, SignatureKind)> {
        signatures(line, Markup::embedded(), &Site::english())
            .into_iter()
            .map(|s| (s.user, s.when.map(Stamp::iso), s.kind))
            .collect()
    }

    #[test]
    fn dated_signatures() {
        let s = sigs(
            ":Fine. [[User:Black Kite|Black Kite]] ([[User_talk:Black Kite|talk]]) 14:27, 16 September 2026 (UTC)",
        );
        assert_eq!(
            s,
            [(
                "Black Kite".into(),
                Some("2026-09-16T14:27:00Z".into()),
                SignatureKind::Dated
            )]
        );
        let temp = sigs(
            "x [[Special:Contributions/&#126;2025-34665-91|&#126;2025-34665-91]] 23:13, 10 December 2025 (UTC)",
        );
        assert_eq!(temp[0].0, "~2025-34665-91");
    }

    #[test]
    fn later_signatures_are_notes() {
        let s = sigs(
            "<s>Text [[Special:Contributions/1.2.3.4|1.2.3.4]] 12:33, 5 June 2025 (UTC)</s> <small>Note [[User:B|B]] 16:02, 7 June 2025 (UTC)</small>",
        );
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, "1.2.3.4");
        assert_eq!(s[1].0, "B");
    }

    #[test]
    fn a_link_far_from_the_timestamp_is_not_a_signature() {
        let filler = "word ".repeat(30);
        assert!(sigs(&format!("[[User:A|A]] {filler} 12:00, 5 June 2025 (UTC)")).is_empty());
    }

    #[test]
    fn undated_and_unsigned() {
        assert_eq!(
            sigs("I would change these. --[[User:LMS|LMS]]")[0].2,
            SignatureKind::Undated
        );
        assert_eq!(
            sigs(":[[User:Stirling Newberry|Stirling Newberry]]")[0].2,
            SignatureKind::Undated
        );
        assert!(sigs("as [[User:LMS|LMS]] said, the article is long and needs work").is_empty());
        let u = sigs("Hello {{unsigned|Example|12:00, 5 June 2025 (UTC)}}");
        assert_eq!(
            u,
            [(
                "Example".into(),
                Some("2025-06-05T12:00:00Z".into()),
                SignatureKind::Unsigned
            )]
        );
        assert!(sigs("see [[User:A/sandbox]] --[[User:A/sandbox|A]]").is_empty());
    }
}
