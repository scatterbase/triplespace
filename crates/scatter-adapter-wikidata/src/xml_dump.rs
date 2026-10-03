//! A MediaWiki XML dump (export format 0.10/0.11), streamed one `<page>` at a time with
//! its revisions. A `--current` dump has one revision per page; a history dump has many,
//! all returned in order.

use std::io::BufRead;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::DumpError;

/// One revision of a page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// A minor edit.
    pub minor: bool,
    /// The edit summary.
    pub comment: Option<String>,
    /// The content model (`wikibase-item`, `wikitext`, …).
    pub model: Option<String>,
    /// The content format.
    pub format: Option<String>,
    /// The content.
    pub text: String,
    /// The dump's SHA-1 of the content.
    pub sha1: Option<String>,
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
    /// The site's namespaces, from `<siteinfo>`: number → canonical name.
    namespaces: Vec<(i64, String)>,
    done: bool,
}

impl XmlDump<Box<dyn BufRead + Send>> {
    /// Opens a dump file, decompressing by extension.
    pub fn open(path: &Path) -> Result<Self, DumpError> {
        Ok(Self::new(crate::open(path)?))
    }
}

fn xml(e: impl std::fmt::Display) -> DumpError {
    DumpError::Xml(e.to_string())
}

impl<R: BufRead> XmlDump<R> {
    /// Over a reader.
    pub fn new(reader: R) -> Self {
        let mut r = Reader::from_reader(reader);
        r.config_mut().trim_text(false);
        Self {
            reader: r,
            buf: Vec::new(),
            namespaces: Vec::new(),
            done: false,
        }
    }

    /// The namespaces `<siteinfo>` declared, once it has been read.
    #[must_use]
    pub fn namespaces(&self) -> &[(i64, String)] {
        &self.namespaces
    }

