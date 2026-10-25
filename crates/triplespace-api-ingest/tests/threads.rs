//! The thread write path over Postgres (0019 §1–§4; 14 §1.2–§1.5, §3.6, §4.2): records
//! checked against the thread's fold, minted titles and talk pages, the base-offset check,
//! and the `view` rows of 03 §4.8; and a talk page converted by `scatter-talk` and planned by
//! `scatter-talk-import`, written through the same path. Named by
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::SigningKey;
use scatter_actors::issuer::IssuerRegistry;
use scatter_ingest::{Attestation, RawRecord, Request, write_raw};
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::genesis::Genesis;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{
    Export, GraphRegistry, History, Integrity, KeyEntry, PAYLOAD_CONFIG, Scope,
};
use scatter_log::store::{Draft, LogStore};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, PgLog, log};
use scatter_projection::{Backend, Pipeline};
use scatter_threads::{Invalid, Limits, Target, ThreadOp, Writer};
use serde_json::json;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::PgIngest;
use triplespace_api_ingest::threads::{ThreadError, ThreadWrite, utc_date, write};
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

static COUNTER: AtomicU32 = AtomicU32::new(0);
thread_local! {
    /// The scratch databases this test made. Per thread, not per process: the tests run
    /// in parallel, each on its own thread, and `cleanup` must drop only its own
    /// databases — `DROP DATABASE … WITH (FORCE)` on another test's would terminate
    /// that test's connections mid-run (SQLSTATE 57P01).
    static CREATED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

const TENANT: &str = "librarybase";
const LOCAL: u64 = 0x3001;
const PAGES: u64 = 0x3002;
const TENANT_LOG: u64 = 0x3003;
const ACTORS: u64 = 0x3004;
const INSTANCE_LOG: u64 = 0x3011;
const MIRROR: u64 = 0x3012;
const NOW: u64 = 1_790_000_000_000_000;

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn fresh() -> (Client, PgBackend) {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "tsingest_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    connect(&admin)
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    CREATED.with_borrow_mut(|c| c.push(name.clone()));
    let mut config = admin;
    config.dbname(&name);
    let mut client = connect(&config).await;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .expect("migrate");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    let url = url
        .rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap();
    let pool = triplespace_db::pool::pool(&url, 4).unwrap();
    (client, PgBackend::new(pool))
}

async fn cleanup() {
    let Some(admin) = admin_config() else { return };
    let client = connect(&admin).await;
    let names: Vec<String> = CREATED.with_borrow_mut(std::mem::take);
    for n in names {
        let _ = client
            .batch_execute(&format!("DROP DATABASE IF EXISTS {n} WITH (FORCE)"))
            .await;
    }
}

fn skipped() -> bool {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return true;
    }
    false
}

fn pipeline() -> Pipeline<PgIngest> {
    milestone_pipeline(
        IssuerRegistry::default_registry().clone(),
        Farm {
            slug: "scatter".into(),
            code: "scatter".into(),
            base: "https://scatter.example".into(),
        },
    )
    .unwrap()
}

/// Genesis, the tenant's registry entry, the partitions and the sequences.
async fn seed(client: Client) -> Client {
    let key = SigningKey::from_bytes(&[29; 32]);
    let mut store = PgLog::new(client);
    Genesis {
        partition: CONFIG_PARTITION,
        scope: Scope::Instance,
        key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
        segment_exponent: 16,
        appended_at: 1,
        founding: Value::map(Vec::new()),
        attestation: Value::map(vec![(
            Value::text("actor"),
            Value::text("instance:scatter"),
        )]),
        comment: None,
    }
    .append(&mut store, &GraphRegistry::embedded())
    .await
    .unwrap();
    store
        .append(
            0,
            Draft {
                appended_at: 2,
                payload_type: PAYLOAD_CONFIG.into(),
                key: Some(format!("tenant:{TENANT}")),
                revid: None,
                logid: None,
                page_id: None,
                body: Body::core(
                    &Value::from_json(&json!({"kind": "tenant", "slug": TENANT, "base": "https://librarybase.org", "issuer": TENANT})),
                    &Value::Null,
                    &Value::map(vec![(Value::text("actor"), Value::text("instance:scatter"))]),
                )
                .unwrap(),
            },
        )
        .await
        .unwrap();
    let client = store.into_inner();
    for (partition, tenant, name) in [
        (LOCAL, Some(TENANT), "local"),
        (PAGES, Some(TENANT), "pages"),
        (TENANT_LOG, Some(TENANT), "log"),
        (ACTORS, Some(TENANT), "actors"),
        (INSTANCE_LOG, None, "log"),
        (MIRROR, None, "mirror/internetdomains"),
    ] {
        log::create_partition(
            &client,
            partition,
            Segments::new(8).unwrap(),
            &PartitionInfo {
                tenant: tenant.map(str::to_owned),
                name: name.into(),
                graph_iri: format!("https://scatter.example/graph/{name}"),
                history: History::Full,
                integrity: Integrity::Logged,
                export: Export::Public,
                hash: "sha-256".into(),
            },
        )
        .await
        .unwrap();
    }
    PgIngest::create_sequences(&client, TENANT, &["item", "property"], &["domain"])
        .await
        .unwrap();
    client
}

