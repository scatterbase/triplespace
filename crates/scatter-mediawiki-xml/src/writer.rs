//! Writing a dump in export schema 0.11, as MediaWiki 1.43's `XmlDumpWriter` writes one
//! (0047 §8.1; 21 §4.2): the root element and `<siteinfo>` first, then each page with its
//! revisions, then the closing tag. A hidden or erased part is written as MediaWiki
//! writes revision-deleted content: `<text bytes="…" deleted="deleted" />`,
//! `<comment deleted="deleted" />`, `<contributor deleted="deleted" />`, with `<sha1 />`
//! where the text is hidden. A revision's further slots are `<content>` elements.
//!
//! Text is escaped as `htmlspecialchars(…, ENT_NOQUOTES)` escapes it (`&`, `<`, `>`),
//! attribute values with `"` as well, and characters XML 1.0 forbids are replaced by
//! U+FFFD, as `Xml::elementClean` cleans them. The output reads back through
//! [`crate::reader`] to the same pages.

use std::io::Write;

use crate::reader::{DumpPage, Revision, SiteInfo, Slot};
use crate::{SCHEMA_NAMESPACE, SCHEMA_VERSION, XmlError, sha1_base36};

/// A dump being written, page by page.
pub struct Writer<W: Write> {
    out: W,
    opened: bool,
    closed: bool,
}

/// Text content, escaped.
fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

/// An attribute value, escaped.
fn attr(s: &str) -> String {
    text(s).replace('"', "&quot;")
}

impl<W: Write> Writer<W> {
    /// Over a sink.
    pub fn new(out: W) -> Self {
        Self {
            out,
            opened: false,
            closed: false,
        }
    }

    /// Writes the root element and `<siteinfo>`. Called once, before any page; a page
    /// written first opens the dump with an empty `<siteinfo>`.
    ///
    /// # Errors
    ///
    /// A write failure, or a dump already opened.
    pub fn start(&mut self, site: &SiteInfo) -> Result<(), XmlError> {
        if self.opened {
            return Err(XmlError::Malformed("the dump is already open".into()));
        }
        self.opened = true;
        let o = &mut self.out;
        writeln!(
            o,
            "<mediawiki xmlns=\"{SCHEMA_NAMESPACE}\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"{SCHEMA_NAMESPACE} http://www.mediawiki.org/xml/export-{SCHEMA_VERSION}.xsd\" version=\"{SCHEMA_VERSION}\" xml:lang=\"{}\">",
            attr(&site.lang)
        )?;
        writeln!(o, "  <siteinfo>")?;
        writeln!(o, "    <sitename>{}</sitename>", text(&site.sitename))?;
        writeln!(o, "    <dbname>{}</dbname>", text(&site.dbname))?;
        writeln!(o, "    <base>{}</base>", text(&site.base))?;
        writeln!(o, "    <generator>{}</generator>", text(&site.generator))?;
        writeln!(o, "    <case>{}</case>", text(&site.case))?;
        writeln!(o, "    <namespaces>")?;
        for ns in &site.namespaces {
            if ns.name.is_empty() {
                writeln!(
                    o,
                    "      <namespace key=\"{}\" case=\"{}\" />",
                    ns.key,
                    attr(&ns.case)
                )?;
            } else {
                writeln!(
                    o,
                    "      <namespace key=\"{}\" case=\"{}\">{}</namespace>",
                    ns.key,
                    attr(&ns.case),
                    text(&ns.name)
                )?;
            }
        }
        writeln!(o, "    </namespaces>")?;
        writeln!(o, "  </siteinfo>")?;
        Ok(())
    }

    /// Writes an XML comment between pages: what the export has to say about itself,
    /// such as that `templates` added nothing (0047 A33).
    ///
    /// # Errors
    ///
    /// A write failure.
    pub fn comment(&mut self, note: &str) -> Result<(), XmlError> {
        self.ensure_open()?;
        writeln!(self.out, "  <!-- {} -->", note.replace("--", "- -"))?;
        Ok(())
    }

