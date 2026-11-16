//! HTML helpers: escaping, character references, attribute parsing, MediaWiki's attribute
//! allow-list, inline-style checks and section anchors (11 §1.2).
//!
//! The allow-list and the checks follow MediaWiki's documented Sanitizer behaviour: which
//! attributes each tag keeps, that `style` loses anything that could fetch or run code,
//! and how ids are written. `ammonia` then sanitizes the serialized output again
//! ([`crate::render`]), so a gap here is not a hole.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

/// Escapes text for an HTML text node.
pub fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// Escapes text for a double-quoted attribute value.
pub fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn entities() -> &'static HashMap<&'static str, char> {
    static MAP: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m: HashMap<&'static str, char> =
            parse_wiki_text_2::HTML_ENTITIES.iter().copied().collect();
        // `&apos;` is HTML5 and XML, which MediaWiki accepts.
        m.insert("apos", '\'');
        m
    })
}

/// The character a reference `&...;` stands for, given the text between `&` and `;`.
fn reference(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let n = if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse::<u32>().ok()?
        };
        // MediaWiki refuses the references HTML says are invalid.
        return match n {
            0 | 0xD800..=0xDFFF => None,
            n => char::from_u32(n),
        };
    }
    entities().get(name).copied()
}

/// Decodes character references; anything that is not a valid one is kept as written.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let end = after
            .char_indices()
            .take(33)
            .find(|&(_, c)| c == ';')
            .map(|(i, _)| i);
        if let Some((e, c)) = end.and_then(|e| reference(&after[..e]).map(|c| (e, c))) {
            out.push(c);
            rest = &after[e + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Parses the attribute part of a tag (`class="a" style=b`), as MediaWiki reads it:
/// names lowercased, values double-, single- or un-quoted, references decoded, a later
/// duplicate replacing an earlier one.
#[allow(clippy::many_single_char_names)] // a byte scanner: positions and bytes
pub fn parse_attributes(s: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    let is_name = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b':' | b'_' | b'-' | b'.');
    while i < b.len() {
        if !is_name(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && is_name(b[i]) {
            i += 1;
        }
        let name = s[start..i].to_ascii_lowercase();
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let value = if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                let vs = j + 1;
                if let Some(p) = b[vs..].iter().position(|&c| c == q) {
                    i = vs + p + 1;
                    Some(&s[vs..vs + p])
                } else {
                    // An unclosed quote: MediaWiki drops the attribute.
                    i = b.len();
                    None
                }
            } else {
                let vs = j;
                while j < b.len() && !b[j].is_ascii_whitespace() {
                    j += 1;
                }
                i = j;
                Some(&s[vs..j])
            }
        } else {
            // A bare name: MediaWiki gives it an empty value.
            Some("")
        };
        if let Some(v) = value {
            let v = decode_entities(v);
            if let Some(slot) = out.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = v;
            } else {
                out.push((name, v));
            }
        }
    }
    out
}

const COMMON: &[&str] = &[
    "id",
    "class",
    "style",
    "lang",
    "dir",
    "title",
    "role",
    "aria-describedby",
    "aria-flowto",
    "aria-hidden",
    "aria-label",
    "aria-labelledby",
    "aria-level",
    "aria-owns",
    "about",
    "property",
    "resource",
    "datatype",
    "typeof",
    "itemid",
    "itemprop",
    "itemref",
    "itemscope",
    "itemtype",
];
const TABLE_CELL: &[&str] = &[
    "abbr", "axis", "headers", "scope", "rowspan", "colspan", "nowrap", "width", "height",
    "bgcolor", "align", "valign",
];