async fn rows(client: &Client, sql: &str) -> Vec<String> {
    let mut out: Vec<String> = client
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect();
    out.sort();
    out
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

fn request(actor: &str, base: Option<u64>) -> Request {
    let mut r = Request::new(TENANT, Attestation::by(actor), NOW);
    r.base_offset = base;
    r
}

fn create(name: &str, target: Target) -> ThreadOp {
    ThreadOp::Create {
        name: name.into(),
        target,
        also: Vec::new(),
        status: None,
        pinned: false,
        media_type: None,
        imported_from: None,
    }
}

fn post(reply: Option<u64>, status: Option<&str>) -> ThreadOp {
    ThreadOp::Post {
        in_reply_to: reply,
        status: status.map(str::to_string),
        media_type: None,
        heading: None,
        collapse: None,
        uncollapse: None,
        imported_from: None,
    }
}

fn by_editor(thread: Option<u64>, op: ThreadOp, text: Option<&str>) -> ThreadWrite {
    ThreadWrite {
        thread,
        op,
        text: text.map(str::to_string),
        writer: Writer::Editor,
        limits: Limits::default(),
        date: None,
        signer: None,
    }
}

/// Appends and commits, as an API call would.
async fn put(
    store: &PgIngest,
    p: &Pipeline<PgIngest>,
    req: &Request,
    w: ThreadWrite,
) -> Result<triplespace_api_ingest::threads::ThreadWritten, ThreadError> {
    let mut cx = store.begin().await.unwrap();
    match write(store, p, &mut cx, req, w).await {
        Ok(out) => {
            store.commit(cx).await.unwrap();
            Ok(out)
        }
        Err(e) => {
            store.rollback(cx).await.unwrap();
            Err(e)
        }
    }
}

/// An account to write to, as the actor projection would leave it.
async fn add_editor(client: &Client) {
    client
        .execute(
            "INSERT INTO view.actor (tenant, actor_key, issuer, subject, kind, name, status, iri, \"offset\")
             VALUES ('librarybase', 'librarybase:7', 'librarybase', '7', 'registered', 'Editor', 'active',
                     'https://librarybase.org/actor/7', 0)",
            &[],
        )
        .await
        .unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn threads_are_written_checked_and_projected() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    add_editor(&client).await;

    // A document page to talk about, and an account to write to.
    let mut cx = store.begin().await.unwrap();
    let page = write_raw(
        &store,
        &p,
        &mut cx,
        &request("librarybase:2", None),
        RawRecord {
            graph: "pages".into(),
            payload_type: scatter_pages::PAYLOAD_PAGE.into(),
            key: None,
            page_id: None,
            content: Value::from_json(
                &json!({"op": "create", "ns": 4, "title": "Scope", "model": "wikitext"}),
            ),
            extra: vec![Value::text("What Librarybase covers.")],
        },
    )
    .await
    .unwrap()
    .record
    .header()
    .page_id
    .unwrap();
    store.commit(cx).await.unwrap();

    // Create: a title from the date and subject, a talk page minted for the target.
    let a = put(
        &store,
        &p,
        &request("librarybase:7", None),
        by_editor(
            None,
            create("why is_P31  wrong", Target::Page(page)),
            Some("Because."),
        ),
    )
    .await
    .unwrap();
    let date = utc_date(NOW);
    assert_eq!(
        rows(
            &client,
            &format!("SELECT title FROM view.page WHERE page_id = {}", a.thread)
        )
        .await,
        [format!("{date}/Why is P31 wrong")]
    );
    assert_eq!(
        rows(&client, "SELECT ns::text || ':' || title || ':' || content_model FROM view.page WHERE content_model = 'triplespace-talk'").await,
        ["5:Scope:triplespace-talk"]
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.talk_page").await,
        1
    );
    assert_eq!(
        rows(
            &client,
            "SELECT subject || ':' || status || ':' || post_count FROM view.thread"
        )
        .await,
        ["Why is P31 wrong:open:1"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.thread_attachment WHERE home"
        )
        .await,
        1
    );

    // The same subject the same day takes a suffix, and the same talk page.
    let b = put(
        &store,
        &p,
        &request("librarybase:7", None),
        by_editor(
            None,
            create("Why is P31 wrong", Target::Page(page)),
            Some("Again."),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        rows(
            &client,
            &format!("SELECT title FROM view.page WHERE page_id = {}", b.thread)
        )
        .await,
        [format!("{date}/Why is P31 wrong (2)")]
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.talk_page").await,
        1
    );

    // Replies need no base; the latest status wins.
    let r1 = put(
        &store,
        &p,
        &request("librarybase:2", None),
        by_editor(Some(a.thread), post(Some(a.revid), None), Some("Really?")),
    )
    .await
    .unwrap();
    let r2 = put(
        &store,
        &p,
        &request("librarybase:7", None),
        by_editor(
            Some(a.thread),
            post(Some(r1.revid), Some("resolved")),
            Some("Fixed."),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        rows(
            &client,
            &format!(
                "SELECT status || ':' || post_count FROM view.thread WHERE page_id = {}",
                a.thread
            )
        )
        .await,
        ["resolved:3"]
    );
    assert_eq!(
        rows(
            &client,
            &format!(
                "SELECT parent_revid::text FROM view.post WHERE revid = {}",
                r2.revid
            )
        )
        .await,
        [r1.revid.to_string()]
    );

    // Refusals: a parent outside the thread, an editor's heading, an edit without a base,
    // an edit on a stale base.
    assert!(matches!(
        put(
            &store,
            &p,
            &request("librarybase:2", None),
            by_editor(Some(a.thread), post(Some(b.revid), None), Some("x"))
        )
        .await,
        Err(ThreadError::Invalid(Invalid::NotAPost(_)))
    ));
    let heading = ThreadOp::Post {
        in_reply_to: None,
        status: None,
        media_type: None,
        heading: Some(3),
        collapse: None,
        uncollapse: None,
        imported_from: None,
    };
    assert!(matches!(
        put(
            &store,
            &p,
            &request("librarybase:2", None),
            by_editor(Some(a.thread), heading, Some("== x =="))
        )
        .await,
        Err(ThreadError::Invalid(Invalid::ImportOnly("heading")))
    ));
    let edit = ThreadOp::Edit {
        object: r1.revid,
        media_type: None,
    };
    assert!(matches!(
        put(
            &store,
            &p,
            &request("librarybase:2", None),
            by_editor(Some(a.thread), edit.clone(), Some("Really!"))
        )
        .await,
        Err(ThreadError::NeedsBase("edit"))
    ));
    assert!(matches!(
        put(
            &store,
            &p,
            &request("librarybase:2", Some(r1.written.offset())),
            by_editor(Some(a.thread), edit.clone(), Some("Really!"))
        )
        .await,
        Err(ThreadError::Conflict { .. })
    ));
    let e = put(
        &store,
        &p,
        &request("librarybase:2", Some(r2.written.offset())),
        by_editor(Some(a.thread), edit, Some("Really!")),
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT latest_offset FROM view.post WHERE revid = {}",
                r1.revid
            )
        )
        .await,
        i64::try_from(e.written.offset()).unwrap()
    );

    // Pin on the home; list on the account's talk page; unlist.
    let mut base = e.written.offset();
    let pin = put(
        &store,
        &p,
        &request("librarybase:7", Some(base)),
        by_editor(
            Some(a.thread),
            ThreadOp::Pin {
                target: Target::Page(page),
            },
            None,
        ),
    )
    .await
    .unwrap();
    base = pin.written.offset();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.thread_attachment WHERE pinned IS NOT NULL"
        )
        .await,
        1
    );
    assert!(matches!(
        put(
            &store,
            &p,
            &request("librarybase:7", Some(base)),
            by_editor(
                Some(a.thread),
                ThreadOp::Attach {
                    target: Target::Actor("librarybase:7".into())
                },
                None
            )
        )
        .await,
        Err(ThreadError::Invalid(Invalid::ActorListing))
    ));
    let other = Target::Page(b.thread);
    let at = put(
        &store,
        &p,
        &request("librarybase:7", Some(base)),
        by_editor(
            Some(a.thread),
            ThreadOp::Attach {
                target: other.clone(),
            },
            None,
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT count(*) FROM view.thread_attachment WHERE thread_id = {}",
                a.thread
            )
        )
        .await,
        2
    );
    let detached = put(
        &store,
        &p,
        &request("librarybase:7", Some(at.written.offset())),
        by_editor(Some(a.thread), ThreadOp::Detach { target: other }, None),
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT count(*) FROM view.thread_attachment WHERE thread_id = {}",
                a.thread
            )
        )
        .await,
        1
    );

    // A rename keeps its own title when the subject normalizes the same, and keeps the date.
    let kept = put(
        &store,
        &p,
        &request("librarybase:7", Some(detached.written.offset())),
        by_editor(
            Some(a.thread),
            ThreadOp::Rename {
                name: "why is P31 wrong".into(),
            },
            None,
        ),
    )
    .await
    .unwrap();
    put(
        &store,
        &p,
        &request("librarybase:7", Some(kept.written.offset())),
        by_editor(
            Some(a.thread),
            ThreadOp::Rename {
                name: "P31 is fine".into(),
            },
            None,
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        rows(&client, "SELECT title FROM view.page WHERE ns = 214").await,
        [
            format!("{date}/P31 is fine"),
            format!("{date}/Why is P31 wrong (2)")
        ]
    );

    // A user talk page: the actor's name in User talk.
    put(
        &store,
        &p,
        &request("librarybase:2", None),
        by_editor(
            None,
            create("Welcome", Target::Actor("librarybase:7".into())),
            Some("Hello!"),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        rows(&client, "SELECT ns::text || ':' || title FROM view.page WHERE content_model = 'triplespace-talk' ORDER BY ns").await,
        ["3:Editor", "5:Scope"]
    );
    assert!(matches!(
        put(
            &store,
            &p,
            &request("librarybase:2", None),
            by_editor(
                None,
                create("Ghost", Target::Actor("librarybase:99".into())),
                Some("?")
            )
        )
        .await,
        Err(ThreadError::Invalid(Invalid::NoTarget(_)))
    ));
    cleanup().await;
}

#[tokio::test]
async fn a_converted_talk_page_is_written_through_the_same_path() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    add_editor(&client).await;

    let text = "{{Talk header}}\n== Merge? ==\n{{atop|result=Merged. [[User:C|C]] 13:00, 5 June 2025 (UTC)}}\nShould we? [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:Yes. [[User:B|B]] 12:05, 5 June 2025 (UTC)\n{{abot}}";
    let (markup, site) = (
        scatter_talk::Markup::embedded(),
        scatter_talk::Site::english(),
    );
    let conv = scatter_talk::convert(text, markup, &site, &scatter_talk::Options::default());
    let source = scatter_talk_import::Source {
        repository: Some("librarybase".into()),
        page: "User talk:Editor".into(),
        revision: Some(41),
        archive: None,
    };
    let home = Target::Actor("librarybase:7".into());
    for planned in scatter_talk_import::plan(&conv, "User talk:Editor", &source) {
        let date = planned.date.map(|d| {
            let (y, m, dd, _, _) = d.civil();
            format!("{y:04}-{m:02}-{dd:02}")
        });
        let (mut thread, mut revids) = (None, Vec::new());
        for (k, r) in planned.records.iter().enumerate() {
            let w = ThreadWrite {
                thread,
                op: planned.op(k, &home, &revids),
                text: Some(r.text.clone()),
                writer: Writer::Import,
                limits: Limits::default(),
                date: date.clone(),
                signer: None,
            };
            let out = put(&store, &p, &request("librarybase:2", None), w)
                .await
                .unwrap();
            thread = Some(out.thread);
            revids.push(out.revid);
        }
    }
    assert_eq!(
        rows(
            &client,
            "SELECT title FROM view.page WHERE content_model = 'triplespace-thread'"
        )
        .await,
        [
            "2025-06-05/Merge?".to_string(),
            format!("{}/User talk:Editor: talk page header", utc_date(NOW))
        ]
    );
    assert_eq!(
        rows(
            &client,
            "SELECT subject || ':' || status || ':' || post_count FROM view.thread"
        )
        .await,
        [
            "Merge?:closed:3",
            "User talk:Editor: talk page header:open:1"
        ]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.thread_attachment WHERE pinned IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(
        rows(
            &client,
            "SELECT dt_name FROM view.post WHERE dt_name IS NOT NULL"
        )
        .await,
        [
            "c-A-20250605120000",
            "c-B-20250605120500",
            "c-C-20250605130000"
        ]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.post WHERE original IS NOT NULL"
        )
        .await,
        3
    );
    cleanup().await;
}
