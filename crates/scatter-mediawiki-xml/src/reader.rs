//! A MediaWiki XML dump (export schema 0.10 or 0.11), streamed one `<page>` at a time
//! with its revisions. A `--current` dump has one revision per page; a history dump has
//! many, all returned in order. The `<siteinfo>` header is kept for the reader to ask
//! for once a page has been read. A document that ends before its `</mediawiki>` is a
//! truncated one, and the reader says so with an error after the pages it could read.
//! About 0005 A119 and 0047 §8.1.

use std::io::BufRead;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::XmlError;

/// One namespace `<siteinfo>` declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiteNamespace {
    /// The number.
    pub key: i64,
    /// `first-letter` or `case-sensitive`.
    pub case: String,
    /// The local name; empty for the main namespace.
    pub name: String,
}

/// The dump's `<siteinfo>` and the `xml:lang` of its root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiteInfo {
    /// `xml:lang` of `<mediawiki>`: the wiki's content language.
    pub lang: String,
    /// `<sitename>`.
    pub sitename: String,
    /// `<dbname>`.
    pub dbname: String,
    /// `<base>`: the main page's URL.
    pub base: String,
    /// `<generator>`: `MediaWiki 1.43.9`.
    pub generator: String,
    /// `<case>`: `first-letter`.
    pub case: String,
    /// The namespaces, in dump order.
    pub namespaces: Vec<SiteNamespace>,
}

/// A further slot of a revision (schema 0.11's `<content>`): everything but the main
/// slot, which [`Revision`] carries itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slot {
    /// The slot role.
    pub role: String,
    /// The revision whose content this is (`<origin>`).
    pub origin: Option<u64>,
    /// The content model.
    pub model: Option<String>,
    /// The content format.
    pub format: Option<String>,
    /// The content.
    pub text: String,
    /// Whether the dump hid the content.
    pub text_hidden: bool,
    /// The dump's SHA-1 of the content.
    pub sha1: Option<String>,
}

/// One revision of a page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // one per flag the export writes: `<minor/>` and three `deleted`
pub struct Revision {
    /// The revision ID.
    pub id: u64,
    /// The parent revision ID.
    pub parent_id: Option<u64>,
    /// The timestamp, as the dump writes it (`2026-09-20T14:02:11Z`).
    pub timestamp: String,
    /// The contributor's user ID and name, for a registered user.
    pub user: Option<(u64, String)>,
    /// The contributor's IP, for an anonymous edit.
    pub ip: Option<String>,
    /// Whether the dump hid the contributor (`<contributor deleted="deleted"/>`).
    pub user_hidden: bool,
    /// Whether the dump hid the edit summary (`<comment deleted="deleted"/>`).
    pub comment_hidden: bool,
    /// Whether the dump hid the text (`<text deleted="deleted"/>`).
    pub text_hidden: bool,
    /// A minor edit.
    pub minor: bool,
    /// The edit summary.
    pub comment: Option<String>,
    /// The revision whose main-slot content this is (`<origin>`), where the dump says.
    pub origin: Option<u64>,
    /// The content model (`wikibase-item`, `wikitext`, …).
    pub model: Option<String>,
    /// The content format.
    pub format: Option<String>,
    /// The content of the main slot.
    pub text: String,
    /// The dump's SHA-1 of the content, base 36.
    pub sha1: Option<String>,
    /// The further slots, in dump order.
    pub slots: Vec<Slot>,
}

/// One page with its revisions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DumpPage {
    /// The full title, `Item:Q6`.
    pub title: String,
    /// The namespace number.
    pub ns: i64,
    /// The page ID.
    pub id: u64,
    /// The redirect target, when the page is one.
    pub redirect: Option<String>,
    /// The revisions, in dump order.
    pub revisions: Vec<Revision>,
}

impl DumpPage {
    /// The newest revision: the highest revision ID.
    #[must_use]
    pub fn latest(&self) -> Option<&Revision> {
        self.revisions.iter().max_by_key(|r| r.id)
    }
}

/// A streaming reader over an XML dump.
pub struct XmlDump<R: BufRead> {
    reader: Reader<R>,
    buf: Vec<u8>,
    siteinfo: SiteInfo,
    /// Whether `<mediawiki>` is open: the input may not end while it is.
    open: bool,
    done: bool,
}

