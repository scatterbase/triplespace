//! The queue's `adopt`, `sync` and `follow` jobs (0104 §4, A9): what the command line
//! queues with `triplespace job start`, a runner runs from the row, under the job ID the
//! queue minted. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::{Path, PathBuf};

use serde_json::json;
use tokio_postgres::{Config, NoTls};
use triplespace_api_ingest::jobs::{self, Kind, Runner};
use triplespace_cli::job::{JobCommand, StartKind};
use triplespace_cli::{adopt, follow, instance, job, job_work, sync};

const TENANT: &str = "librarybase";
const HOST: &str = "librarybase.org";

const DUMP_XML: &str = r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" version="0.11" xml:lang="en">
  <siteinfo><sitename>Librarybase</sitename><namespaces>
    <namespace key="120" case="first-letter">Item</namespace>
    <namespace key="122" case="first-letter">Property</namespace>
  </namespaces></siteinfo>
  <page><title>Property:P12</title><ns>122</ns><id>40</id>
    <revision><id>903</id><timestamp>2024-01-03T09:15:40Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <model>wikibase-property</model><format>application/json</format>
      <text xml:space="preserve">{"type":"property","id":"P12","datatype":"string","labels":{"en":{"language":"en","value":"note"}},"claims":{}}</text>
    </revision></page>
  <page><title>Item:Q6</title><ns>120</ns><id>12</id>
    <revision><id>41877</id><timestamp>2026-09-20T14:02:11Z</timestamp>
      <contributor><username>Bob</username><id>9</id></contributor>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six"}},"claims":{}}</text>
    </revision></page>
</mediawiki>
"#;

fn dump_json() -> String {
    let entities = [
        json!({"type": "item", "id": "Q9", "labels": {"en": {"language": "en", "value": "example.org"}},
            "descriptions": {}, "aliases": {}, "claims": {}, "sitelinks": {},
            "lastrevid": 55, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "property", "id": "P1", "datatype": "string",
            "labels": {"en": {"language": "en", "value": "domain name"}},
            "descriptions": {}, "aliases": {}, "claims": {},
            "lastrevid": 3, "modified": "2026-10-01T00:00:00Z"}),
    ];
    let lines: Vec<String> = entities.iter().map(ToString::to_string).collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

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

/// An instance whose tenant adopts a wiki and mirrors a provider, and its key file.
async fn setup(name: &str) -> Option<(String, PathBuf)> {
    admin_config()?;
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec!["internetdomains".into()],
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    Some((database, dir))
}

async fn rows(database: &str, sql: &str) -> Vec<Vec<Option<String>>> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client
        .query(&format!("SELECT * FROM ({sql}) q"), &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (0..r.len()).map(|i| r.get(i)).collect())
        .collect()
}

async fn one(database: &str, sql: &str) -> String {
    rows(database, sql).await[0][0].clone().unwrap_or_default()
}

fn adopt_args(database: &str, dump: &Path) -> adopt::Adopt {
    adopt::Adopt {
        job_id: None,
        database: database.to_string(),
        tenant: TENANT.into(),
        dump: dump.to_path_buf(),
        source: format!("https://{HOST}/"),
        version: Some("test".into()),
        frozen: true,
        current_only: false,
        counters: Some("item=6,property=12".into()),
        log_floor: 0,
        entity_sources: vec![],
        subsidiary: None,
        batch: 200,
        skip_talk: false,
        no_accounts: false,
    }
}

async fn start(kind: StartKind) {
    job::run(JobCommand::Start { kind })
        .await
        .expect("job start");
}

/// The newest queued row's job ID.
async fn queued(database: &str) -> u64 {
    one(
        database,
        "SELECT job_id::text FROM ops.job_queue ORDER BY job_id DESC LIMIT 1",
    )
    .await
    .parse()
    .unwrap()
}

