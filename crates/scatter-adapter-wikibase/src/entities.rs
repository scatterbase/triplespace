//! Either dump as a stream of entities: the JSON dump's lines, or the XML dump's entity
//! pages (newest revision, redirects and other content models skipped), each with its
//! page metadata filled in from the dump — `lastrevid` and `modified` from the revision,
//! `pageid` from the page. The format is told from the first byte: `<` is XML.
//!
//! [`Entities::entries`] keeps the redirects as well: an XML dump carries a merged item
//! as a page whose newest revision is `{"entity": "Q2", "redirect": "Q1"}`, which a
//! mirror sync records as an upstream redirect (0002 §8.4). A JSON dump has none.

use std::io::BufRead;
use std::path::Path;

use scatter_wikibase_model::entity::{Entity, PageInfo, ParsedEntity};
use scatter_wikibase_model::id::EntityId;

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

/// An upstream merge as a dump records it: `from`'s page now redirects to `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpRedirect {
    /// The retired ID, as the dump writes it (`Q2`).
    pub from: EntityId,
    /// The target (`Q1`).
    pub to: EntityId,
    /// The revision that made the page a redirect.
    pub revid: u64,
    /// Its timestamp.
    pub timestamp: String,
}

/// An entity or a redirect.
#[derive(Debug, Clone, PartialEq)]
pub enum DumpEntry {
    /// An entity, with its page metadata.
    Entity(Box<ParsedEntity>),
    /// A redirect.
    Redirect(DumpRedirect),
}

/// A dump's entities and redirects ([`Entities::entries`]).
pub struct Entries<R: BufRead>(Entities<R>);

impl<R: BufRead> Entities<R> {
    /// Whether the dump is a MediaWiki XML dump, whose snaks carry no `datatype`.
    pub fn is_xml(&self) -> bool {
        matches!(self, Self::Xml(_))
    }

    /// The dump's redirects as well as its entities.
    pub fn entries(self) -> Entries<R> {
        Entries(self)
    }

    fn next_entry(&mut self) -> Option<Result<DumpEntry, DumpError>> {
        match self {
            Self::Json(d) => d.next().map(|r| r.map(|e| DumpEntry::Entity(Box::new(e)))),
            Self::Xml(d) => loop {
                let page = match d.next()? {
                    Ok(p) => p,
                    Err(e) => return Some(Err(e)),
                };
                let Some(latest) = page.latest() else {
                    continue;
                };
                if !latest
                    .model
                    .as_deref()
                    .is_some_and(|m| ENTITY_MODELS.contains(&m))
                {
                    continue;
                }
                if page.redirect.is_some() || is_redirect_json(&latest.text) {
                    match redirect_of(&latest.text) {
                        Some((from, to)) => {
                            return Some(Ok(DumpEntry::Redirect(DumpRedirect {
                                from,
                                to,
                                revid: latest.id,
                                timestamp: latest.timestamp.clone(),
                            })));
                        }
                        None => continue,
                    }
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
                        .map(|e| DumpEntry::Entity(Box::new(e)))
                        .map_err(|e| DumpError::Entity {
                            line: usize::try_from(page.id).unwrap_or(0),
                            message: format!("page {} (`{}`): {e}", page.id, page.title),
                        }),
                );
            },
        }
    }
}

/// The two IDs of a redirect's content, `{"entity": "Q2", "redirect": "Q1"}`.
fn redirect_of(text: &str) -> Option<(EntityId, EntityId)> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let from = EntityId::parse(v.get("entity")?.as_str()?).ok()?;
    let to = EntityId::parse(v.get("redirect")?.as_str()?).ok()?;
    Some((from, to))
}

impl<R: BufRead> Iterator for Entities<R> {
    type Item = Result<ParsedEntity, DumpError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.next_entry()? {
                Ok(DumpEntry::Entity(e)) => return Some(Ok(*e)),
                Ok(DumpEntry::Redirect(_)) => {}
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

impl<R: BufRead> Iterator for Entries<R> {
    type Item = Result<DumpEntry, DumpError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next_entry()
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

    #[test]
    fn entries_keep_the_redirects() {
        let entries: Vec<DumpEntry> =
            Entities::Xml(XmlDump::new(crate::xml_dump::tests::SAMPLE.as_bytes()))
                .entries()
                .map(Result::unwrap)
                .collect();
        let redirects: Vec<&DumpRedirect> = entries
            .iter()
            .filter_map(|e| match e {
                DumpEntry::Redirect(r) => Some(r),
                DumpEntry::Entity(_) => None,
            })
            .collect();
        assert_eq!(
            entries.len() - redirects.len(),
            2,
            "the same entities as `Entities`"
        );
        assert_eq!(redirects.len(), 1);
        assert_eq!(
            (redirects[0].from.as_str(), redirects[0].to.as_str()),
            ("Q7", "Q6")
        );
    }
}