    fn ensure_open(&mut self) -> Result<(), XmlError> {
        if self.closed {
            return Err(XmlError::Malformed("the dump is closed".into()));
        }
        if !self.opened {
            self.start(&SiteInfo::default())?;
        }
        Ok(())
    }

    /// Writes a whole page with its revisions.
    ///
    /// # Errors
    ///
    /// A write failure.
    pub fn page(&mut self, page: &DumpPage) -> Result<(), XmlError> {
        self.open_page(page)?;
        for r in &page.revisions {
            self.revision(r)?;
        }
        self.close_page()
    }

    /// Opens a page: its title, namespace, ID and redirect. Its revisions follow through
    /// [`Self::revision`], then [`Self::close_page`].
    ///
    /// # Errors
    ///
    /// A write failure.
    pub fn open_page(&mut self, page: &DumpPage) -> Result<(), XmlError> {
        self.ensure_open()?;
        let o = &mut self.out;
        writeln!(o, "  <page>")?;
        writeln!(o, "    <title>{}</title>", text(&page.title))?;
        writeln!(o, "    <ns>{}</ns>", page.ns)?;
        writeln!(o, "    <id>{}</id>", page.id)?;
        if let Some(target) = &page.redirect {
            writeln!(o, "    <redirect title=\"{}\" />", attr(target))?;
        }
        Ok(())
    }

    /// Writes one revision of the open page.
    ///
    /// # Errors
    ///
    /// A write failure.
    pub fn revision(&mut self, r: &Revision) -> Result<(), XmlError> {
        let o = &mut self.out;
        writeln!(o, "    <revision>")?;
        writeln!(o, "      <id>{}</id>", r.id)?;
        if let Some(p) = r.parent_id {
            writeln!(o, "      <parentid>{p}</parentid>")?;
        }
        writeln!(o, "      <timestamp>{}</timestamp>", text(&r.timestamp))?;
        if r.user_hidden {
            writeln!(o, "      <contributor deleted=\"deleted\" />")?;
        } else if let Some((id, name)) = &r.user {
            writeln!(o, "      <contributor>")?;
            writeln!(o, "        <username>{}</username>", text(name))?;
            writeln!(o, "        <id>{id}</id>")?;
            writeln!(o, "      </contributor>")?;
        } else if let Some(ip) = &r.ip {
            writeln!(o, "      <contributor>")?;
            writeln!(o, "        <ip>{}</ip>", text(ip))?;
            writeln!(o, "      </contributor>")?;
        } else {
            writeln!(o, "      <contributor deleted=\"deleted\" />")?;
        }
        if r.minor {
            writeln!(o, "      <minor />")?;
        }
        if r.comment_hidden {
            writeln!(o, "      <comment deleted=\"deleted\" />")?;
        } else if let Some(c) = &r.comment {
            writeln!(o, "      <comment>{}</comment>", text(c))?;
        }
        if let Some(origin) = r.origin {
            writeln!(o, "      <origin>{origin}</origin>")?;
        }
        writeln!(
            o,
            "      <model>{}</model>",
            text(r.model.as_deref().unwrap_or("wikitext"))
        )?;
        writeln!(
            o,
            "      <format>{}</format>",
            text(r.format.as_deref().unwrap_or("text/x-wiki"))
        )?;
        Self::text_element(o, "      ", &r.text, r.text_hidden, r.sha1.as_deref())?;
        for slot in &r.slots {
            Self::slot(o, slot)?;
        }
        writeln!(o, "    </revision>")?;
        Ok(())
    }

