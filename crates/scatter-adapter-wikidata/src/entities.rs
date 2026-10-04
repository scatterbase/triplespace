//! Either dump as a stream of entities: the JSON dump's lines, or the XML dump's entity
//! pages (newest revision, redirects and other content models skipped), each with its
//! page metadata filled in from the dump — `lastrevid` and `modified` from the revision,
//! `pageid` from the page. The format is told from the first byte: `<` is XML.

use std::io::BufRead;
use std::path::Path;

use scatter_wikibase_model::entity::{Entity, PageInfo, ParsedEntity};

use crate::DumpError;
use crate::adoption::{ENTITY_MODELS, is_redirect_json};
use crate::json_dump::JsonDump;
use crate::xml_dump::XmlDump;

/// The entities of a dump in either format.
pub enum Entities<R: BufRead> {
    /// A JSON dump.
    Json(JsonDump<R>),
    /// An XML dump.
    Xml(XmlDump<R>),
}

impl Entities<Box<dyn BufRead + Send>> {
    /// Opens a dump by path, decompressing by extension and telling the format by the
    /// first byte.
    pub fn open(path: &Path) -> Result<Self, DumpError> {
        let mut reader = crate::open(path)?;
        let first = reader.fill_buf()?.first().copied();
        Ok(if first == Some(b'<') {
            Self::Xml(XmlDump::new(reader))
        } else {
            Self::Json(JsonDump::new(reader))
        })
    }
}

impl<R: BufRead> Iterator for Entities<R> {
    type Item = Result<ParsedEntity, DumpError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Json(d) => d.next(),
            Self::Xml(d) => loop {
                let page = match d.next()? {
                    Ok(p) => p,
                    Err(e) => return Some(Err(e)),
                };
                let Some(latest) = page.latest() else {
                    continue;
                };
                if page.redirect.is_some()
                    || !latest
                        .model
                        .as_deref()
                        .is_some_and(|m| ENTITY_MODELS.contains(&m))
                    || is_redirect_json(&latest.text)
                {
                    continue;
                }
                return Some(
                    Entity::from_json(&latest.text)
                        .map(|mut parsed| {
                            let page_info = parsed.page.get_or_insert_with(PageInfo::default);
                            page_info.pageid = Some(page.id);
                            page_info.lastrevid = Some(latest.id);
                            page_info.modified = Some(latest.timestamp.clone());
                            parsed
                        })
                        .map_err(|e| DumpError::Entity {
                            line: usize::try_from(page.id).unwrap_or(0),
                            message: format!("page {} (`{}`): {e}", page.id, page.title),
                        }),
                );
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_pages_become_entities_with_revisions() {
        let items: Vec<ParsedEntity> =
            Entities::Xml(XmlDump::new(crate::xml_dump::tests::SAMPLE.as_bytes()))
                .map(Result::unwrap)
                .collect();
        assert_eq!(items.len(), 2);
        let page = items[0].page.as_ref().unwrap();
        assert_eq!((page.pageid, page.lastrevid), (Some(12), Some(41_877)));
        assert_eq!(page.modified.as_deref(), Some("2026-09-20T14:02:11Z"));
    }
}
