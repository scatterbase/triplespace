//! `instance setup` leading into `tenant setup`, end to end against Postgres (0018 A36):
//! the answers create the instance and its primary tenant as `instance create` would, then
//! a second tenant, which is refused its host until it is free and needs the auth base;
//! each owner's password is set from the answer. Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

use tokio_postgres::{Config, NoTls};
use triplespace_cli::setup::{self, Scripted};

const DB: &str = "tssetup_test";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
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

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scripted session, read top to bottom
async fn instance_setup_creates_the_instance_then_more_tenants() {
    if admin_config().is_none() {
        eprintln!("skipping: TRIPLESPACE_TEST_DATABASE_URL is not set");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssetup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file = dir.join("triplespace.key");
    let _ = std::fs::remove_file(&key_file);

    let mut p = Scripted::new([
        // The database: one that cannot be reached is asked again.
        "postgres://nobody@127.0.0.1:1/none",
        database.as_str(),
        "",
        // The instance.
        "myfarm",
        "",
        // Its primary tenant.
        "",
        "alpha",
        "alpha.example",
        "Alpha Wiki",
        "Alice",
        "alice-secret-1234",
        "alice-secret-1234",
        // The farm base, the primary's; then the summary.
        "",
        "y",
        // Another wiki: yes, by `tenant setup`, its database and key already known.
        "y",
        "",
        "",
        // The auth host, which cannot be the primary's.
        "alpha.example",
        "auth.example",
        "",
        "beta",
        "alpha.example",
        "beta.example",
        "",
        "Bob",
        "",
        "y",
        "n",
    ]);
    setup::instance_setup(
        &mut p,
        setup::InstanceSetup {
            database: None,
            key_file: key_file.clone(),
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{e:#}\n{}", p.transcript.join("\n")));
    assert!(p.finished(), "{}", p.transcript.join("\n"));
    let warnings = p.warnings();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings[0].starts_with("cannot use that database"));
    assert!(warnings[1].contains("the host of `alpha`"));
    assert!(warnings[2].contains("the host of `alpha`"));
    assert!(
        p.transcript
            .iter()
            .any(|l| l.starts_with("triplespace instance create")),
        "the summary shows the equivalent command"
    );

    let client = triplespace_cli::common::connect(&database).await.unwrap();
    let farm = triplespace_cli::common::farm_of(&client).await.unwrap();
    assert_eq!(farm.slug, "myfarm");
    assert_eq!(farm.base, "https://alpha.example");
    let tenants = triplespace_projections::tenants::tenants(&client)
        .await
        .unwrap();
    let mut slugs: Vec<_> = tenants
        .iter()
        .map(|t| (t.slug.as_str(), t.base.as_str()))
        .collect();
    slugs.sort_unstable();
    assert_eq!(
        slugs,
        [
            ("alpha", "https://alpha.example"),
            ("beta", "https://beta.example")
        ]
    );
    assert_eq!(
        triplespace_projections::tenants::primary(&client)
            .await
            .unwrap()
            .as_deref(),
        Some("alpha")
    );
    // Alice's password was set from her answer; Bob, who gave none, has none yet.
    let passwords: i64 = client
        .query_one("SELECT count(*) FROM private.password", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(passwords, 1);

    // Setting up again on the same database finds the instance and goes on to a tenant.
    let mut again = Scripted::new([database.as_str(), "n"]);
    let refused = setup::instance_setup(
        &mut again,
        setup::InstanceSetup {
            database: None,
            key_file: key_file.clone(),
        },
    )
    .await;
    assert!(
        refused.is_err(),
        "the script ends before a database is chosen"
    );
    assert_eq!(
        again.warnings(),
        ["this database already holds an instance"]
    );
}
