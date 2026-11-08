//! The general import job (0008 §9, A48; 10 §6) against the in-memory store: new pages
//! under minted IDs with their original timestamps and authors, merging onto a title
//! that exists by timestamp with the skip rule and the restoring edit, and a second run
//! that writes nothing twice.

use std::collections::BTreeMap;

use scatter_ingest::IngestError;
use scatter_ingest::adopt::Author;
use scatter_ingest::import::{
    HeldPage, HeldRevision, Import, ImportHost, ImportPage, ImportRevision, PAYLOAD_PAGE,
    RESTORE_COMMENT, run_import,
};
use scatter_ingest::memory::{MemoryCx, MemoryIngest};
use scatter_ingest::write::{Attestation, RawRecord, Request, write_raw};
use scatter_log::cbor::Value;
use scatter_log::record::Record;
use scatter_mediawiki_xml::sha1_base36;
use scatter_projection::{Backend as _, Pipeline};
use serde_json::json;

const TENANT: &str = "lb";
const PAGES: u64 = 0x12;
const TENANT_LOG: u64 = 0x13;
const ACTORS: u64 = 0x14;
const NOW: u64 = 1_790_000_000_000_000;
const DAY: u64 = 86_400_000_000;

fn setup() -> (MemoryIngest, Pipeline<MemoryIngest>) {
    let store = MemoryIngest::new();
    store.create_partition(PAGES, TENANT, "pages");
    store.create_partition(TENANT_LOG, TENANT, "log");
    store.create_partition(ACTORS, TENANT, "actors");
    store.create_partition(0x21, "", "log");
    (store, Pipeline::new())
}

/// A timestamp in 2026-01, day `d`, as the file writes it, and its µs.
fn stamp(d: u64) -> (String, u64) {
    (
        format!("2026-01-{d:02}T00:00:00Z"),
        1_767_225_600_000_000 + (d - 1) * DAY,
    )
}

fn micros_of(stamp: &str) -> u64 {
    let d: u64 = stamp[8..10].parse().unwrap();
    1_767_225_600_000_000 + (d - 1) * DAY
}

fn revision(id: u64, day: u64, text: &str, author: Author) -> ImportRevision {
    let (source_time, time) = stamp(day);
    ImportRevision {
        source_revid: id,
        source_parentid: (id > 1).then(|| id - 1),
        source_time,
        time,
        model: "wikitext".into(),
        text: Some(text.into()),
        sha1: sha1_base36(text),
        minor: false,
        comment: Some(format!("edit {id}")),
        withheld: Vec::new(),
        author,
    }
}

/// The surface's title index, read from the store's own records.
struct Host;

fn page_json(r: &Record) -> Option<serde_json::Value> {
    r.body().content().value().ok().flatten()?.to_json().ok()
}

fn text_of(r: &Record) -> Option<String> {
    match r.body().part(3)?.value().ok().flatten()? {
        Value::Text(t) => Some(t),
        _ => None,
    }
}

impl ImportHost<MemoryIngest> for Host {
    async fn existing(
        &self,
        _store: &MemoryIngest,
        cx: &mut MemoryCx,
        _tenant: &str,
        ns: i32,
        title: &str,
    ) -> Result<Option<HeldPage>, IngestError> {
        let records = &cx.state.partitions[&PAGES].records;
        let mut by_key: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
        for r in records {
            if r.header().payload_type == PAYLOAD_PAGE {
                by_key
                    .entry(r.header().key.clone().unwrap_or_default())
                    .or_default()
                    .push(r);
            }
        }
        for (key, recs) in by_key {
            let mut current_title = None;
            let mut model = String::new();
            let mut revisions = Vec::new();
            let mut current_text = None;
            for r in &recs {
                let c = page_json(r).unwrap();
                if c["op"] == "create" || c["op"] == "move" {
                    current_title = Some((
                        i32::try_from(c["ns"].as_i64().unwrap()).unwrap(),
                        c["title"].as_str().unwrap().to_string(),
                    ));
                }
                if let Some(m) = c["model"].as_str() {
                    model = m.to_string();
                }
                if c["op"] == "create" || c["op"] == "edit" {
                    let text = text_of(r);
                    revisions.push(HeldRevision {
                        time: c["source_time"]
                            .as_str()
                            .map_or(r.header().appended_at, micros_of),
                        sha1: text.as_deref().map(sha1_base36).unwrap_or_default(),
                    });
                    current_text = text;
                }
            }
            if current_title == Some((ns, title.to_string())) {
                return Ok(Some(HeldPage {
                    page_id: key.parse().unwrap(),
                    model,
                    revisions,
                    current_text,
                }));
            }
        }
        Ok(None)
    }
}

