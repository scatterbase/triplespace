//! Round trips over real dumps (0047 §8.1; 0005 A119): each sample in `docs/samples/`
//! read, written again, and read back to the same pages and the same `<siteinfo>`.

use std::io::BufReader;
use std::path::PathBuf;

use scatter_mediawiki_xml::{DumpPage, SiteInfo, Writer, XmlDump};

fn sample(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples")
        .join(path)
}

fn read(path: &str) -> (SiteInfo, Vec<DumpPage>) {
    let file = std::fs::File::open(sample(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    let gzip = std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gz"));
    let reader: Box<dyn std::io::BufRead> = if gzip {
        Box::new(BufReader::new(flate2::read::GzDecoder::new(file)))
    } else {
        Box::new(BufReader::new(bzip2::read::MultiBzDecoder::new(file)))
    };
    let mut dump = XmlDump::new(reader);
    let pages: Vec<DumpPage> = dump.by_ref().map(|p| p.unwrap()).collect();
    (dump.siteinfo().clone(), pages)
}

fn round_trip(path: &str, pages_expected: usize, revisions_expected: usize) {
    let (site, pages) = read(path);
    assert_eq!(pages.len(), pages_expected, "{path}");
    let revisions: usize = pages.iter().map(|p| p.revisions.len()).sum();
    assert_eq!(revisions, revisions_expected, "{path}");
    assert!(
        !site.sitename.is_empty() && !site.namespaces.is_empty(),
        "{path}"
    );

    let mut w = Writer::new(Vec::new());
    w.start(&site).unwrap();
    for p in &pages {
        w.page(p).unwrap();
    }
    let written = w.finish().unwrap();

    let mut again = XmlDump::new(written.as_slice());
    let back: Vec<DumpPage> = again.by_ref().map(|p| p.unwrap()).collect();
    assert_eq!(again.siteinfo(), &site, "{path}: siteinfo");
    assert_eq!(back.len(), pages.len());
    for (a, b) in pages.iter().zip(&back) {
        assert_eq!(a, b, "{path}: page {}", a.title);
    }
    // Every revision carried its own sha1, which the writer kept rather than recomputed.
    assert!(
        back.iter()
            .flat_map(|p| &p.revisions)
            .all(|r| r.sha1.is_some() && r.origin.is_some()),
        "{path}"
    );
}

#[test]
fn librarybase_wikitext_pages_round_trip() {
    round_trip("librarybase/dump/librarybase-wikitext.xml.gz", 39, 147);
}

#[test]
fn librarybase_entities_round_trip() {
    round_trip("librarybase/dump/librarybase-sample.xml.gz", 254, 295);
}

#[test]
fn enwiki_talk_current_round_trips() {
    round_trip("enwiki-talk/current.xml.bz2", 9, 9);
}

#[test]
fn enwiki_talk_history_round_trips() {
    round_trip("enwiki-talk/history.xml.bz2", 1, 4677);
}

/// A dump's text escaped only where XML needs it: the wikitext's own `&quot;` and
/// `&lt;` entities come back as the characters the dump decoded them to.
#[test]
fn the_sample_text_survives_escaping() {
    let (_, pages) = read("enwiki-talk/history.xml.bz2");
    let first = &pages[0].revisions[0];
    assert!(
        first
            .text
            .contains(r##"("<font color="#ba0000"><u>Sarah</u></font>")"##)
    );
    let mut w = Writer::new(Vec::new());
    w.page(&DumpPage {
        revisions: vec![first.clone()],
        ..pages[0].clone()
    })
    .unwrap();
    let out = String::from_utf8(w.finish().unwrap()).unwrap();
    assert!(
        out.contains("(&quot;&lt;font color=&quot;#ba0000&quot;&gt;")
            || out.contains("(\"&lt;font color=\"#ba0000\"&gt;")
    );
}