    /// `<text>` and `<sha1>` of a slot.
    fn text_element(
        o: &mut W,
        indent: &str,
        content: &str,
        hidden: bool,
        sha1: Option<&str>,
    ) -> Result<(), XmlError> {
        if hidden {
            writeln!(
                o,
                "{indent}<text bytes=\"{}\" deleted=\"deleted\" />",
                content.len()
            )?;
            writeln!(o, "{indent}<sha1 />")?;
            return Ok(());
        }
        let sha1 = sha1.map_or_else(|| sha1_base36(content), str::to_string);
        writeln!(
            o,
            "{indent}<text bytes=\"{}\" sha1=\"{sha1}\" xml:space=\"preserve\">{}</text>",
            content.len(),
            text(content)
        )?;
        writeln!(o, "{indent}<sha1>{sha1}</sha1>")?;
        Ok(())
    }

    /// A further slot as a `<content>` element.
    fn slot(o: &mut W, slot: &Slot) -> Result<(), XmlError> {
        writeln!(o, "      <content>")?;
        writeln!(o, "        <role>{}</role>", text(&slot.role))?;
        if let Some(origin) = slot.origin {
            writeln!(o, "        <origin>{origin}</origin>")?;
        }
        writeln!(
            o,
            "        <model>{}</model>",
            text(slot.model.as_deref().unwrap_or("wikitext"))
        )?;
        writeln!(
            o,
            "        <format>{}</format>",
            text(slot.format.as_deref().unwrap_or("text/x-wiki"))
        )?;
        Self::text_element(
            o,
            "        ",
            &slot.text,
            slot.text_hidden,
            slot.sha1.as_deref(),
        )?;
        writeln!(o, "      </content>")?;
        Ok(())
    }

    /// Closes the open page.
    ///
    /// # Errors
    ///
    /// A write failure.
    pub fn close_page(&mut self) -> Result<(), XmlError> {
        writeln!(self.out, "  </page>")?;
        Ok(())
    }