fn xml(e: impl std::fmt::Display) -> XmlError {
    XmlError::Malformed(e.to_string())
}

fn attribute(e: &BytesStart<'_>, name: &str) -> Result<Option<String>, XmlError> {
    e.try_get_attribute(name)
        .map_err(xml)?
        .map(|a| {
            a.normalized_value(quick_xml::XmlVersion::default())
                .map(std::borrow::Cow::into_owned)
        })
        .transpose()
        .map_err(xml)
}

fn is_deleted(e: &BytesStart<'_>) -> Result<bool, XmlError> {
    Ok(e.try_get_attribute("deleted").map_err(xml)?.is_some())
}

impl<R: BufRead> XmlDump<R> {
    /// Over a reader.
    pub fn new(reader: R) -> Self {
        let mut r = Reader::from_reader(reader);
        r.config_mut().trim_text(false);
        Self {
            reader: r,
            buf: Vec::new(),
            siteinfo: SiteInfo::default(),
            open: false,
            done: false,
        }
    }

    /// The namespaces `<siteinfo>` declared, once it has been read: number → name.
    #[must_use]
    pub fn namespaces(&self) -> Vec<(i64, String)> {
        self.siteinfo
            .namespaces
            .iter()
            .map(|n| (n.key, n.name.clone()))
            .collect()
    }

    /// The `<siteinfo>` header, once it has been read (it comes before the first page).
    #[must_use]
    pub fn siteinfo(&self) -> &SiteInfo {
        &self.siteinfo
    }