    /// Reads the text up to the matching end tag of the element just opened.
    fn text_of(&mut self, end: &[u8]) -> Result<String, DumpError> {
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
                                return Err(DumpError::Xml(format!(
                                    "unknown entity reference `&{other};`"
                                )));
                            }
                        });
                    }
                }
                Event::CData(c) => out
                    .push_str(std::str::from_utf8(&c).map_err(|e| DumpError::Xml(e.to_string()))?),
                Event::End(e) if e.name().as_ref() == end => return Ok(out),
                Event::Eof => {
                    return Err(DumpError::Xml(format!(
                        "unexpected end of file inside <{}>",
                        String::from_utf8_lossy(end)
                    )));
                }
                _ => {}
            }
        }
    }

    fn read_contributor(&mut self, rev: &mut Revision) -> Result<(), DumpError> {
        let mut id = None;
        let mut name = None;
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"id" => id = self.text_of(b"id")?.trim().parse().ok(),
                    b"username" => name = Some(self.text_of(b"username")?),
                    b"ip" => rev.ip = Some(self.text_of(b"ip")?),
                    other => {
                        let other = other.to_vec();
                        self.text_of(&other)?;
                    }
                },
                Event::End(e) if e.name().as_ref() == b"contributor" => break,
                Event::Eof => {
                    return Err(DumpError::Xml(
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

    fn read_revision(&mut self) -> Result<Revision, DumpError> {
        let mut rev = Revision::default();
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"id" => rev.id = self.text_of(b"id")?.trim().parse().unwrap_or(0),
                    b"parentid" => rev.parent_id = self.text_of(b"parentid")?.trim().parse().ok(),
                    b"timestamp" => rev.timestamp = self.text_of(b"timestamp")?.trim().to_string(),
                    b"contributor" => {
                        if e.try_get_attribute("deleted").map_err(xml)?.is_some() {
                            rev.user_hidden = true;
                        }
                        self.read_contributor(&mut rev)?;
                    }
                    b"comment" => rev.comment = Some(self.text_of(b"comment")?),
                    b"model" => rev.model = Some(self.text_of(b"model")?.trim().to_string()),
                    b"format" => rev.format = Some(self.text_of(b"format")?.trim().to_string()),
                    b"text" => rev.text = self.text_of(b"text")?,
                    b"sha1" => rev.sha1 = Some(self.text_of(b"sha1")?.trim().to_string()),
                    other => {
                        let other = other.to_vec();
                        self.text_of(&other)?;
                    }
                },
                Event::Empty(e) => match e.name().as_ref() {
                    b"minor" => rev.minor = true,
                    b"contributor" if e.try_get_attribute("deleted").map_err(xml)?.is_some() => {
                        rev.user_hidden = true;
                    }
                    _ => {}
                },
                Event::End(e) if e.name().as_ref() == b"revision" => return Ok(rev),
                Event::Eof => {
                    return Err(DumpError::Xml(
                        "unexpected end of file inside <revision>".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn read_page(&mut self) -> Result<DumpPage, DumpError> {
        let mut page = DumpPage::default();
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"title" => page.title = self.text_of(b"title")?,
                    b"ns" => page.ns = self.text_of(b"ns")?.trim().parse().unwrap_or(0),
                    b"id" => page.id = self.text_of(b"id")?.trim().parse().unwrap_or(0),
                    b"revision" => page.revisions.push(self.read_revision()?),
                    b"redirect" => {
                        page.redirect = e
                            .try_get_attribute("title")
                            .map_err(xml)?
                            .map(|a| {
                                a.normalized_value(quick_xml::XmlVersion::default())
                                    .map(std::borrow::Cow::into_owned)
                            })
                            .transpose()
                            .map_err(xml)?;
                        self.text_of(b"redirect")?;
                    }
                    other => {
                        let other = other.to_vec();
                        self.text_of(&other)?;
                    }
                },
                Event::Empty(e) if e.name().as_ref() == b"redirect" => {
                    page.redirect = e
                        .try_get_attribute("title")
                        .map_err(xml)?
                        .map(|a| {
                            a.normalized_value(quick_xml::XmlVersion::default())
                                .map(std::borrow::Cow::into_owned)
                        })
                        .transpose()
                        .map_err(xml)?;
                }
                Event::End(e) if e.name().as_ref() == b"page" => return Ok(page),
                Event::Eof => {
                    return Err(DumpError::Xml(
                        "unexpected end of file inside <page>".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn read_siteinfo(&mut self) -> Result<(), DumpError> {
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf).map_err(xml)? {
                Event::Start(e) if e.name().as_ref() == b"namespace" => {
                    let key: i64 = e
                        .try_get_attribute("key")
                        .map_err(xml)?
                        .and_then(|a| {
                            a.normalized_value(quick_xml::XmlVersion::default())
                                .ok()
                                .and_then(|v| v.parse().ok())
                        })
                        .unwrap_or(0);
                    let name = self.text_of(b"namespace")?;
                    self.namespaces.push((key, name));
                }
                Event::Empty(e) if e.name().as_ref() == b"namespace" => {
                    let key: i64 = e
                        .try_get_attribute("key")
                        .map_err(xml)?
                        .and_then(|a| {
                            a.normalized_value(quick_xml::XmlVersion::default())
                                .ok()
                                .and_then(|v| v.parse().ok())
                        })
                        .unwrap_or(0);
                    self.namespaces.push((key, String::new()));
                }
                Event::End(e) if e.name().as_ref() == b"siteinfo" => return Ok(()),
                Event::Eof => {
                    return Err(DumpError::Xml(
                        "unexpected end of file inside <siteinfo>".into(),
                    ));
                }
                _ => {}
            }
        }
    }
}

impl<R: BufRead> Iterator for XmlDump<R> {
    type Item = Result<DumpPage, DumpError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
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
                Ok(Event::Eof) => {
                    self.done = true;
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
      <model>wikibase-item</model>
      <format>application/json</format>
      <text bytes="120" xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six &amp; more"}},"claims":{}}</text>
      <sha1>abc</sha1>
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
      <text xml:space="preserve">Hello</text>
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
      <text xml:space="preserve">{"entity":"Q6","redirect":"Q7"}</text>
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
        let q6 = &pages[0];
        assert_eq!((q6.title.as_str(), q6.ns, q6.id), ("Item:Q6", 120, 12));
        assert_eq!(q6.revisions.len(), 2);
        let latest = q6.latest().unwrap();
        assert_eq!(latest.id, 41877);
        assert_eq!(latest.parent_id, Some(41000));
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
        assert!(pages[1].latest().unwrap().user_hidden);
        assert_eq!(pages[2].latest().unwrap().ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(pages[3].redirect.as_deref(), Some("Item:Q6"));
        assert!(
            XmlDump::new("<mediawiki><page><title>x".as_bytes())
                .next()
                .unwrap()
                .is_err()
        );
    }
}
