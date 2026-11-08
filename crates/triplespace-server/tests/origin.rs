//! A partition's origin line is fixed at its creation and read from its registry row
//! (01 §4.2; 0006 A29): a tenant aliased to a new host before its first checkpoint still
//! signs under the host it was created under, and every checkpoint, manifest and bundle
//! carries that origin. A partition created before the row recorded it keeps its first
//! checkpoint's origin, and one never signed is derived from the current host, the best
//! that can be done. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::PathBuf;

use scatter_integrity::checkpoint::SignedCheckpoint;
use tokio_postgres::{Config, NoTls};
use triplespace_cli::log_bundle::{Checkpoint, Export, Scope, checkpoint, export};
use triplespace_cli::{instance, settings};

const DB: &str = "tsorigin_test";
const TENANT: &str = "wiki";
const HOST: &str = "old.example";
const MOVED: &str = "new.example";

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

/// An instance whose primary tenant lives at `HOST`, the farm base defaulting to it.
async fn setup() -> (String, PathBuf) {
    let database = fresh_database(DB).await;
    let dir = std::env::temp_dir().join(format!("{DB}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    let key_file = dir.join("triplespace.key");
    let _ = std::fs::remove_file(&key_file);
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    (database, key_file)
}

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.query(sql, &[]).await.unwrap()
}

async fn execute(database: &str, sql: &str) {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.batch_execute(sql).await.unwrap();
}

/// The origin a partition of `tenant` (NULL for an instance one) named `name` was created
/// with under `host`, as 01 §4.2 fixes it.
fn expected(host: &str, tenant: Option<&str>, name: &str) -> String {
    match tenant {
        Some(_) => format!("{host}/log/{name}"),
        None => format!("{host}/instance/log/{name}"),
    }
}

/// Appends a record to the tenant's `config` partition, so that it has grown.
async fn grow(database: &str, value: &str) {
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.to_string(),
            tenant: Some(TENANT.into()),
            instance: false,
        },
        key: "challenge.bits".into(),
        value: value.into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .expect("settings set");
}

/// Every signed note of the tenant's partitions, with the partition's name: checkpoints
/// and segment manifests.
async fn signed(database: &str) -> Vec<(String, SignedCheckpoint)> {
    let mut out = Vec::new();
    for sql in [
        "SELECT p.name, c.signed_note FROM log.checkpoint c JOIN log.partition p USING (partition)
         WHERE p.tenant = 'wiki' ORDER BY p.name, c.tree_size",
        "SELECT p.name, m.signed_note FROM log.segment_manifest m JOIN log.partition p USING (partition)
         WHERE p.tenant = 'wiki' ORDER BY p.name, m.segment",
    ] {
        for r in query(database, sql).await {
            let note: String = r.get(1);
            out.push((r.get(0), SignedCheckpoint::parse(note.as_bytes()).unwrap()));
        }
    }
    out
}

// 01 §4.2; 0006 A29: the origin is fixed at creation, recorded, and read from there.
#[tokio::test]
async fn origin_is_fixed_at_creation_not_at_first_checkpoint() {
    if admin_config().is_none() {
        eprintln!("skipping: TRIPLESPACE_TEST_DATABASE_URL is not set");
        return;
    }
    let (database, key_file) = setup().await;

    // 1. Every partition's row records its origin: a tenant partition under the tenant's
    //    host, an instance partition under the farm's, which defaulted to the tenant's.
    let rows = query(
        &database,
        "SELECT tenant, name, origin FROM log.partition ORDER BY tenant NULLS FIRST, name",
    )
    .await;
    assert!(rows.len() >= 8, "{} partitions", rows.len());
    for r in &rows {
        let tenant: Option<String> = r.get(0);
        let name: String = r.get(1);
        let origin: Option<String> = r.get(2);
        assert_eq!(
            origin.as_deref(),
            Some(expected(HOST, tenant.as_deref(), &name).as_str()),
            "{tenant:?}/{name}"
        );
    }

    // 2. The tenant is aliased to a new host before anything is signed: its current base
    //    changes, the recorded origins do not.
    execute(
        &database,
        &format!(
            "UPDATE view.registry SET config = jsonb_set(config, '{{base}}', '\"https://{MOVED}\"')
             WHERE tenant = '' AND kind = 'tenant' AND code = '{TENANT}'"
        ),
    )
    .await;
    let base: String = query(
        &database,
        "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = 'wiki'",
    )
    .await[0]
        .get(0);
    assert_eq!(base, format!("https://{MOVED}"), "the alias took");

    // 3. The first checkpoints are signed under the host at creation, not the current one.
    let args = Checkpoint {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: None,
    };
    let done = checkpoint(&args).await.unwrap();
    assert!(done.checkpoints > 0, "{done:?}");
    let notes = signed(&database).await;
    assert!(!notes.is_empty());
    for (name, note) in &notes {
        let origin = note
            .checkpoint
            .origin
            .split("/segment/")
            .next()
            .unwrap()
            .to_string();
        assert_eq!(origin, expected(HOST, Some(TENANT), name), "{name}");
        assert!(!origin.contains(MOVED), "{name}: signed under the alias");
    }

    // 4. A bundle names each partition by the same origin.
    let out = std::env::temp_dir().join(format!("{DB}-bundle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let parts = export(&Export {
        database: database.clone(),
        scope: Scope::Backup,
        tenant: Some(TENANT.into()),
        out: out.clone(),
    })
    .await
    .unwrap();
    assert!(!parts.is_empty());
    for p in &parts {
        assert_eq!(
            p.origin,
            expected(HOST, Some(TENANT), &p.name),
            "{}",
            p.name
        );
    }
    let _ = std::fs::remove_dir_all(&out);

    // 5. A partition created before the row recorded its origin (the column NULL) keeps
    //    its first checkpoint's origin for every later one (0006 A29).
    execute(
        &database,
        "UPDATE log.partition SET origin = NULL WHERE tenant = 'wiki' AND name = 'config'",
    )
    .await;
    grow(&database, "8").await;
    // `config` grew, and the `log` partition with it, by the setting's log event.
    let done = checkpoint(&args).await.unwrap();
    assert!(done.checkpoints >= 1, "{done:?}: `config` grew");
    let config: Vec<_> = signed(&database)
        .await
        .into_iter()
        .filter(|(name, _)| name == "config")
        .collect();
    assert_eq!(config.len(), 2, "two checkpoints of `config`");
    assert!(config[1].1.checkpoint.size > config[0].1.checkpoint.size);
    assert_eq!(
        config[1].1.checkpoint.origin,
        expected(HOST, Some(TENANT), "config"),
        "the first checkpoint's origin, not the alias"
    );

    // 6. One never signed and never recorded can only be derived from the current host:
    //    the pre-A29 behaviour, kept for such partitions alone.
    execute(
        &database,
        "DELETE FROM log.checkpoint WHERE partition IN
           (SELECT partition FROM log.partition WHERE tenant = 'wiki' AND name = 'config')",
    )
    .await;
    grow(&database, "9").await;
    let done = checkpoint(&args).await.unwrap();
    assert!(done.checkpoints >= 1, "{done:?}");
    let config: Vec<_> = signed(&database)
        .await
        .into_iter()
        .filter(|(name, _)| name == "config")
        .collect();
    assert_eq!(config.len(), 1);
    assert_eq!(
        config[0].1.checkpoint.origin,
        expected(MOVED, Some(TENANT), "config"),
        "derived from the current host, the best that can be done"
    );
}
