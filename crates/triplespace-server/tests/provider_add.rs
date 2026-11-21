//! `instance provider add` (0018 §5): a provider's mirror opened on a running instance,
//! as `instance create --provider` opens it at creation, with its `graph:` record, and
//! its follower queued. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use tokio_postgres::{Config, NoTls};
use triplespace_cli::{instance, provider};

const DB: &str = "tsprovideradd_test";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {DB}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(DB);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

async fn count(database: &str, sql: &str) -> i64 {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.query_one(sql, &[]).await.unwrap().get(0)
}

fn add(
    database: &str,
    key_file: &std::path::Path,
    slug: &str,
    no_follow: bool,
) -> provider::Provider {
    provider::Provider::Add(provider::Add {
        database: database.to_string(),
        key_file: key_file.to_path_buf(),
        provider: slug.to_string(),
        no_follow,
    })
}

// 0018 §5.
#[tokio::test]
async fn a_mirror_is_opened_on_a_running_instance_and_its_follower_queued() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsprovideradd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file = dir.join("triplespace.key");
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: "reader".into(),
        base: "https://reader.example".into(),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(1),
        owner_name: "Reader".into(),
        owner_password_file: None,
    })
    .await
    .expect("instance create");
    let partitions =
        "SELECT count(*) FROM log.partition WHERE tenant IS NULL AND name = 'mirror/wikidata'";
    let graphs = "SELECT count(*) FROM view.registry WHERE tenant = '' AND kind = 'graph' AND code = 'mirror/wikidata'";
    let followers = "SELECT count(*) FROM ops.job_queue WHERE tenant = '' AND kind = 'follow'
                     AND params->>'provider' = 'wikidata'";
    assert_eq!(count(&database, partitions).await, 0);

    provider::run(add(&database, &key_file, "wikidata", false))
        .await
        .expect("provider add");
    assert_eq!(
        count(&database, partitions).await,
        1,
        "the partition is opened"
    );
    assert_eq!(
        count(&database, graphs).await,
        1,
        "its `graph:` record is projected"
    );
    assert_eq!(
        count(&database, followers).await,
        1,
        "its follower is queued"
    );

    // Opened once: a second add is refused, and queues nothing more.
    let again = provider::run(add(&database, &key_file, "wikidata", false)).await;
    assert!(
        again
            .as_ref()
            .is_err_and(|e| e.to_string().contains("already has")),
        "{again:?}"
    );
    assert_eq!(count(&database, followers).await, 1);

    // A provider the registry does not know is refused.
    let unknown = provider::run(add(&database, &key_file, "nosuchwiki", false)).await;
    assert!(
        unknown
            .as_ref()
            .is_err_and(|e| e.to_string().contains("not a registered provider")),
        "{unknown:?}"
    );

    // `--no-follow` opens the mirror alone.
    provider::run(add(&database, &key_file, "openalex", true))
        .await
        .expect("provider add --no-follow");
    assert_eq!(
        count(
            &database,
            "SELECT count(*) FROM log.partition WHERE tenant IS NULL AND name = 'mirror/openalex'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &database,
            "SELECT count(*) FROM ops.job_queue WHERE kind = 'follow'"
        )
        .await,
        1
    );
}
