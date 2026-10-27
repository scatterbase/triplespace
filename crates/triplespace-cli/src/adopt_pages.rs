//! The document pages of an adopted wiki (0035 A13; 0008 §9, A38; 10 §6): every page the
//! tenant's namespaces accept comes in the adoption job, revision by revision, into the
//! tenant's `pages` partition under its source page ID and revision IDs.
//!
//! A page is accepted when its namespace holds pages and allows its content model, with
//! the registry's own namespaces: Project, User, Template and the main namespace (0008
//! A41; 0042 A19). Talk pages, whose model is `triplespace-talk`, become threads by a later
//! conversion; they and pages in reserved namespaces (MediaWiki) are counted `deferred`.
//!
//! The first revision is a `create` with the page's current title (an export lists a
//! page's revisions under its current title, so earlier titles are not known); each later
//! one an `edit`, with the model when it changed. The parts the source hid are left out
//! and named in `withheld`; a hidden user is a hidden attestation, as for entities.

use scatter_actors::key::ActorKey;
use scatter_adapter_wikibase::adoption::Contributor;
use scatter_adapter_wikibase::xml_dump::DumpPage;
use scatter_ingest::adopt::{AdoptedPage, Author};
use scatter_log::cbor::Value;
use scatter_pages::{PAYLOAD_PAGE, PageOp, Source, validate, withheld};
use scatter_titles::Namespaces;

/// What became of a dump page that is not an entity page.
#[derive(Debug)]
pub enum PageRead {
    /// Its revisions, to adopt.
    Adopt(Vec<AdoptedPage>),
    /// Left for a later import, with the reason.
    Deferred(String),
}

/// The author of a dump revision, under the tenant's issuer.
#[must_use]
pub fn author(contributor: Contributor, issuer: &str) -> Author {
    match contributor {
        Contributor::Account(id) => Author::Actor(ActorKey::numeric(issuer, id).to_string()),
        Contributor::Ip(ip) => Author::Surrogate {
            kind: "anonymous".into(),
            raw: ip,
        },
        Contributor::Imported(name) => Author::Surrogate {
            kind: "imported".into(),
            raw: name,
        },
        Contributor::Hidden => Author::Hidden,
    }
}