/// The attributes `tag` keeps beyond the common set, or `None` if the tag is not allowed.
fn tag_attributes(tag: &str) -> Option<&'static [&'static str]> {
    Some(match tag {
        "div" | "center" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "caption" => &["align"],
        "span" | "b" | "i" | "u" | "s" | "strike" | "small" | "big" | "sub" | "sup" | "em"
        | "strong" | "cite" | "dfn" | "kbd" | "code" | "samp" | "var" | "abbr" | "tt" | "mark"
        | "bdi" | "bdo" | "rb" | "rp" | "rt" | "rtc" | "ruby" | "wbr" | "dl" | "dt" | "dd" => &[],
        "br" => &["clear"],
        "blockquote" | "q" => &["cite"],
        "del" | "ins" => &["cite", "datetime"],
        "time" => &["datetime"],
        "data" => &["value"],
        "pre" | "hr" => &["width"],
        "ul" => &["type"],
        "ol" => &["type", "start", "reversed"],
        "li" => &["type", "value"],
        "table" => &[
            "summary",
            "width",
            "border",
            "frame",
            "rules",
            "cellspacing",
            "cellpadding",
            "align",
            "bgcolor",
        ],
        "thead" | "tfoot" | "tbody" => &["align", "valign"],
        "tr" => &["bgcolor", "align", "valign"],
        "td" | "th" => TABLE_CELL,
        "font" => &["size", "color", "face"],
        _ => return None,
    })
}

/// Whether `tag` is an HTML tag wikitext may write.
pub fn tag_allowed(tag: &str) -> bool {
    tag_attributes(tag).is_some()
}

/// Keeps the attributes MediaWiki keeps on `tag`, with `style` checked and `id` written as
/// an id. `br` keeps only a few (`id`, `class`, `title`, `style`, `clear`), as there.
pub fn sanitize_attributes(tag: &str, attrs: Vec<(String, String)>) -> Vec<(String, String)> {
    let Some(extra) = tag_attributes(tag) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, value) in attrs {
        let allowed = if tag == "br" {
            matches!(name.as_str(), "id" | "class" | "title" | "style" | "clear")
        } else {
            COMMON.contains(&name.as_str())
                || extra.contains(&name.as_str())
                || data_attribute(&name)
        };
        if !allowed {
            continue;
        }
        let value = match name.as_str() {
            "style" => check_css(&value),
            "id" => escape_id(&value),
            _ => value,
        };
        out.push((name, value));
    }
    out
}

/// `data-*` attributes, except the ones MediaWiki reserves for its own use.
fn data_attribute(name: &str) -> bool {
    name.strip_prefix("data-").is_some_and(|rest| {
        !rest.is_empty()
            && !rest.starts_with("ooui")
            && !rest.starts_with("mw")
            && !rest.starts_with("parsoid")
            && rest
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    })
}

/// What a refused inline style becomes, as in MediaWiki.
pub const INSECURE_CSS: &str = "/* insecure input */";

/// Checks an inline `style` value. References and CSS escapes are decoded and comments
/// removed; anything that could load a resource or run code (`url(`, `image(`,
/// `image-set(`, `expression`, `behavior`, `-moz-binding`, `-o-link`, `attr(… url)`, and
/// the legacy `filter:` and `accelerator:`) replaces the whole value with
/// [`INSECURE_CSS`]. Otherwise the decoded value is kept. The full TemplateStyles policy is
/// `scatter-css`'s (0055 §2); this is the attribute check alone.
pub fn check_css(value: &str) -> String {
    let decoded = decode_css_escapes(&decode_entities(value));
    let Some(stripped) = strip_css_comments(&decoded) else {
        return INSECURE_CSS.to_string();
    };
    let lower = stripped.to_lowercase();
    let squeezed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    let bad_functions = ["url(", "image(", "image-set(", "expression(", "expression"];
    let bad_properties = [
        "filter:",
        "accelerator:",
        "-o-link:",
        "-o-link-source:",
        "-o-replace:",
        "behavior:",
        "-moz-binding",
    ];
    if bad_functions.iter().any(|f| squeezed.contains(f))
        || bad_properties.iter().any(|p| squeezed.contains(p))
        || (squeezed.contains("attr(") && squeezed.contains("url"))
    {
        return INSECURE_CSS.to_string();
    }
    stripped.trim().to_string()
}

/// Decodes `\XX` hex and `\c` escapes.
fn decode_css_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
            hex.push(chars.next().unwrap_or('0'));
        }
        if hex.is_empty() {
            match chars.next() {
                // A backslash before a newline is a line continuation.
                Some('\n') | None => {}
                Some(c) => out.push(c),
            }
        } else {
            if chars.peek().is_some_and(char::is_ascii_whitespace) {
                chars.next();
            }
            let n = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
            out.push(
                char::from_u32(n)
                    .filter(|&c| c != '\0')
                    .unwrap_or('\u{FFFD}'),
            );
        }
    }
    out
}