fn import() -> Import {
    Import {
        source: "upload".into(),
        version: Some("pages.xml".into()),
        history: true,
        summary: None,
    }
}

/// One page record: (op, source_revid, text, actor, comment).
type Row = (
    String,
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The page records of a title, in order.
fn history(store: &MemoryIngest, page_id: u64) -> Vec<Row> {
    store
        .records(PAGES)
        .iter()
        .filter(|r| r.header().key.as_deref() == Some(&page_id.to_string()))
        .map(|r| {
            let c = page_json(r).unwrap();
            let att = r
                .body()
                .part(2)
                .and_then(|p| p.value().ok().flatten())
                .and_then(|v| v.to_json().ok())
                .unwrap_or_default();
            let comment = r
                .body()
                .part(1)
                .and_then(|p| p.value().ok().flatten())
                .and_then(|v| v.to_json().ok())
                .and_then(|v| v.as_str().map(str::to_string));
            (
                c["op"].as_str().unwrap().to_string(),
                c["source_revid"].as_u64(),
                text_of(r),
                att["actor"].as_str().map(str::to_string),
                comment,
            )
        })
        .collect()
}

fn events(store: &MemoryIngest) -> Vec<(String, serde_json::Value)> {
    store
        .records(TENANT_LOG)
        .iter()
        .filter_map(page_json)
        .map(|c| {
            (
                c["action"].as_str().unwrap().to_string(),
                c["params"].clone(),
            )
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)]
fn an_import_mints_ids_keeps_timestamps_and_attributes_authors() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let pages = vec![
            ImportPage {
                ns: 0,
                title: "Alpha".into(),
                revisions: vec![
                    // Out of order in the file; the job orders them by timestamp.
                    revision(
                        1002,
                        3,
                        "two",
                        Author::Surrogate {
                            kind: "imported".into(),
                            raw: "wp>Jane".into(),
                        },
                    ),
                    revision(1001, 1, "one", Author::Actor("lb:7".into())),
                    revision(
                        1003,
                        5,
                        "three",
                        Author::Surrogate {
                            kind: "imported".into(),
                            raw: "wp>Jane".into(),
                        },
                    ),
                ],
            },
            ImportPage {
                ns: 4,
                title: "About".into(),
                revisions: vec![revision(9, 2, "about", Author::Hidden)],
            },
            ImportPage {
                ns: 0,
                title: "Empty".into(),
                revisions: vec![],
            },
        ];
        let out = run_import(
            &store,
            &pipeline,
            &Host,
            TENANT,
            Attestation::by("lb:1"),
            import(),
            pages,
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(
            (
                out.counts.created,
                out.counts.revisions,
                out.counts.rejected
            ),
            (2, 4, 1)
        );
        assert_eq!(out.rejects.len(), 1);
        assert!(out.rejects[0].reason.contains("no revisions"));
        let created: Vec<_> = out.pages.iter().filter(|p| p.created).collect();
        assert_eq!(created.len(), 2);
        let alpha = created[0].page_id;
        assert_eq!((created[0].ns, created[0].title.as_str()), (0, "Alpha"));
        assert_ne!(alpha, created[1].page_id);
        assert_eq!((created[0].revisions, created[0].present), (3, 0));

        let h = history(&store, alpha);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].0, "create");
        assert_eq!(
            (h[0].1, h[1].1, h[2].1),
            (Some(1001), Some(1002), Some(1003)),
            "in timestamp order"
        );
        assert_eq!(h[0].2.as_deref(), Some("one"));
        assert_eq!(
            h[0].3.as_deref(),
            Some("lb:7"),
            "a registered issuer's account"
        );
        assert_eq!(h[1].3.as_deref(), Some("actor:1"), "an imported surrogate");
        assert_eq!(
            h[2].3.as_deref(),
            Some("actor:1"),
            "the same raw value, once"
        );
        assert_eq!(h[0].4.as_deref(), Some("edit 1001"));
        // The content keeps the source's revision ID, parent and time; the record's own
        // revision ID is the tenant's.
        let first = store
            .records(PAGES)
            .into_iter()
            .find(|r| r.header().key.as_deref() == Some(&alpha.to_string()))
            .unwrap();
        let c = page_json(&first).unwrap();
        assert_eq!(c["source_time"], "2026-01-01T00:00:00Z");
        assert_eq!(c["source_revid"], 1001);
        assert_eq!(c["ns"], 0);
        assert_eq!(c["title"], "Alpha");
        assert_eq!(c["model"], "wikitext");
        assert_eq!(first.header().revid, Some(1), "the tenant's own sequence");
        let second = &store.records(PAGES)[1];
        assert_eq!(page_json(second).unwrap()["source_parentid"], 1001);
        // The surrogate's record names what it stands for.
        let actors = store.records(ACTORS);
        assert_eq!(actors.len(), 1);
        assert_eq!(
            page_json(&actors[0]).unwrap(),
            json!({"kind": "imported", "status": "active", "raw": "wp>Jane"})
        );
        // A hidden author: no actor.
        let about = history(&store, created[1].page_id);
        assert_eq!(about[0].3, None);
        // The job's records.
        let ev = events(&store);
        assert_eq!(ev[0].0, "start");
        assert_eq!(ev[0].1["mode"], "import");
        assert_eq!(ev[0].1["graph"], "pages");
        assert_eq!(ev[0].1["args"]["history"], "full");
        assert_eq!(ev[1].0, "finish");
        assert_eq!(ev[1].1["import"]["created"], 2);
        assert_eq!(ev[1].1["counts"]["rejected"], 1);
        assert_eq!(ev[1].1["rejects"].as_array().unwrap().len(), 1);
    });
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_second_run_writes_nothing_twice_and_a_merge_goes_by_timestamp() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let author = || Author::Actor("lb:7".into());
        let page = || ImportPage {
            ns: 0,
            title: "Alpha".into(),
            revisions: vec![
                revision(1, 1, "one", author()),
                revision(2, 3, "two", author()),
                revision(3, 5, "three", author()),
            ],
        };
        let run = |pages: Vec<ImportPage>| {
            run_import(
                &store,
                &pipeline,
                &Host,
                TENANT,
                Attestation::by("lb:1"),
                import(),
                pages,
                NOW,
            )
        };
        let out = run(vec![page()]).await.unwrap();
        assert_eq!(out.counts.revisions, 3);
        let alpha = out.pages[0].page_id;

        // The same file again: every revision is present, nothing is written.
        let out = run(vec![page()]).await.unwrap();
        assert_eq!(
            (
                out.counts.created,
                out.counts.merged,
                out.counts.revisions,
                out.counts.present,
                out.counts.restored
            ),
            (0, 1, 0, 3, 0)
        );
        assert!(!out.pages[0].created);
        assert_eq!(out.pages[0].page_id, alpha);
        assert_eq!(history(&store, alpha).len(), 3);

        // A local edit after the import, then a file with one older revision the page
        // lacks and one newer: the older goes into the history, the newer becomes
        // current; no restoring edit.
        let mut cx = store.begin().await.unwrap();
        let mut request = Request::new(TENANT, Attestation::by("lb:2"), stamp(4).1 + 1);
        request.comment = Some("local".into());
        write_raw(
            &store,
            &pipeline,
            &mut cx,
            &request,
            RawRecord {
                graph: "pages".into(),
                payload_type: PAYLOAD_PAGE.into(),
                key: Some(alpha.to_string()),
                page_id: None,
                content: Value::from_json(&json!({"op": "edit"})),
                extra: vec![Value::text("local text")],
            },
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
        let out = run(vec![ImportPage {
            ns: 0,
            title: "Alpha".into(),
            revisions: vec![
                revision(2, 3, "two", author()),
                revision(10, 4, "two and a half", author()),
                revision(11, 40, "much later", author()),
            ],
        }])
        .await
        .unwrap();
        assert_eq!(
            (
                out.counts.merged,
                out.counts.revisions,
                out.counts.present,
                out.counts.restored
            ),
            (1, 2, 1, 0)
        );
        let h = history(&store, alpha);
        assert_eq!(h.len(), 6);
        assert_eq!(h[5].2.as_deref(), Some("much later"));

        // A file whose revisions are all older than the page's current one: they go into
        // the history and the current revision is restored on top, by the job.
        let out = run(vec![ImportPage {
            ns: 0,
            title: "Alpha".into(),
            revisions: vec![revision(12, 6, "six", author())],
        }])
        .await
        .unwrap();
        assert_eq!((out.counts.revisions, out.counts.restored), (1, 1));
        assert!(out.pages[0].restored);
        let h = history(&store, alpha);
        assert_eq!(h.len(), 8);
        assert_eq!(h[6].2.as_deref(), Some("six"));
        assert_eq!(h[7].2.as_deref(), Some("much later"));
        assert_eq!(h[7].1, None, "the restoring edit is the job's own");
        assert_eq!(h[7].3.as_deref(), Some("lb:1"));
        assert_eq!(h[7].4.as_deref(), Some(RESTORE_COMMENT));
        // And once more: nothing to do.
        let out = run(vec![ImportPage {
            ns: 0,
            title: "Alpha".into(),
            revisions: vec![revision(12, 6, "six", author())],
        }])
        .await
        .unwrap();
        assert_eq!(
            (
                out.counts.revisions,
                out.counts.present,
                out.counts.restored
            ),
            (0, 1, 0)
        );
        assert_eq!(history(&store, alpha).len(), 8);
    });
}

