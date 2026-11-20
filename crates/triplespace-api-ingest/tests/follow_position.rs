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
    fresh_named(admin).await.0
}

/// A migrated database, and its name.
async fn fresh_named(admin: Config) -> (Client, String) {
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
    (client, name)
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
    assert!(at(&client).await.state.is_empty());
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
    assert_eq!(
        at(&client).await.state.open.as_ref(),
        Some(&job),
        "a position saved before 0092 A8 holds the open job alone"
    );
    // 0092 A8: a closed day's job still compacting is kept beside the open one.
    let state = scatter_ingest::FollowState {
        open: Some(scatter_ingest::FollowJob {
            id: 43,
            day: 20_769,
            ..Default::default()
        }),
        closing: Some(scatter_ingest::Closing {
            job: job.clone(),
            partition: 7,
            to: 90_000,
            at: 40_000,
            removed: 1_234,
        }),
    };
    client
        .execute(
            "UPDATE ops.repo_cursor SET job = $1",
            &[&serde_json::to_value(&state).unwrap()],
        )
        .await
        .unwrap();
    assert_eq!(at(&client).await.state, state);
    assert!(
        record_load(&client, "", KIND_PROVIDER, "wikidata", LATER)
            .await
            .unwrap()
    );
    assert_eq!(at(&client).await.state, state, "a load leaves the jobs");
}

/// 0092 A10: the follower's row is written in the store's unit of work, so it commits with
/// what the unit of work wrote, and a unit of work rolled back leaves it as it was.
#[tokio::test]
async fn the_position_commits_with_the_unit_of_work() {
    use scatter_ingest::SavePosition;
    use scatter_projection::Backend;
    use triplespace_api_ingest::PgIngest;
    use triplespace_api_ingest::follow::{Saved, Saver};

    let Some(admin) = admin_config() else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (client, name) = fresh_named(admin).await;
    assert!(
        record_load(&client, "", KIND_PROVIDER, "wikidata", LOAD)
            .await
            .unwrap()
    );
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    let url = url
        .rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap();
    let pool = triplespace_db::pool::pool(&url, 2).unwrap();
    let store = PgIngest::new(triplespace_projections::PgBackend::new(pool));
    let saver = Saver::new("", KIND_PROVIDER, "wikidata", || LATER);
    let state = |id| scatter_ingest::FollowState {
        open: Some(scatter_ingest::FollowJob {
            id,
            day: 20_776,
            ..Default::default()
        }),
        closing: None,
    };

    // Rolled back: neither the jobs nor the position.
    let mut cx = store.begin().await.unwrap();
    saver.save(&mut cx, None, &state(1)).await.unwrap();
    saver
        .save(&mut cx, Some(&Saved::at(LATER)), &state(1))
        .await
        .unwrap();
    store.rollback(cx).await.unwrap();
    let p = at(&client).await;
    assert!(p.state.is_empty());
    assert_eq!(p.rc_time, None);

    // The jobs alone leave the position; the jobs with a position move both.
    let mut cx = store.begin().await.unwrap();
    saver.save(&mut cx, None, &state(2)).await.unwrap();
    store.commit(cx).await.unwrap();
    let p = at(&client).await;
    assert_eq!(p.state, state(2));
    assert_eq!(p.rc_time, None);
    let mut cx = store.begin().await.unwrap();
    saver
        .save(&mut cx, Some(&Saved::at(LATER)), &state(3))
        .await
        .unwrap();
    store.commit(cx).await.unwrap();
    let p = at(&client).await;
    assert_eq!(p.state, state(3));
    assert_eq!(p.rc_time, Some(LATER));
}

/// 0092 A11: a lost position is rebuilt from the newest revision the follower's graph
/// holds, read from the version cursor; a graph with nothing in it has none, and another
/// graph's or tenant's revisions do not count.
#[tokio::test]
async fn the_newest_revision_a_graph_holds_rebuilds_a_lost_position() {
    use triplespace_api_ingest::follow::newest_revision;

    let Some(admin) = admin_config() else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let client = fresh(admin).await;
    assert_eq!(
        newest_revision(&client, "", "mirror/wikidata")
            .await
            .unwrap(),
        None
    );
    for (tenant, entity, graph, version) in [
        ("", "WDQ1", "mirror/wikidata", Some("900")),
        ("", "WDQ2", "mirror/wikidata", Some("2572371073")),
        ("", "WDQ3", "mirror/wikidata", None),
        ("", "OAW1", "mirror/openalex", Some("2026-11-01")),
        ("", "Q1", "mirror/other", Some("9999999999")),
        ("lb", "WDQ4", "mirror/wikidata", Some("9999999999")),
    ] {
        client
            .execute(
                "INSERT INTO view.entity_source (tenant, entity_id, graph, \"offset\",
                                                 upstream_version, content_hash, size, synced_at)
                 VALUES ($1, $2, $3, 0, $4, '\\x00', 1, now())",
                &[&tenant, &entity, &graph, &version],
            )
            .await
            .unwrap();
    }
    assert_eq!(
        newest_revision(&client, "", "mirror/wikidata")
            .await
            .unwrap(),
        Some(2_572_371_073)
    );
    assert_eq!(
        newest_revision(&client, "", "mirror/openalex")
            .await
            .unwrap(),
        None,
        "a version that is not a revision ID"
    );
}
