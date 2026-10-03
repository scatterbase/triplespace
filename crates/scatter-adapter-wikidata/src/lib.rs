//! The Wikibase adapter (0002 §8.4; 0035 §3): reading a Wikibase's dumps and turning them
//! into what the ingester writes.
//!
//! - [`json_dump`]: the entity JSON dump `dumpJson.php` writes (one entity per line in a
//!   JSON array; Wikidata's `latest-all.json.gz`), streamed, gzip or bzip2 compressed.
//! - [`xml_dump`]: a MediaWiki XML dump (`dumpBackup.php`, Special:Export), streamed page
//!   by page with each page's revisions: the complete input for an adoption, since it
//!   carries the page ID, the revision ID and timestamp, the contributor and the entity
//!   JSON together.
//! - [`adoption`]: a dump's pages as adopted entities in home form (0035 §3), with the
//!   [`adoption::Survey`] that derives the sequence floors and the accounts from a pass
//!   over the dump.
//! - [`mirror`]: a dump's entities as mirror states under the provider's prefixed IDs.
//!
//! The crate reads files and parses; it knows no log and no database (0005 §2).

#![forbid(unsafe_code)]

pub mod adoption;
pub mod json_dump;
pub mod mirror;
pub mod xml_dump;

pub use adoption::{Account, AdoptedRevision, Survey, adopt_page};
pub use json_dump::JsonDump;
pub use xml_dump::{DumpPage, Revision, XmlDump};

use std::io::{BufRead, BufReader};
use std::path::Path;

/// Why a dump could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DumpError {
    /// Reading failed.
    #[error("read: {0}")]
    Io(#[from] std::io::Error),
    /// The XML is not a MediaWiki dump.
    #[error("xml: {0}")]
    Xml(String),
    /// A line is not an entity.
    #[error("line {line}: {message}")]
    Entity {
        /// 1-based.
        line: usize,
        /// What went wrong.
        message: String,
    },
    /// An ID could not be rewritten.
    #[error(transparent)]
    Adapter(#[from] scatter_wikibase_changeset::AdapterError),
}

/// Opens a dump for reading, decompressing by extension: `.gz` and `.bz2`, else plain.
pub fn open(path: &Path) -> Result<Box<dyn BufRead + Send>, DumpError> {
    let file = std::fs::File::open(path)?;
    let name = path.to_string_lossy();
    Ok(if name.ends_with(".gz") {
        Box::new(BufReader::new(flate2::read::MultiGzDecoder::new(file)))
    } else if name.ends_with(".bz2") {
        Box::new(BufReader::new(bzip2::read::MultiBzDecoder::new(file)))
    } else {
        Box::new(BufReader::new(file))
    })
}
