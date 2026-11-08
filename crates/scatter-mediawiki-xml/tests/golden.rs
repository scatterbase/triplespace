//! Golden test against the Wikibase 1.43 reference install (docs/samples/wikibase-1.43):
//! its `meta=siteinfo` written as a dump's `<siteinfo>`, and its main test item `Q8` as
//! one `wikibase-item` revision, must come out exactly as `tests/fixtures/wikibase-1.43-Q8.xml`
//! holds them, which is the shape MediaWiki 1.43's `XmlDumpWriter` gives a
//! `Special:Export` of the item (0047 §8.1; 21 §4.2). The samples are captures; the
//! expectation is this crate's.

use std::path::PathBuf;

use scatter_mediawiki_xml::{DumpPage, Revision, SiteInfo, SiteNamespace, Writer, XmlDump};
use serde_json::Value;

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/wikibase-1.43")
        .join(name)
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The reference install's `<siteinfo>`, from its `meta=siteinfo`.
fn siteinfo() -> SiteInfo {
    let v: Value = serde_json::from_slice(
        &std::fs::read(sample("mw-1.43.9-wb-REL1_43.siteinfo.json")).unwrap(),
    )
    .unwrap();
    let general = &v["query"]["general"];
    let mut namespaces: Vec<SiteNamespace> = v["query"]["namespaces"]
        .as_object()
        .unwrap()
        .values()
        .map(|n| SiteNamespace {
            key: n["id"].as_i64().unwrap(),
            case: n["case"].as_str().unwrap().to_string(),
            name: n["name"].as_str().unwrap().to_string(),
        })
        .collect();
    namespaces.sort_by_key(|n| n.key);
    SiteInfo {
        lang: general["lang"].as_str().unwrap().to_string(),
        sitename: general["sitename"].as_str().unwrap().to_string(),
        dbname: general["wikiid"].as_str().unwrap().to_string(),
        base: general["base"].as_str().unwrap().to_string(),
        generator: general["generator"].as_str().unwrap().to_string(),
        case: general["case"].as_str().unwrap().to_string(),
        namespaces,
    }
}

/// `Q8` as one revision: the entity JSON `Special:EntityData` serves, as the item's page
/// text, with the page and revision IDs the reference install gave it.
fn q8() -> DumpPage {
    let v: Value =
        serde_json::from_slice(&std::fs::read(sample("wikibase-Q8.json")).unwrap()).unwrap();
    let entity = &v["entities"]["Q8"];
    let mut text = entity.clone();
    let page_id = text["pageid"].as_u64().unwrap();
    let revid = text["lastrevid"].as_u64().unwrap();
    let modified = text["modified"].as_str().unwrap().to_string();
    for key in ["pageid", "ns", "title", "lastrevid", "modified"] {
        text.as_object_mut().unwrap().remove(key);
    }
    DumpPage {
        title: "Item:Q8".into(),
        ns: 120,
        id: page_id,
        redirect: None,
        revisions: vec![Revision {
            id: revid,
            parent_id: None,
            timestamp: modified,
            user: Some((1, "Admin".into())),
            comment: Some("/* wbeditentity-create-item:0| */".into()),
            origin: Some(revid),
            model: Some("wikibase-item".into()),
            format: Some("application/json".into()),
            text: text.to_string(),
            ..Revision::default()
        }],
    }
}

#[test]
fn the_reference_installs_item_as_special_export_writes_it() {
    let mut w = Writer::new(Vec::new());
    w.start(&siteinfo()).unwrap();
    w.page(&q8()).unwrap();
    let out = String::from_utf8(w.finish().unwrap()).unwrap();
    let golden = fixture("wikibase-1.43-Q8.xml");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &out).unwrap();
    }
    let expected = std::fs::read_to_string(&golden).unwrap();
    assert_eq!(out, expected, "set UPDATE_GOLDEN=1 to rewrite the fixture");

    // And it reads back as the same page, with MediaWiki's sha1 of the text.
    let mut dump = XmlDump::new(out.as_bytes());
    let pages: Vec<DumpPage> = dump.by_ref().map(Result::unwrap).collect();
    assert_eq!(dump.siteinfo(), &siteinfo());
    assert_eq!(pages.len(), 1);
    let rev = &pages[0].revisions[0];
    assert_eq!(rev.model.as_deref(), Some("wikibase-item"));
    assert_eq!(
        rev.sha1.as_deref(),
        Some(scatter_mediawiki_xml::sha1_base36(&rev.text).as_str())
    );
    let item: Value = serde_json::from_str(&rev.text).unwrap();
    assert_eq!(item["id"], "Q8");
}
