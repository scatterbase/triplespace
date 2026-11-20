//! Golden test against the Wikibase 1.43 reference install (docs/samples/wikibase-1.43),
//! moved here from `scatter-mediawiki-xml` when the writer became RevisionChest's `mwdump`
//! (0112 §2), so that a change in `mwdump` that would change `Special:Export` shows here:
//! its `meta=siteinfo` written as a dump's `<siteinfo>`, and its main test item `Q8` as
//! one `wikibase-item` revision, must come out exactly as `tests/fixtures/wikibase-1.43-Q8.xml`
//! holds them, which is the shape MediaWiki 1.43's `XmlDumpWriter` gives a
//! `Special:Export` of the item (0047 §8.1; 21 §4.2). The samples are captures; the
//! expectation is this crate's.

use std::path::PathBuf;

use mwdump::{DumpReader, Namespace, PageHeader, Revision, SiteInfo, Writer};
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
    let mut namespaces: Vec<Namespace> = v["query"]["namespaces"]
        .as_object()
        .unwrap()
        .values()
        .map(|n| {
            let mut ns = Namespace::default();
            ns.key = n["id"].as_i64().unwrap();
            ns.case = Some(n["case"].as_str().unwrap().to_string());
            ns.name = n["name"].as_str().unwrap().to_string();
            ns
        })
        .collect();
    namespaces.sort_by_key(|n| n.key);
    let s = |key: &str| Some(general[key].as_str().unwrap().to_string());
    let mut site = SiteInfo::default();
    site.lang = s("lang");
    site.sitename = s("sitename");
    site.dbname = s("wikiid");
    site.base = s("base");
    site.generator = s("generator");
    site.case = s("case");
    site.namespaces = namespaces;
    site
}

/// `Q8` as one revision: the entity JSON `Special:EntityData` serves, as the item's page
/// text, with the page and revision IDs the reference install gave it.
fn q8() -> (PageHeader, Revision) {
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
    let mut header = PageHeader::default();
    header.title = "Item:Q8".into();
    header.ns = 120;
    header.id = page_id;
    let mut rev = Revision::default();
    rev.id = revid;
    rev.timestamp = modified;
    rev.user_id = Some(1);
    rev.user_name = Some("Admin".into());
    rev.comment = Some("/* wbeditentity-create-item:0| */".into());
    rev.origin = Some(revid);
    rev.model = Some("wikibase-item".into());
    rev.format = Some("application/json".into());
    rev.text = text.to_string();
    (header, rev)
}

#[test]
fn the_reference_installs_item_as_special_export_writes_it() {
    let mut w = Writer::new(Vec::new());
    w.start(&siteinfo()).unwrap();
    let (header, rev) = q8();
    w.page(&header, &[rev]).unwrap();
    let out = String::from_utf8(w.finish().unwrap()).unwrap();
    let golden = fixture("wikibase-1.43-Q8.xml");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &out).unwrap();
    }
    let expected = std::fs::read_to_string(&golden).unwrap();
    assert_eq!(out, expected, "set UPDATE_GOLDEN=1 to rewrite the fixture");

    // And it reads back as the same page, with MediaWiki's sha1 of the text.
    let mut dump = DumpReader::new(out.as_bytes()).pages();
    let pages: Vec<mwdump::Page> = dump.by_ref().map(Result::unwrap).collect();
    assert_eq!(dump.site_info(), Some(&siteinfo()));
    assert_eq!(pages.len(), 1);
    let rev = &pages[0].revisions[0];
    assert_eq!(rev.model.as_deref(), Some("wikibase-item"));
    assert_eq!(
        rev.sha1.as_deref(),
        Some(mwdump::sha1_base36(&rev.text).as_str())
    );
    let item: Value = serde_json::from_str(&rev.text).unwrap();
    assert_eq!(item["id"], "Q8");
}
