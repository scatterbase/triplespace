//! Where a load ended, as the follower continues from it (0092 §5, A4): a load moves the
//! recorded end only forward. Named by `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::sync::atomic::{AtomicU32, Ordering};

use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::follow::{KIND_PROVIDER, position, record_load};

static COUNTER: AtomicU32 = AtomicU32::new(0);

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

async fn fresh(admin: Config) -> Client {
    let name = format!(
        "tsfollowpos_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    connect(&admin)
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(&name);
    let mut config = admin;
    config.dbname(&name);
    let mut client = connect(&config).await;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .expect("migrate");
    client
}

const LOAD: u64 = 1_793_101_295; // 2026-10-27T11:54:55Z
const SMALL: u64 = 1_785_151_024; // 2026-07-27T11:17:04Z
const LATER: u64 = 1_794_000_000;

async fn at(client: &Client) -> triplespace_api_ingest::follow::Position {
    position(client, "", KIND_PROVIDER, "wikidata")
        .await
        .unwrap()
}

#[tokio::test]
async fn a_load_moves_the_recorded_end_only_forward() {
    let Some(admin) = admin_config() else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let client = fresh(admin).await;

    assert!(
        record_load(&client, "", KIND_PROVIDER, "wikidata", LOAD)
            .await
            .unwrap()
    );
    assert_eq!(at(&client).await.loaded_to, Some(LOAD));

    // The follower runs on from the load.
    client
        .execute(
            "UPDATE ops.repo_cursor SET rc_time = to_timestamp($1::bigint), handed_over = true",
            &[&i64::try_from(LOAD + 3600).unwrap()],
        )
        .await
        .unwrap();

    // A sync of a few older entities, and the same load again, change nothing.
    for end in [SMALL, LOAD] {
        assert!(
            !record_load(&client, "", KIND_PROVIDER, "wikidata", end)
                .await
                .unwrap()
        );
        let p = at(&client).await;
        assert_eq!(p.loaded_to, Some(LOAD), "the end stays");
        assert_eq!(
            p.rc_time,
            Some(LOAD + 3600),
            "the follower's position stays"
        );
    }

    // A later load moves the end and hands the follower its position again.
    assert!(
        record_load(&client, "", KIND_PROVIDER, "wikidata", LATER)
            .await
            .unwrap()
    );
    let p = at(&client).await;
    assert_eq!(p.loaded_to, Some(LATER));
    assert_eq!(p.rc_time, None);
}

/// The follower's open job is kept with its position (0092 A6), and a load leaves it.
#[tokio::test]
async fn the_open_job_is_kept_with_the_position() {
    let Some(admin) = admin_config() else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let client = fresh(admin).await;
    assert!(
        record_load(&client, "", KIND_PROVIDER, "wikidata", LOAD)
            .await
            .unwrap()
    );
    assert_eq!(at(&client).await.job, None);
    let mut job = scatter_ingest::FollowJob {
        id: 42,
        day: 20_768,
        batches: 3,
        rows: 1_500,
        ..Default::default()
    };
    job.counts.merged = 700;
    client
        .execute(
            "UPDATE ops.repo_cursor SET job = $1",
            &[&serde_json::to_value(&job).unwrap()],
        )
        .await
        .unwrap();
    assert_eq!(at(&client).await.job.as_ref(), Some(&job));
    assert!(
        record_load(&client, "", KIND_PROVIDER, "wikidata", LATER)
            .await
            .unwrap()
    );
    assert_eq!(
        at(&client).await.job,
        Some(job),
        "a load leaves the open job"
    );
}
