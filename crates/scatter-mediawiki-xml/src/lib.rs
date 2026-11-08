//! MediaWiki's export XML, schema 0.11, read and written for any MediaWiki, with or
//! without Wikibase (0005 A119; 22 §2.1): the home of MediaWiki functionality that is not
//! particular to Wikibase.
//!
//! - [`reader`]: a dump (`dumpBackup.php`, `Special:Export`, `action=query&export`)
//!   streamed one `<page>` at a time with its revisions, each revision with its content
//!   model, format, text, the parts the dump hid (`deleted="deleted"`), and the further
//!   slots a 0.11 dump carries as `<content>` elements. Moved here from
//!   `scatter-adapter-wikibase`'s `xml_dump.rs`, which reads through it still.
//! - [`writer`]: the same shape written, page by page, as MediaWiki 1.43's
//!   `XmlDumpWriter` writes it: `<siteinfo>` first, then each page's revisions, a hidden or
//!   erased part as `<text deleted="deleted"/>`, `<comment deleted="deleted"/>` or
//!   `<contributor deleted="deleted"/>`. What `Special:Export` writes (0047 §8.1; 21 §4.2).
//!
//! The crate is pure (22 §1.2 rule 2): it reads from any `BufRead` and writes to any
//! `Write`, opens no file and knows no network.

#![forbid(unsafe_code)]

pub mod reader;
pub mod writer;

pub use reader::{DumpPage, Revision, SiteInfo, SiteNamespace, Slot, XmlDump};
pub use writer::Writer;

/// The export schema version written and read.
pub const SCHEMA_VERSION: &str = "0.11";
/// The XML namespace of the schema.
pub const SCHEMA_NAMESPACE: &str = "http://www.mediawiki.org/xml/export-0.11/";

/// Why a dump could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum XmlError {
    /// The XML is not a MediaWiki dump, or is cut short.
    #[error("xml: {0}")]
    Malformed(String),
    /// Reading or writing failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// MediaWiki's `sha1` of a revision's text: the SHA-1 digest in base 36, left-padded with
/// zeros to 31 characters, as `Wikimedia\base_convert` writes it (`rev_sha1`).
#[must_use]
pub fn sha1_base36(text: &str) -> String {
    use sha1::{Digest as _, Sha1};
    let digest = Sha1::digest(text.as_bytes());
    base36(&digest)
}

/// A big-endian byte string as a base-36 number, 31 digits wide.
fn base36(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut number: Vec<u8> = bytes.to_vec();
    let mut out = Vec::with_capacity(31);
    while number.iter().any(|&b| b != 0) {
        let mut carry = 0u32;
        for b in &mut number {
            let value = (carry << 8) | u32::from(*b);
            *b = u8::try_from(value / 36).unwrap_or(0);
            carry = value % 36;
        }
        out.push(DIGITS[carry as usize]);
    }
    while out.len() < 31 {
        out.push(b'0');
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_as_mediawiki_writes_it() {
        // Librarybase's `User:Database import`, revision 1096 (docs/samples/librarybase).
        assert_eq!(
            sha1_base36(
                "Edits attributed to this account have been imported using BatchIngestion."
            ),
            "jvd0zndg8rmpjno1ro5g8a78mno8uu3"
        );
        assert_eq!(sha1_base36("").len(), 31);
        assert_eq!(base36(&[0; 20]), "0".repeat(31));
    }
}