/// Reads a dump page as document page revisions.
#[must_use]
pub fn read_page(page: &DumpPage, issuer: &str, current_only: bool) -> PageRead {
    let Ok(ns) = i32::try_from(page.ns) else {
        return PageRead::Deferred(format!("namespace {} is out of range", page.ns));
    };
    let Some(namespace) = Namespaces::embedded().get(ns) else {
        return PageRead::Deferred(format!("namespace {ns} is not registered here"));
    };
    let model = page
        .revisions
        .last()
        .and_then(|r| r.model.clone())
        .or_else(|| namespace.default_model.clone())
        .unwrap_or_else(|| "wikitext".into());
    if namespace.kind != scatter_titles::Kind::Pages {
        return PageRead::Deferred(format!("`{}` holds no pages here", namespace.canonical));
    }
    if !namespace.allows_model(&model) {
        return PageRead::Deferred(format!(
            "`{}` pages are not `{model}` here{}",
            namespace.canonical,
            if namespace.default_model.as_deref() == Some("triplespace-talk") {
                " (talk pages become threads)"
            } else {
                ""
            }
        ));
    }
    let title = if ns == 0 {
        page.title.clone()
    } else {
        page.title
            .split_once(':')
            .map_or_else(|| page.title.clone(), |(_, t)| t.to_string())
    };
    let revisions: Vec<_> = if current_only {
        page.revisions.last().into_iter().collect()
    } else {
        page.revisions.iter().collect()
    };
    let mut out = Vec::new();
    let mut previous_model: Option<String> = None;
    let mut adopted_ids = Vec::new();
    for r in revisions {
        let rev_model = r.model.clone().unwrap_or_else(|| model.clone());
        let mut hidden = Vec::new();
        if r.text_hidden {
            hidden.push(withheld::TEXT.to_string());
        }
        if r.comment_hidden {
            hidden.push(withheld::COMMENT.to_string());
        }
        if r.user_hidden {
            hidden.push(withheld::ATTESTATION.to_string());
        }
        let source = (!current_only).then(|| Source {
            source_revid: r.id,
            source_parentid: r.parent_id.filter(|p| adopted_ids.contains(p)),
            source_time: r.timestamp.clone(),
            withheld: hidden.clone(),
        });
        let op = match &previous_model {
            None => PageOp::Create {
                ns,
                title: title.clone(),
                model: rev_model.clone(),
                minor: r.minor,
                source,
            },
            Some(prev) => PageOp::Edit {
                model: (*prev != rev_model).then(|| rev_model.clone()),
                minor: r.minor,
                source,
            },
        };
        let text = (!r.text_hidden).then_some(r.text.as_str());
        if let Err(e) = validate(&op, text) {
            return PageRead::Deferred(format!("revision {}: {e}", r.id));
        }
        previous_model = Some(rev_model);
        adopted_ids.push(r.id);
        out.push(AdoptedPage {
            page_id: page.id,
            payload_type: PAYLOAD_PAGE.into(),
            content: op.to_json(),
            extra: text.map(Value::text).into_iter().collect(),
            source_revid: r.id,
            comment: if r.comment_hidden {
                None
            } else {
                r.comment.clone()
            },
            author: if current_only {
                Author::Job
            } else {
                author(Contributor::of(r), issuer)
            },
        });
    }
    PageRead::Adopt(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_adapter_wikibase::xml_dump::Revision;
    use serde_json::json;

    fn revision(id: u64, parent: Option<u64>, text: &str) -> Revision {
        Revision {
            id,
            parent_id: parent,
            timestamp: format!("2026-01-0{id}T00:00:00Z"),
            user: Some((7, "Harej".into())),
            model: Some("wikitext".into()),
            text: text.into(),
            comment: Some(format!("edit {id}")),
            ..Revision::default()
        }
    }

    #[test]
    fn a_project_page_with_history() {
        let mut second = revision(2, Some(1), "two");
        second.comment_hidden = true;
        second.comment = None;
        let page = DumpPage {
            title: "Project:Scope".into(),
            ns: 4,
            id: 10,
            redirect: None,
            revisions: vec![revision(1, None, "one"), second],
        };
        let PageRead::Adopt(revs) = read_page(&page, "lb", false) else {
            panic!("adopted")
        };
        assert_eq!(revs.len(), 2);
        assert_eq!(
            revs[0].content,
            json!({"op": "create", "ns": 4, "title": "Scope", "model": "wikitext",
                   "source_revid": 1, "source_time": "2026-01-01T00:00:00Z"})
        );
        assert_eq!(revs[0].extra, vec![Value::text("one")]);
        assert_eq!(revs[0].author, Author::Actor("lb:7".into()));
        assert_eq!(
            revs[1].content,
            json!({"op": "edit", "source_revid": 2, "source_parentid": 1,
                   "source_time": "2026-01-02T00:00:00Z", "withheld": ["comment"]})
        );
        assert_eq!(revs[1].comment, None);
    }

    #[test]
    fn talk_and_interface_pages_are_deferred() {
        let talk = DumpPage {
            title: "User talk:Harej".into(),
            ns: 3,
            id: 1,
            redirect: None,
            revisions: vec![revision(1, None, "hi")],
        };
        assert!(
            matches!(read_page(&talk, "lb", false), PageRead::Deferred(r) if r.contains("threads"))
        );
        let message = DumpPage {
            title: "MediaWiki:Sidebar".into(),
            ns: 8,
            id: 2,
            redirect: None,
            revisions: vec![revision(1, None, "x")],
        };
        assert!(matches!(
            read_page(&message, "lb", false),
            PageRead::Deferred(_)
        ));
        let template = DumpPage {
            title: "Template:Cwbn".into(),
            ns: 10,
            id: 3,
            redirect: None,
            revisions: vec![revision(1, None, "<div/>")],
        };
        assert!(matches!(read_page(&template, "lb", false), PageRead::Adopt(r) if r.len() == 1));
    }
}