#[test]
fn a_hidden_text_is_withheld_and_a_model_change_is_kept() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let mut hidden = revision(1, 1, "", Author::Actor("lb:7".into()));
        hidden.text = None;
        hidden.sha1 = String::new();
        hidden.withheld = vec!["text".into(), "comment".into()];
        hidden.comment = None;
        let mut css = revision(2, 2, "body {}", Author::Actor("lb:7".into()));
        css.model = "css".into();
        let out = run_import(
            &store,
            &pipeline,
            &Host,
            TENANT,
            Attestation::by("lb:1"),
            import(),
            vec![ImportPage {
                ns: 2,
                title: "Jane/common.css".into(),
                revisions: vec![hidden, css],
            }],
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(out.counts.revisions, 2);
        let recs: Vec<Record> = store
            .records(PAGES)
            .into_iter()
            .filter(|r| r.header().key.as_deref() == Some(&out.pages[0].page_id.to_string()))
            .collect();
        let first = page_json(&recs[0]).unwrap();
        assert_eq!(first["withheld"], json!(["text", "comment"]));
        assert!(recs[0].body().part(3).is_none(), "no text part");
        let second = page_json(&recs[1]).unwrap();
        assert_eq!(second["op"], "edit");
        assert_eq!(second["model"], "css");
        assert_eq!(text_of(&recs[1]).as_deref(), Some("body {}"));
    });
}