    /// Reads the text up to the matching end tag of the element just opened.
    fn text_of(&mut self, end: &[u8]) -> Result<String, XmlError> {
        let mut out = String::new();
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Text(t) => out.push_str(&t.decode().map_err(xml)?),
                Event::GeneralRef(r) => {
                    if let Some(c) = r.resolve_char_ref().map_err(xml)? {
                        out.push(c);
                    } else {
                        let name = r.decode().map_err(xml)?;
                        out.push_str(match name.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            other => {
                                return Err(XmlError::Malformed(format!(
                                    "unknown entity reference `&{other};`"
                                )));
                            }
                        });
                    }
                }
                Event::CData(c) => out.push_str(std::str::from_utf8(&c).map_err(xml)?),
                Event::End(e) if e.name().as_ref() == end => return Ok(out),
                Event::Eof => {
                    return Err(XmlError::Malformed(format!(
                        "unexpected end of file inside <{}>",
                        String::from_utf8_lossy(end)
                    )));
                }
                _ => {}
            }
        }
    }

    fn number_of(&mut self, end: &[u8]) -> Result<Option<u64>, XmlError> {
        Ok(self.text_of(end)?.trim().parse().ok())
    }

    fn skip(&mut self, name: &[u8]) -> Result<(), XmlError> {
        let name = name.to_vec();
        self.text_of(&name).map(drop)
    }

    fn read_contributor(&mut self, rev: &mut Revision) -> Result<(), XmlError> {
        let mut id = None;
        let mut name = None;
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"id" => id = self.number_of(b"id")?,
                    b"username" => name = Some(self.text_of(b"username")?),
                    b"ip" => rev.ip = Some(self.text_of(b"ip")?),
                    other => {
                        let other = other.to_vec();
                        self.skip(&other)?;
                    }
                },
                Event::End(e) if e.name().as_ref() == b"contributor" => break,
                Event::Eof => {
                    return Err(XmlError::Malformed(
                        "unexpected end of file inside <contributor>".into(),
                    ));
                }
                _ => {}
            }
        }
        if let (Some(i), Some(n)) = (id, name) {
            rev.user = Some((i, n));
        }
        Ok(())
    }

    fn read_slot(&mut self) -> Result<Slot, XmlError> {
        let mut slot = Slot::default();
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"role" => slot.role = self.text_of(b"role")?.trim().to_string(),
                    b"origin" => slot.origin = self.number_of(b"origin")?,
                    b"model" => slot.model = Some(self.text_of(b"model")?.trim().to_string()),
                    b"format" => slot.format = Some(self.text_of(b"format")?.trim().to_string()),
                    b"text" => {
                        slot.text_hidden = is_deleted(&e)?;
                        slot.text = self.text_of(b"text")?;
                    }
                    b"sha1" => slot.sha1 = Some(self.text_of(b"sha1")?.trim().to_string()),
                    other => {
                        let other = other.to_vec();
                        self.skip(&other)?;
                    }
                },
                Event::Empty(e) if e.name().as_ref() == b"text" => {
                    slot.text_hidden = is_deleted(&e)?;
                }
                Event::End(e) if e.name().as_ref() == b"content" => return Ok(slot),
                Event::Eof => {
                    return Err(XmlError::Malformed(
                        "unexpected end of file inside <content>".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn read_revision(&mut self) -> Result<Revision, XmlError> {
        let mut rev = Revision::default();
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"id" => rev.id = self.number_of(b"id")?.unwrap_or(0),
                    b"parentid" => rev.parent_id = self.number_of(b"parentid")?,
                    b"timestamp" => rev.timestamp = self.text_of(b"timestamp")?.trim().to_string(),
                    b"contributor" => {
                        if is_deleted(&e)? {
                            rev.user_hidden = true;
                        }
                        self.read_contributor(&mut rev)?;
                    }
                    b"comment" => {
                        if is_deleted(&e)? {
                            rev.comment_hidden = true;
                        }
                        rev.comment = Some(self.text_of(b"comment")?);
                    }
                    b"origin" => rev.origin = self.number_of(b"origin")?,
                    b"model" => rev.model = Some(self.text_of(b"model")?.trim().to_string()),
                    b"format" => rev.format = Some(self.text_of(b"format")?.trim().to_string()),
                    b"text" => {
                        if is_deleted(&e)? {
                            rev.text_hidden = true;
                        }
                        rev.text = self.text_of(b"text")?;
                    }
                    b"sha1" => {
                        let s = self.text_of(b"sha1")?.trim().to_string();
                        rev.sha1 = (!s.is_empty()).then_some(s);
                    }
                    b"content" => rev.slots.push(self.read_slot()?),
                    other => {
                        let other = other.to_vec();
                        self.skip(&other)?;
                    }
                },
                Event::Empty(e) => match e.name().as_ref() {
                    b"minor" => rev.minor = true,
                    b"contributor" if is_deleted(&e)? => rev.user_hidden = true,
                    b"comment" if is_deleted(&e)? => rev.comment_hidden = true,
                    b"text" if is_deleted(&e)? => rev.text_hidden = true,
                    _ => {}
                },
                Event::End(e) if e.name().as_ref() == b"revision" => return Ok(rev),
                Event::Eof => {
                    return Err(XmlError::Malformed(
                        "unexpected end of file inside <revision>".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn read_page(&mut self) -> Result<DumpPage, XmlError> {
        let mut page = DumpPage::default();
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"title" => page.title = self.text_of(b"title")?,
                    b"ns" => page.ns = self.text_of(b"ns")?.trim().parse().unwrap_or(0),
                    b"id" => page.id = self.number_of(b"id")?.unwrap_or(0),
                    b"revision" => page.revisions.push(self.read_revision()?),
                    b"redirect" => {
                        page.redirect = attribute(&e, "title")?;
                        self.skip(b"redirect")?;
                    }
                    other => {
                        let other = other.to_vec();
                        self.skip(&other)?;
                    }
                },
                Event::Empty(e) if e.name().as_ref() == b"redirect" => {
                    page.redirect = attribute(&e, "title")?;
                }
                Event::End(e) if e.name().as_ref() == b"page" => return Ok(page),
                Event::Eof => {
                    return Err(XmlError::Malformed(
                        "unexpected end of file inside <page>".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn read_siteinfo(&mut self) -> Result<(), XmlError> {
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"sitename" => self.siteinfo.sitename = self.text_of(b"sitename")?,
                    b"dbname" => self.siteinfo.dbname = self.text_of(b"dbname")?,
                    b"base" => self.siteinfo.base = self.text_of(b"base")?,
                    b"generator" => self.siteinfo.generator = self.text_of(b"generator")?,
                    b"case" => self.siteinfo.case = self.text_of(b"case")?,
                    b"namespace" => {
                        let key = attribute(&e, "key")?.and_then(|k| k.parse().ok());
                        let case = attribute(&e, "case")?.unwrap_or_default();
                        let name = self.text_of(b"namespace")?;
                        self.siteinfo.namespaces.push(SiteNamespace {
                            key: key.unwrap_or(0),
                            case,
                            name,
                        });
                    }
                    _ => {}
                },
                Event::Empty(e) if e.name().as_ref() == b"namespace" => {
                    let key = attribute(&e, "key")?.and_then(|k| k.parse().ok());
                    let case = attribute(&e, "case")?.unwrap_or_default();
                    self.siteinfo.namespaces.push(SiteNamespace {
                        key: key.unwrap_or(0),
                        case,
                        name: String::new(),
                    });
                }
                Event::End(e) if e.name().as_ref() == b"siteinfo" => return Ok(()),
                Event::Eof => {
                    return Err(XmlError::Malformed(
                        "unexpected end of file inside <siteinfo>".into(),
                    ));
                }
                _ => {}
            }
        }
    }
}

impl<R: BufRead> Iterator for XmlDump<R> {
    type Item = Result<DumpPage, XmlError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"mediawiki" => {
                        self.open = true;
                        match attribute(&e, "xml:lang") {
                            Ok(lang) => self.siteinfo.lang = lang.unwrap_or_default(),
                            Err(e) => {
                                self.done = true;
                                return Some(Err(e));
                            }
                        }
                    }
                    b"siteinfo" => {
                        if let Err(e) = self.read_siteinfo() {
                            self.done = true;
                            return Some(Err(e));
                        }
                    }
                    b"page" => {
                        let page = self.read_page();
                        if page.is_err() {
                            self.done = true;
                        }
                        return Some(page);
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"mediawiki" => self.open = false,
                Ok(Event::Eof) => {
                    self.done = true;
                    // A document cut short, even between two pages, is not the document:
                    // whoever reads it must learn that pages are missing.
                    if self.open {
                        return Some(Err(XmlError::Malformed(
                            "unexpected end of file before </mediawiki>: the document is truncated"
                                .into(),
                        )));
                    }
                    return None;
                }
                Ok(_) => {}
                Err(e) => {
                    self.done = true;
                    return Some(Err(xml(e)));
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" version="0.11" xml:lang="en">
  <siteinfo>
    <sitename>Librarybase</sitename>
    <dbname>librarybasewiki</dbname>
    <base>https://librarybase.org/wiki/Project:Home</base>
    <generator>MediaWiki 1.43.9</generator>
    <case>first-letter</case>
    <namespaces>
      <namespace key="0" case="first-letter" />
      <namespace key="120" case="first-letter">Item</namespace>
      <namespace key="122" case="first-letter">Property</namespace>
    </namespaces>
  </siteinfo>
  <page>
    <title>Item:Q6</title>
    <ns>120</ns>
    <id>12</id>
    <revision>
      <id>41000</id>
      <timestamp>2026-09-01T00:00:00Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <model>wikibase-item</model>
      <format>application/json</format>
      <text bytes="2" xml:space="preserve">{}</text>
    </revision>
    <revision>
      <id>41877</id>
      <parentid>41000</parentid>
      <timestamp>2026-09-20T14:02:11Z</timestamp>
      <contributor><username>Bob</username><id>9</id></contributor>
      <minor/>
      <comment>/* wbsetlabel-add:1|en */ Six</comment>
      <origin>41877</origin>
      <model>wikibase-item</model>
      <format>application/json</format>
      <text bytes="120" xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six &amp; more"}},"claims":{}}</text>
      <sha1>abc</sha1>
      <content>
        <role>notes</role>
        <origin>41877</origin>
        <model>wikitext</model>
        <format>text/x-wiki</format>
        <text bytes="5" xml:space="preserve">Hello</text>
        <sha1>def</sha1>
      </content>
    </revision>
  </page>
  <page>
    <title>Property:P12</title>
    <ns>122</ns>
    <id>40</id>
    <revision>
      <id>903</id>
      <timestamp>2024-01-03T09:15:40Z</timestamp>
      <contributor deleted="deleted" />
      <comment deleted="deleted" />
      <model>wikibase-property</model>
      <format>application/json</format>
      <text xml:space="preserve">{"type":"property","id":"P12","datatype":"wikibase-item","labels":{},"claims":{}}</text>
    </revision>
  </page>
  <page>
    <title>Main Page</title>
    <ns>0</ns>
    <id>1</id>
    <revision>
      <id>2</id>
      <timestamp>2020-01-01T00:00:00Z</timestamp>
      <contributor><ip>127.0.0.1</ip></contributor>
      <model>wikitext</model>
      <format>text/x-wiki</format>
      <text bytes="5" deleted="deleted" />
      <sha1 />
    </revision>
  </page>
  <page>
    <title>Item:Q7</title>
    <ns>120</ns>
    <id>13</id>
    <redirect title="Item:Q6" />
    <revision>
      <id>44000</id>
      <timestamp>2026-09-21T00:00:00Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <model>wikibase-item</model>
      <format>application/json</format>
      <text xml:space="preserve">{"entity":"Q7","redirect":"Q6"}</text>
    </revision>
  </page>
</mediawiki>
"#;

    #[test]
    fn reads_pages_and_revisions() {
        let mut dump = XmlDump::new(SAMPLE.as_bytes());
        let pages: Vec<DumpPage> = dump.by_ref().map(Result::unwrap).collect();
        assert_eq!(pages.len(), 4);
        assert_eq!(
            dump.namespaces().iter().find(|(k, _)| *k == 120).unwrap().1,
            "Item"
        );
        let site = dump.siteinfo();
        assert_eq!(site.lang, "en");
        assert_eq!(site.sitename, "Librarybase");
        assert_eq!(site.generator, "MediaWiki 1.43.9");
        assert_eq!(site.namespaces[0].name, "");
        let q6 = &pages[0];
        assert_eq!((q6.title.as_str(), q6.ns, q6.id), ("Item:Q6", 120, 12));
        assert_eq!(q6.revisions.len(), 2);
        let latest = q6.latest().unwrap();
        assert_eq!(latest.id, 41877);
        assert_eq!(latest.parent_id, Some(41000));
        assert_eq!(latest.origin, Some(41877));
        assert_eq!(latest.user, Some((9, "Bob".to_string())));
        assert!(latest.minor);
        assert_eq!(latest.model.as_deref(), Some("wikibase-item"));
        assert!(
            latest.text.contains(r#""value":"Six & more""#),
            "entities are unescaped"
        );
        assert_eq!(
            latest.comment.as_deref(),
            Some("/* wbsetlabel-add:1|en */ Six")
        );
        assert_eq!(latest.slots.len(), 1, "a 0.11 dump's further slot");
        assert_eq!(latest.slots[0].role, "notes");
        assert_eq!(latest.slots[0].text, "Hello");
        assert_eq!(latest.slots[0].sha1.as_deref(), Some("def"));
        let p12 = pages[1].latest().unwrap();
        assert!(p12.user_hidden && p12.comment_hidden && !p12.text_hidden);
        let main = pages[2].latest().unwrap();
        assert_eq!(main.ip.as_deref(), Some("127.0.0.1"));
        assert!(main.text_hidden);
        assert_eq!(main.sha1, None, "an empty <sha1 /> is none");
        assert_eq!(pages[3].redirect.as_deref(), Some("Item:Q6"));
        assert!(
            XmlDump::new("<mediawiki><page><title>x".as_bytes())
                .next()
                .unwrap()
                .is_err()
        );
    }

    // 0047 §8.1: an export cut short is refused, wherever the cut falls.
    #[test]
    fn a_document_without_its_closing_tag_is_an_error() {
        // Cut after the first `</page>`: the page reads, then the reader reports the cut.
        let cut = SAMPLE.find("</page>").unwrap() + "</page>".len();
        let mut dump = XmlDump::new(&SAMPLE.as_bytes()[..cut]);
        assert_eq!(dump.next().unwrap().unwrap().title, "Item:Q6");
        let e = dump.next().unwrap().unwrap_err();
        assert!(e.to_string().contains("</mediawiki>"), "{e}");
        assert!(dump.next().is_none(), "the error ends the iteration");
        // Cut before any page.
        let cut = SAMPLE.find("</siteinfo>").unwrap() + "</siteinfo>".len();
        assert!(
            XmlDump::new(&SAMPLE.as_bytes()[..cut])
                .next()
                .unwrap()
                .is_err()
        );
        // The whole document, and one with no pages, end without one.
        assert!(XmlDump::new(SAMPLE.as_bytes()).all(|p| p.is_ok()));
        assert_eq!(
            XmlDump::new("<mediawiki></mediawiki>".as_bytes()).count(),
            0
        );
        assert_eq!(XmlDump::new("<mediawiki/>".as_bytes()).count(), 0);
        assert_eq!(XmlDump::new("".as_bytes()).count(), 0);
    }
}