/// Removes `/* … */` comments; `None` for an unterminated one, which MediaWiki treats as
/// insecure.
fn strip_css_comments(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let end = rest[start + 2..].find("*/")?;
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    Some(out)
}

/// An `id` attribute value as MediaWiki writes it in HTML5 mode: whitespace runs become
/// one underscore and the ends are trimmed.
pub fn escape_id(s: &str) -> String {
    let normalized: Vec<&str> = s.split_whitespace().collect();
    normalized.join("_")
}

/// The legacy form of an anchor MediaWiki also writes, as a separate empty `<span id>`,
/// when it differs: every byte outside `[A-Za-z0-9_.:-]` as `.XX`.
pub fn legacy_id(s: &str) -> String {
    let id = escape_id(s);
    let mut out = String::with_capacity(id.len());
    for &byte in id.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, ".{byte:02X}");
        }
    }
    out
}

/// Percent-encodes a title or fragment for a URL path, as MediaWiki's `wfUrlencode` does:
/// spaces become underscores first, and `;@$!*(),/~:` stay readable.
pub fn url_encode_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &byte in s.replace(' ', "_").as_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_'
                    | b'.'
                    | b';'
                    | b'@'
                    | b'$'
                    | b'!'
                    | b'*'
                    | b'('
                    | b')'
                    | b','
                    | b'/'
                    | b'~'
                    | b':'
            )
        {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references() {
        assert_eq!(
            decode_entities("a&amp;b&#x41;&#66;&nbsp;&bogus;&"),
            "a&bAB\u{a0}&bogus;&"
        );
        assert_eq!(decode_entities("&Agrave;&apos;"), "À'");
    }

    #[test]
    fn attributes() {
        let a = parse_attributes(r#"class="a b" STYLE='x:y' width=50% nowrap id="&quot;q""#);
        assert_eq!(
            a,
            [
                ("class".into(), "a b".into()),
                ("style".into(), "x:y".into()),
                ("width".into(), "50%".into()),
                ("nowrap".into(), String::new()),
                ("id".into(), "\"q".into()),
            ]
        );
        assert_eq!(
            parse_attributes(r#"a="1" a="2""#),
            [("a".into(), "2".into())]
        );
        assert!(parse_attributes(r#"a="unclosed"#).is_empty());
    }

    #[test]
    fn allow_list() {
        let a = parse_attributes(r#"onclick="x" style="color:red" align=left data-x=1 data-mw=2"#);
        assert_eq!(
            sanitize_attributes("div", a),
            [
                ("style".into(), "color:red".into()),
                ("align".into(), "left".into()),
                ("data-x".into(), "1".into()),
            ]
        );
        assert!(sanitize_attributes("script", vec![("id".into(), "x".into())]).is_empty());
    }

    #[test]
    fn css() {
        assert_eq!(check_css("color: red"), "color: red");
        assert_eq!(
            check_css("background: linear-gradient(to right, #fff, #000)"),
            "background: linear-gradient(to right, #fff, #000)"
        );
        for bad in [
            "background: url(x.png)",
            "background: u\\72l(x)",
            "width: expression(alert(1))",
            "background: image-set('a' 1x)",
            "behavior: url(x.htc)",
            "color: red /* open",
            "background: &#117;rl(x)",
            "content: attr(title url)",
        ] {
            assert_eq!(check_css(bad), INSECURE_CSS, "{bad}");
        }
    }

    #[test]
    fn ids() {
        assert_eq!(
            escape_id(" Welcome to  Librarybase! "),
            "Welcome_to_Librarybase!"
        );
        assert_eq!(
            legacy_id("Welcome to Librarybase!"),
            "Welcome_to_Librarybase.21"
        );
        assert_eq!(legacy_id("Café: x"), "Caf.C3.A9:_x");
        assert_eq!(
            url_encode_title("User talk:A&B/c?d"),
            "User_talk:A%26B/c%3Fd"
        );
    }
}
