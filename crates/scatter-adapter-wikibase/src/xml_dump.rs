//! A MediaWiki XML dump (export format 0.10/0.11), streamed one `<page>` at a time with
//! its revisions: the reader lives in `scatter-mediawiki-xml` since 0005 A119, and this
//! module is the adapter's door to it, with [`open`], which decompresses a dump file by
//! its extension, which the pure crate cannot.

use std::io::BufRead;
use std::path::Path;

pub use scatter_mediawiki_xml::{DumpPage, Revision, SiteInfo, Slot, XmlDump, XmlError};

use crate::DumpError;

/// Opens a dump file, decompressing by extension.
///
/// # Errors
///
/// The file cannot be opened.
pub fn open(path: &Path) -> Result<XmlDump<Box<dyn BufRead + Send>>, DumpError> {
    Ok(XmlDump::new(crate::open(path)?))
}

impl From<XmlError> for DumpError {
    fn from(e: XmlError) -> Self {
        match e {
            XmlError::Io(e) => Self::Io(e),
            other => Self::Xml(other.to_string()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
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
      <text xml:space="preserve">{"entity":"Q7","redirect":"Q6"}</text>
    </revision>
  </page>
</mediawiki>
"#;

    #[test]
    fn the_reader_is_the_pure_crates() {
        let pages: Vec<super::DumpPage> = super::XmlDump::new(SAMPLE.as_bytes())
            .map(Result::unwrap)
            .collect();
        assert_eq!(pages.len(), 4);
        assert_eq!(pages[0].latest().unwrap().id, 41877);
    }
}
