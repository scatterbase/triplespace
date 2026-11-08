//! Bootstrap mode over Postgres (0013 §9, A70; 05 §3.8): `triplespace sync` into an empty
//! instance mirror with no key map loads in blocks by binary `COPY`, from records prepared
//! on worker threads, and writes what the ordinary sync writes. The same JSON dump goes
//! into fresh instances, each way, and the mirror's records, its tree and the `view` rows
//! a mirror fills are compared; then a bootstrapped instance is rebuilt from its log, which
//! reads every `COPY`-written record back from its bytes. Several blocks written at once
//! (A82) write what one block at a time writes, and a load that fails with blocks in
//! flight leaves a prefix of whole blocks, which a rerun completes.

use std::path::PathBuf;

use tokio_postgres::{Config, NoTls};
use triplespace_cli::{instance, status, sync};

const HOST: &str = "reader.example";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database(name: &str) -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(name);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap()
}

/// A query's rows, each as its columns joined by `|`.
async fn rows(database: &str, sql: &str) -> Vec<String> {
    let config: Config = database.parse().unwrap();
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .simple_query(sql)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|m| match m {
            tokio_postgres::SimpleQueryMessage::Row(r) => Some(
                (0..r.len())
                    .map(|i| r.get(i).unwrap_or("∅").to_string())
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            _ => None,
        })
        .collect()
}

/// The internetdomains JSON fixture, six entities.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/internetdomains.json")
}

/// A JSON dump of `n` items shaped like the fixture's, each with a label and a statement,
/// so that a load of it takes many blocks. With `repeat`, the item at that line names the
/// first item's subject again, as a dump that lists an entity twice does.
fn generated(n: u32, repeat: Option<u32>) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tsbootstrap-{}-gen{n}-{}.json",
        std::process::id(),
        repeat.unwrap_or(0)
    ));
    let mut lines = vec!["[".to_string()];
    for i in 1..=n {
        let q = if repeat == Some(i) { 1001 } else { 1000 + i };
        let comma = if i < n { "," } else { "" };
        lines.push(format!(
            r#"{{"type":"item","id":"Q{q}","labels":{{"en":{{"language":"en","value":"site{q}.example"}}}},"descriptions":{{}},"aliases":{{}},"claims":{{"P1":[{{"mainsnak":{{"snaktype":"value","property":"P1","datatype":"string","datavalue":{{"value":"site{q}.example","type":"string"}}}},"type":"statement","rank":"normal","id":"Q{q}$00000000-0000-0000-0000-{q:012X}"}}]}},"sitelinks":{{}},"lastrevid":{q},"modified":"2026-10-01T00:00:00Z"}}{comma}"#
        ));
    }
    lines.push("]".into());
    std::fs::write(&path, lines.join("\n")).unwrap();
    path
}

/// How an instance is loaded.
struct Load {
    dump: PathBuf,
    no_bootstrap: bool,
    writers: usize,
    batch: usize,
}

/// An instance mirroring Librarybase (which has no key map), created empty.
async fn created(name: &str) -> String {
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("tsbootstrap-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "reader".into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec!["librarybase".into()],
        adopt: None,
        owner: Some(1),
        owner_name: "Owner".into(),
        owner_password_file: None,
    })
    .await
    .expect("instance create");
    database
}

/// Syncs a dump into an instance, read as Librarybase's.
async fn sync_into(database: &str, load: &Load) -> anyhow::Result<()> {
    sync::run(sync::Sync {
        database: database.to_string(),
        provider: Some("librarybase".into()),
        tenant: None,
        source: None,
        dump: load.dump.clone(),
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: load.batch,
        rejects: None,
        no_bootstrap: load.no_bootstrap,
        writers: load.writers,
    })
    .await
}

/// A fresh instance with a dump loaded.
async fn loaded(name: &str, load: &Load) -> String {
    let database = created(name).await;
    sync_into(&database, load).await.expect("sync");
    database
}

const MIRROR: &str = "(SELECT partition FROM log.partition WHERE name = 'mirror/librarybase')";

/// What both loads must agree on: the mirror's records, its size and tree shape, and the
/// view rows a mirror fills.
fn compared() -> Vec<(&'static str, String)> {
    vec![
        ("records", format!("SELECT \"offset\", key, revid, page_id, payload_type FROM log.record WHERE partition = {MIRROR} ORDER BY \"offset\"")),
        ("size", "SELECT next_offset FROM log.partition WHERE name = 'mirror/librarybase'".into()),
        ("tree", format!("SELECT level, count(*) FROM log.merkle_node WHERE partition = {MIRROR} GROUP BY level ORDER BY level")),
        ("entity", "SELECT tenant, id, type, page_id, resolved_kind, resolved_size, deleted FROM view.entity ORDER BY tenant, id".into()),
        ("term", "SELECT tenant, entity_id, kind, lang, ordinal, text FROM view.term ORDER BY 1, 2, 3, 4, 5".into()),
        ("identifier", "SELECT tenant, property, value_key, entity_id, graph, statement_id FROM view.identifier ORDER BY 1, 2, 3, 4, 5".into()),
        ("entity_source", "SELECT tenant, entity_id, graph, \"offset\", upstream_version, prev_upstream, size FROM view.entity_source ORDER BY 1, 2, 3".into()),
        ("job counts", "SELECT counts::text FROM view.job ORDER BY job_id".into()),
    ]
}