// 0104 §4 (A9): an adoption and a sync are jobs of the queue, run from a dump the server
// reads, under the ID the queue gave them.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn an_adoption_and_a_sync_run_through_the_queue() {
    let Some((database, dir)) = setup("tsjobkinds_loads").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    let now = triplespace_cli::common::now;
    let xml = dir.join("librarybase.xml");
    std::fs::write(&xml, DUMP_XML).unwrap();
    let mirror = dir.join("internetdomains.json");
    std::fs::write(&mirror, dump_json()).unwrap();

    // A runner that was not given the work does not claim the kind.
    start(StartKind::Adopt(adopt_args(&database, &xml))).await;
    let adoption = queued(&database).await;
    assert!(
        jobs::run_once(&store, &pipeline, &Runner::new("plain"), now)
            .await
            .unwrap()
            .is_none()
    );
    let runner = job_work::with_loads(Runner::new("loads"), &database);
    let ran = jobs::run_once(&store, &pipeline, &runner, now)
        .await
        .unwrap()
        .expect("the adoption waited");
    assert_eq!(
        (ran.job_id, ran.kind, ran.error),
        (adoption, Kind::Adopt, None)
    );
    assert_eq!(
        rows(
            &database,
            &format!(
                "SELECT status, mode FROM view.job WHERE tenant = '{TENANT}' AND job_id = {adoption}"
            )
        )
        .await,
        vec![vec![Some("finished".into()), Some("adopt".into())]],
        "the adoption ran under the queue's ID"
    );
    assert_eq!(
        one(
            &database,
            &format!("SELECT count(*)::text FROM view.entity WHERE tenant = '{TENANT}'")
        )
        .await,
        "2"
    );

    // The sync of an instance mirror: an instance job, its actor the farm's operator.
    start(StartKind::Sync(sync::Sync {
        job_id: None,
        database: database.clone(),
        provider: Some("internetdomains".into()),
        tenant: None,
        source: None,
        dump: mirror,
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: 200,
        rejects: None,
        no_bootstrap: false,
        writers: sync::WRITERS,
    }))
    .await;
    let synced = queued(&database).await;
    let ran = jobs::run_once(&store, &pipeline, &runner, now)
        .await
        .unwrap()
        .expect("the sync waited");
    assert_eq!(
        (ran.job_id, ran.kind, ran.error),
        (synced, Kind::Sync, None)
    );
    assert_eq!(
        rows(
            &database,
            &format!("SELECT status, source FROM view.job WHERE tenant = '' AND job_id = {synced}")
        )
        .await,
        vec![vec![
            Some("finished".into()),
            Some("internetdomains".into())
        ]]
    );

    // A dump that is gone when the runner comes to it: the job fails, under its ID, with
    // the reason, and is not run again.
    let gone = dir.join("gone.xml");
    std::fs::write(&gone, DUMP_XML).unwrap();
    start(StartKind::Adopt(adopt_args(&database, &gone))).await;
    let failed = queued(&database).await;
    std::fs::remove_file(&gone).unwrap();
    let ran = jobs::run_once(&store, &pipeline, &runner, now)
        .await
        .unwrap()
        .expect("the second adoption waited");
    assert_eq!(ran.job_id, failed);
    assert!(ran.error.is_some(), "{ran:?}");
    assert_eq!(
        one(
            &database,
            &format!("SELECT status FROM view.job WHERE tenant = '{TENANT}' AND job_id = {failed}")
        )
        .await,
        "failed"
    );
    assert_eq!(
        one(&database, "SELECT count(*)::text FROM ops.job_queue").await,
        "0"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a stub wiki was asked: each request's first line and body.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A wiki's API that has seen no changes and takes any bot password: every listing is
/// empty, a login succeeds, and each request is noted.
async fn quiet_wiki() -> (String, Asked) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let asked = Asked::default();
    let noted = asked.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let noted = noted.clone();
            tokio::spawn(async move {
                // The head, then as much body as it says.
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    request.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&request);
                    let Some((head, body)) = text.split_once("\r\n\r\n") else {
                        if n == 0 {
                            return;
                        }
                        continue;
                    };
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if body.len() >= length || n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let line = text.lines().next().unwrap_or_default().to_string();
                let body = text
                    .split_once("\r\n\r\n")
                    .map_or("", |(_, b)| b)
                    .to_string();
                noted.lock().unwrap().push(format!("{line} {body}"));
                let answer = if text.contains("action=login") {
                    json!({"login": {"result": "Success", "lgusername": "Bot@follow"}})
                } else if text.contains("meta=tokens") {
                    json!({"query": {"tokens": {"logintoken": "a-login-token"}}})
                } else {
                    json!({
                        "batchcomplete": true,
                        "curtimestamp": "2026-11-13T00:00:00Z",
                        "query": {
                            "recentchanges": [],
                            "logevents": [],
                            "userinfo": {"name": "Bot", "rights": ["apihighlimits"]},
                            "namespaces": {
                                "120": {"id": 120, "canonical": "Item", "defaultcontentmodel": "wikibase-item"},
                                "122": {"id": 122, "canonical": "Property", "defaultcontentmodel": "wikibase-property"},
                            },
                        },
                    })
                }
                .to_string();
                let _ = socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\nconnection: close\r\n\r\n{answer}",
                            answer.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (format!("http://{address}/w/api.php"), asked)
}

// 0104 §4 (A9): a follower is a row of the queue that runs a pass at a time: after a
// pass it waits its interval, unclaimed, and is claimed again; cancelling it stops the
// following.
#[tokio::test]
async fn a_follower_runs_a_pass_at_a_time() {
    let Some((database, dir)) = setup("tsjobkinds_follow").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    let now = triplespace_cli::common::now;
    let key_file = dir.join("triplespace.key");
    start(StartKind::Follow(follow::Follow {
        database: database.clone(),
        provider: Some("internetdomains".into()),
        tenant: None,
        source: None,
        once: false,
        from: Some("2026-11-12T00:00:00Z".into()),
        interval: Some(3600),
        batch: 200,
        key_file: key_file.clone(),
        anonymous: true,
        api: Some(quiet_wiki().await.0),
    }))
    .await;
    let row = queued(&database).await;

    // The loads' runner leaves it: followers have a runner of their own.
    let loads = job_work::with_loads(Runner::new("loads"), &database);
    assert!(
        jobs::run_once(&store, &pipeline, &loads, now)
            .await
            .unwrap()
            .is_none()
    );
    let followers = job_work::with_followers(Runner::only("follow"), &database, &key_file);
    let ran = jobs::run_once(&store, &pipeline, &followers, now)
        .await
        .unwrap()
        .expect("the follower waited");
    assert_eq!((ran.job_id, ran.kind, ran.error), (row, Kind::Follow, None));

    // The row is back, unclaimed, for a pass an hour on; nothing claims it before.
    assert_eq!(
        rows(
            &database,
            "SELECT job_id::text, (claimed_at IS NULL)::text, attempts::text,
                    (not_before > now() + interval '50 minutes')::text
             FROM ops.job_queue"
        )
        .await,
        vec![vec![
            Some(row.to_string()),
            Some("true".into()),
            Some("0".into()),
            Some("true".into())
        ]]
    );
    assert!(
        jobs::run_once(&store, &pipeline, &followers, now)
            .await
            .unwrap()
            .is_none()
    );
    // Once its time has come it runs again, as the same row.
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    client
        .execute("UPDATE ops.job_queue SET not_before = now()", &[])
        .await
        .unwrap();
    let again = jobs::run_once(&store, &pipeline, &followers, now)
        .await
        .unwrap()
        .expect("the follower's next pass");
    assert_eq!((again.job_id, again.error), (row, None));

    // Cancelling the row ends the following.
    assert!(jobs::cancel(&client, "", row).await.unwrap());
    assert_eq!(
        one(&database, "SELECT count(*)::text FROM ops.job_queue").await,
        "0"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// 0104 §4 (A9); 0013 §4 (A103): the followers' runner is the one on the accounts login,
// because a follower logs in upstream with a bot password kept in `private`. On the
// public API's login the password cannot be read, and the pass goes on anonymously; on
// the accounts login the follower logs in with it.
#[tokio::test]
async fn a_follower_logs_in_from_the_accounts_login() {
    use triplespace_db::grants::role;
    let Some((database, dir)) = setup("tsjobkinds_login").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let now = triplespace_cli::common::now;
    let key_file = dir.join("triplespace.key");
    let password = dir.join("bot.pw");
    std::fs::write(&password, "bot-secret-5678\n").unwrap();
    follow::run_login(follow::UpstreamLogin::Set(follow::SetLogin {
        which: follow::WhichLogin {
            database: database.clone(),
            provider: Some("internetdomains".into()),
            tenant: None,
            source: None,
        },
        username: "Bot@follow".into(),
        password_file: Some(password),
        key_file: key_file.clone(),
    }))
    .await
    .expect("store the bot password");
    let (api, asked) = quiet_wiki().await;
    start(StartKind::Follow(follow::Follow {
        database: database.clone(),
        provider: Some("internetdomains".into()),
        tenant: None,
        source: None,
        once: false,
        from: Some("2026-11-12T00:00:00Z".into()),
        interval: Some(3600),
        batch: 200,
        key_file: key_file.clone(),
        anonymous: false,
        api: Some(api),
    }))
    .await;
    let row = queued(&database).await;
    let logins = || {
        asked
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.contains("action=login"))
            .cloned()
            .collect::<Vec<_>>()
    };

    // As the public API: the pass runs, and nobody logged in.
    let public = triplespace_db::scratch::as_role(&database, role::SERVER);
    let (store, pipeline) = triplespace_cli::common::store(&public, &farm).unwrap();
    let ran = jobs::run_once(
        &store,
        &pipeline,
        &job_work::with_followers(Runner::only("public"), &public, &key_file),
        now,
    )
    .await
    .unwrap()
    .expect("the follower waited");
    assert_eq!((ran.job_id, ran.error), (row, None));
    assert_eq!(
        logins(),
        Vec::<String>::new(),
        "the password is not the public login's to read"
    );

    // As the accounts service: the follower logs in with the stored password.
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    client
        .execute("UPDATE ops.job_queue SET not_before = now()", &[])
        .await
        .unwrap();
    let accounts = triplespace_db::scratch::as_role(&database, role::ACCOUNTS);
    let (store, pipeline) = triplespace_cli::common::store(&accounts, &farm).unwrap();
    let ran = jobs::run_once(
        &store,
        &pipeline,
        &job_work::with_followers(Runner::only("accounts"), &accounts, &key_file),
        now,
    )
    .await
    .unwrap()
    .expect("the follower's next pass");
    assert_eq!((ran.job_id, ran.error), (row, None));
    let logged_in = logins();
    assert_eq!(logged_in.len(), 1, "{logged_in:?}");
    assert!(
        logged_in[0].contains("lgname=Bot%40follow")
            && logged_in[0].contains("lgpassword=bot-secret-5678")
            && logged_in[0].contains("lgtoken=a-login-token"),
        "{logged_in:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// 0104 §4: the runner as the server runs it, a loop that claims and runs until the
// process ends: a job queued while it waits is picked up and finished, with nobody
// calling it, and nothing is claimed while the instance's runner is paused.
#[tokio::test]
async fn the_runner_loop_picks_up_what_is_queued() {
    let Some((database, dir)) = setup("tsjobkinds_loop").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    let xml = dir.join("librarybase.xml");
    std::fs::write(&xml, DUMP_XML).unwrap();
    let pause = |on: bool| {
        let database = database.clone();
        async move {
            let command = job::Database { database };
            job::run(if on {
                JobCommand::Pause(command)
            } else {
                JobCommand::Resume(command)
            })
            .await
            .expect("pause or resume");
        }
    };
    let status = async |id: u64| {
        rows(
            &database,
            &format!("SELECT status FROM view.job WHERE tenant = '{TENANT}' AND job_id = {id}"),
        )
        .await
        .first()
        .and_then(|r| r[0].clone())
    };

    pause(true).await;
    let looping = tokio::spawn(jobs::run_forever(
        store,
        pipeline,
        job_work::with_loads(Runner::new("loop"), &database),
        std::time::Duration::from_millis(50),
        triplespace_cli::common::now,
    ));
    start(StartKind::Adopt(adopt_args(&database, &xml))).await;
    let adoption = queued(&database).await;
    // Paused: several of the loop's passes go by, and the job still waits.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        status(adoption).await,
        None,
        "nothing is claimed while paused"
    );
    assert_eq!(
        one(&database, "SELECT count(*)::text FROM ops.job_queue").await,
        "1"
    );

    pause(false).await;
    let mut finished = false;
    for _ in 0..200 {
        if status(adoption).await.as_deref() == Some("finished") {
            finished = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    looping.abort();
    assert!(
        finished,
        "the loop ran the adoption: {:?}",
        status(adoption).await
    );
    assert_eq!(
        one(&database, "SELECT count(*)::text FROM ops.job_queue").await,
        "0"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