    /// Writes the closing tag and hands the sink back.
    ///
    /// # Errors
    ///
    /// A write failure.
    pub fn finish(mut self) -> Result<W, XmlError> {
        self.ensure_open()?;
        self.closed = true;
        writeln!(self.out, "</mediawiki>")?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{SiteNamespace, XmlDump};

    fn site() -> SiteInfo {
        SiteInfo {
            lang: "en".into(),
            sitename: "Librarybase".into(),
            dbname: "librarybase".into(),
            base: "https://librarybase.org/wiki/Project:Home".into(),
            generator: "Triplespace 0.0.1".into(),
            case: "first-letter".into(),
            namespaces: vec![
                SiteNamespace {
                    key: 0,
                    case: "first-letter".into(),
                    name: String::new(),
                },
                SiteNamespace {
                    key: 4,
                    case: "first-letter".into(),
                    name: "Librarybase".into(),
                },
            ],
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn writes_mediawikis_shape_and_reads_it_back() {
        let page = DumpPage {
            title: "Librarybase:Home".into(),
            ns: 4,
            id: 489,
            redirect: None,
            revisions: vec![
                Revision {
                    id: 10,
                    parent_id: None,
                    timestamp: "2023-05-04T05:00:02Z".into(),
                    user: Some((3, "Harej".into())),
                    comment: Some("Created <page> & \"more\"".into()),
                    origin: Some(10),
                    model: Some("wikitext".into()),
                    format: Some("text/x-wiki".into()),
                    text: "Hello <b>&amp;</b>\n\tworld".into(),
                    ..Revision::default()
                },
                Revision {
                    id: 11,
                    parent_id: Some(10),
                    timestamp: "2023-05-05T05:00:02Z".into(),
                    ip: Some("127.0.0.1".into()),
                    minor: true,
                    comment_hidden: true,
                    text_hidden: true,
                    text: "hidden".into(),
                    model: Some("wikitext".into()),
                    format: Some("text/x-wiki".into()),
                    ..Revision::default()
                },
                Revision {
                    id: 12,
                    parent_id: Some(11),
                    timestamp: "2023-05-06T05:00:02Z".into(),
                    user_hidden: true,
                    text: "x".into(),
                    model: Some("wikitext".into()),
                    format: Some("text/x-wiki".into()),
                    slots: vec![Slot {
                        role: "notes".into(),
                        origin: Some(12),
                        model: Some("json".into()),
                        format: Some("application/json".into()),
                        text: "{}".into(),
                        ..Slot::default()
                    }],
                    ..Revision::default()
                },
            ],
        };
        let mut w = Writer::new(Vec::new());
        w.start(&site()).unwrap();
        w.comment("templates: none added -- see 0047 A33").unwrap();
        w.page(&page).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert!(out.starts_with(
            "<mediawiki xmlns=\"http://www.mediawiki.org/xml/export-0.11/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.mediawiki.org/xml/export-0.11/ http://www.mediawiki.org/xml/export-0.11.xsd\" version=\"0.11\" xml:lang=\"en\">\n  <siteinfo>\n    <sitename>Librarybase</sitename>\n"
        ));
        assert!(out.contains("      <namespace key=\"0\" case=\"first-letter\" />\n"));
        assert!(out.contains(
            "      <namespace key=\"4\" case=\"first-letter\">Librarybase</namespace>\n"
        ));
        assert!(out.contains("  <!-- templates: none added - - see 0047 A33 -->\n"));
        assert!(out.contains("      <comment>Created &lt;page&gt; &amp; \"more\"</comment>\n"));
        assert!(out.contains(&format!(
            "      <text bytes=\"25\" sha1=\"{0}\" xml:space=\"preserve\">Hello &lt;b&gt;&amp;amp;&lt;/b&gt;\n\tworld</text>\n      <sha1>{0}</sha1>\n",
            sha1_base36(&page.revisions[0].text)
        )));
        assert!(out.contains("      <text bytes=\"6\" deleted=\"deleted\" />\n      <sha1 />\n"));
        assert!(out.contains("      <comment deleted=\"deleted\" />\n"));
        assert!(out.contains("      <contributor deleted=\"deleted\" />\n"));
        assert!(out.contains("        <ip>127.0.0.1</ip>\n"));
        assert!(out.contains("      <minor />\n"));
        assert!(out.contains("      <content>\n        <role>notes</role>\n"));
        assert!(out.ends_with("  </page>\n</mediawiki>\n"));

        let mut dump = XmlDump::new(out.as_bytes());
        let read: Vec<DumpPage> = dump.by_ref().map(Result::unwrap).collect();
        assert_eq!(dump.siteinfo(), &site());
        assert_eq!(read.len(), 1);
        let back = &read[0];
        assert_eq!(back.title, page.title);
        assert_eq!(back.revisions.len(), 3);
        let first = &back.revisions[0];
        assert_eq!(first.text, page.revisions[0].text);
        assert_eq!(first.comment, page.revisions[0].comment);
        assert_eq!(
            first.sha1.as_deref(),
            Some(sha1_base36(&first.text).as_str())
        );
        let second = &back.revisions[1];
        assert!(second.text_hidden && second.comment_hidden && !second.user_hidden);
        assert_eq!(second.text, "", "hidden text is not carried");
        assert_eq!(second.ip.as_deref(), Some("127.0.0.1"));
        assert!(second.minor);
        let third = &back.revisions[2];
        assert!(third.user_hidden);
        assert_eq!(
            third.slots,
            page.revisions[2]
                .slots
                .iter()
                .map(|s| Slot {
                    sha1: Some(sha1_base36(&s.text)),
                    ..s.clone()
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn forbidden_characters_are_replaced() {
        assert_eq!(text("a\u{1}b\tc"), "a\u{FFFD}b\tc");
        assert_eq!(attr("\"<&>\""), "&quot;&lt;&amp;&gt;&quot;");
    }

    #[test]
    fn a_page_before_start_opens_the_dump() {
        let mut w = Writer::new(Vec::new());
        w.page(&DumpPage::default()).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert!(out.starts_with("<mediawiki "));
        assert!(out.contains("<siteinfo>"));
        let mut w = Writer::new(Vec::new());
        w.start(&SiteInfo::default()).unwrap();
        assert!(w.start(&SiteInfo::default()).is_err());
    }
}