#[tokio::test]
async fn a_bootstrap_load_writes_what_a_sync_writes_over_postgres() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let load = |no_bootstrap, writers| Load {
        dump: fixture(),
        no_bootstrap,
        writers,
        batch: 2,
    };
    let ordinary = loaded("tsbootstrap_ordinary", &load(true, 1)).await;
    let bootstrapped = loaded("tsbootstrap_loaded", &load(false, sync::WRITERS)).await;
    // The load took bootstrap mode, and the other did not.
    let job = "SELECT coalesce(params->>'bootstrap', '-') FROM view.job";
    assert_eq!(rows(&bootstrapped, job).await, ["true"]);
    assert_eq!(rows(&ordinary, job).await, ["-"]);
    for (what, sql) in compared() {
        let a = rows(&ordinary, &sql).await;
        // The fixture has no external identifiers; every other table has rows.
        assert!(what == "identifier" || !a.is_empty(), "{what}: no rows");
        assert_eq!(rows(&bootstrapped, &sql).await, a, "{what}");
    }
    // Every stored leaf is its record's, and the tree folds over them.
    let leaves = format!(
        "SELECT count(*) FROM log.record r JOIN log.merkle_node m ON m.partition = r.partition
           AND m.level = 0 AND m.index = r.\"offset\" AND m.hash = r.leaf WHERE r.partition = {MIRROR}"
    );
    let records = format!("SELECT count(*) FROM log.record WHERE partition = {MIRROR}");
    assert_eq!(
        rows(&bootstrapped, &leaves).await,
        rows(&bootstrapped, &records).await
    );
    // A rebuild reads every record back from its bytes and gives the same views.
    let views: Vec<String> = compared().into_iter().skip(3).map(|(_, sql)| sql).collect();
    let mut before = Vec::new();
    for sql in &views {
        before.push(rows(&bootstrapped, sql).await);
    }
    status::rebuild(status::Target {
        database: bootstrapped.clone(),
    })
    .await
    .expect("rebuild");
    for (sql, rows_before) in views.iter().zip(before) {
        assert_eq!(
            rows(&bootstrapped, sql).await,
            rows_before,
            "after the rebuild: {sql}"
        );
    }
}

/// The checks every committed state of a mirror must pass: its next offset is its record
/// count, the records sit at offsets 0 to the next offset, every leaf is its record's, every
/// level of the tree has the nodes a tree of that many leaves has, and every projection has
/// applied exactly the records there are.
async fn assert_prefix(database: &str) -> u64 {
    let one = |sql: String| async move { rows(database, &sql).await.remove(0) };
    let size: u64 =
        one("SELECT next_offset FROM log.partition WHERE name = 'mirror/librarybase'".into())
            .await
            .parse()
            .unwrap();
    let records = one(format!(
        "SELECT count(*) || '|' || coalesce(min(\"offset\"), 0) || '|' || coalesce(max(\"offset\") + 1, 0) FROM log.record WHERE partition = {MIRROR}"
    ))
    .await;
    assert_eq!(
        records,
        format!("{size}|0|{size}"),
        "records at offsets 0..{size}"
    );
    let leaves = one(format!(
        "SELECT count(*) FROM log.record r JOIN log.merkle_node m ON m.partition = r.partition
           AND m.level = 0 AND m.index = r.\"offset\" AND m.hash = r.leaf WHERE r.partition = {MIRROR}"
    ))
    .await;
    assert_eq!(leaves, size.to_string(), "every leaf is its record's");
    // A tree of `size` leaves has floor(size / 2^level) complete nodes at each level.
    let levels = rows(
        database,
        &format!("SELECT level, count(*) FROM log.merkle_node WHERE partition = {MIRROR} GROUP BY level ORDER BY level"),
    )
    .await;
    let mut expected = Vec::new();
    let mut level = 0;
    while size >> level > 0 {
        expected.push(format!("{level}|{}", size >> level));
        level += 1;
    }
    assert_eq!(levels, expected, "the tree of {size} leaves");
    let positions = rows(
        database,
        &format!("SELECT DISTINCT applied_offset FROM ops.projection_state WHERE partition = {MIRROR} AND applied_offset <> 0"),
    )
    .await;
    assert!(
        positions.is_empty() || positions == [size.to_string()],
        "every projection at {size}: {positions:?}"
    );
    let sources =
        one("SELECT count(*) FROM view.entity_source WHERE graph = 'mirror/librarybase'".into())
            .await;
    assert_eq!(sources, size.to_string(), "an entity source per record");
    size
}

/// Several blocks written at once (0013 §9, A82) write what one block at a time writes: the
/// same records, tree, views and job counts over a load of many blocks, with a subject
/// named twice or not, a tree that folds over its records, and a rebuild that gives the
/// same views.
#[tokio::test]
async fn several_blocks_at_once_write_what_one_at_a_time_writes() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let load = |writers, repeat| Load {
        dump: generated(300, repeat),
        no_bootstrap: false,
        writers,
        batch: 7,
    };
    // A dump naming one subject twice, in adjacent blocks: the later block waits for the
    // earlier one to commit (the race it avoids is not forced here; the rule is tested in
    // `scatter-ingest`), and the later state wins, as it does one block at a time.
    let one = loaded("tsbootstrap_one_repeat", &load(1, Some(8))).await;
    let four = loaded("tsbootstrap_four_repeat", &load(4, Some(8))).await;
    for (what, sql) in compared() {
        assert_eq!(
            rows(&four, &sql).await,
            rows(&one, &sql).await,
            "repeated: {what}"
        );
    }
    let one = loaded("tsbootstrap_one_writer", &load(1, None)).await;
    let four = loaded("tsbootstrap_four_writers", &load(4, None)).await;
    for (what, sql) in compared() {
        let a = rows(&one, &sql).await;
        assert!(what == "identifier" || !a.is_empty(), "{what}: no rows");
        assert_eq!(rows(&four, &sql).await, a, "{what}");
    }
    assert_eq!(assert_prefix(&four).await, 300);
    let views: Vec<String> = compared().into_iter().skip(3).map(|(_, sql)| sql).collect();
    let mut before = Vec::new();
    for sql in &views {
        before.push(rows(&four, sql).await);
    }
    status::rebuild(status::Target {
        database: four.clone(),
    })
    .await
    .expect("rebuild");
    for (sql, rows_before) in views.iter().zip(before) {
        assert_eq!(
            rows(&four, sql).await,
            rows_before,
            "after the rebuild: {sql}"
        );
    }
}

/// A load whose block fails with others in flight (0013 §9, A82) commits no block after
/// it: what it leaves is a prefix of whole blocks, consistent in its records, tree, views
/// and positions, and a rerun, now through the ordinary sync, completes it to what one
/// uninterrupted load writes.
#[tokio::test]
async fn a_failed_block_leaves_a_prefix_of_whole_blocks() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let load = Load {
        dump: generated(300, None),
        no_bootstrap: false,
        writers: 4,
        batch: 7,
    };
    let whole = loaded("tsbootstrap_whole", &load).await;
    let failed = created("tsbootstrap_failed").await;
    // The record at offset 150 cannot be written: the 22nd block fails, with others in
    // flight around it.
    let config: Config = failed.parse().unwrap();
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute(&format!(
            "CREATE FUNCTION pg_temp_fail() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
               IF NEW.partition = {MIRROR} AND NEW.\"offset\" = 150 THEN
                 RAISE EXCEPTION 'offset 150 refused by the test';
               END IF;
               RETURN NEW;
             END $$;
             CREATE TRIGGER fail_at_150 BEFORE INSERT ON log.record
               FOR EACH ROW EXECUTE FUNCTION pg_temp_fail();"
        ))
        .await
        .unwrap();
    // The load reports the server's message, not only the driver's "db error".
    let err = sync_into(&failed, &load).await.unwrap_err();
    assert!(
        format!("{err:#}").contains("offset 150 refused by the test"),
        "{err:#}"
    );
    let size = assert_prefix(&failed).await;
    // The failing block (the 22nd) was set going with at most three before it in flight,
    // so the 18 before those had committed.
    assert!(
        (126..=147).contains(&size) && size % 7 == 0,
        "a prefix of whole blocks before 150: {size}"
    );
    // No unit of work was left open.
    let open = rows(
        &failed,
        "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND state LIKE 'idle in transaction%'",
    )
    .await;
    assert_eq!(open, ["0"]);
    // The rerun goes through the ordinary sync, the partition no longer empty, and
    // completes the load.
    client
        .batch_execute("DROP TRIGGER fail_at_150 ON log.record; DROP FUNCTION pg_temp_fail()")
        .await
        .unwrap();
    sync_into(&failed, &load).await.expect("rerun");
    assert_eq!(assert_prefix(&failed).await, 300);
    // The same records and views as one uninterrupted load, but for page IDs: the blocks
    // that rolled back had reserved theirs, which are not reused.
    let compared_without_page_ids = [
        ("records", format!("SELECT \"offset\", key, revid, payload_type FROM log.record WHERE partition = {MIRROR} ORDER BY \"offset\"")),
        ("entity", "SELECT tenant, id, type, resolved_kind, resolved_size, deleted FROM view.entity ORDER BY tenant, id".into()),
        ("term", "SELECT tenant, entity_id, kind, lang, ordinal, text FROM view.term ORDER BY 1, 2, 3, 4, 5".into()),
        ("identifier", "SELECT tenant, property, value_key, entity_id, graph, statement_id FROM view.identifier ORDER BY 1, 2, 3, 4, 5".into()),
        ("entity_source", "SELECT tenant, entity_id, graph, \"offset\", upstream_version, prev_upstream, size FROM view.entity_source ORDER BY 1, 2, 3".into()),
    ];
    for (what, sql) in compared_without_page_ids {
        assert_eq!(
            rows(&failed, &sql).await,
            rows(&whole, &sql).await,
            "{what}"
        );
    }
}
