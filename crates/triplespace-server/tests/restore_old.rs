//! Restoring a backup written under an older schema (0103 §4, A11; SCA-358). The fixture,
//! `tests/fixtures/backup-0038/`, is a backup of a small instance made by the binary as it
//! stood at migration `0038_ops_backup_classes`, before carried sections recorded their
//! migrations; `holder.share` beside it is the one share its sealed sections need. It was
//! written by `write_the_fixture` below, run at that commit, and is never regenerated:
//! it stands for every backup an operator made then and keeps.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_integrity::backup_key::SecretKey;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::backup_key::{self, BackupKey};
use triplespace_cli::{forwarder, instance, log_bundle, restore};

const HOST: &str = "librarybase.org";
const TENANT: &str = "lbwiki";
const REST: &str = "/w/rest.php/triplespace/v0";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database(db: &str) -> String {
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {db}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(db);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{db}"))
        .unwrap()
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A client with one session cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Client {
    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Option<String>,
        extra: &[(header::HeaderName, &str)],
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        for (name, value) in extra {
            req = req.header(name, *value);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        if let Some(sc) = headers.get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Reply { status, body }
    }

    async fn api(&mut self, query: &str) -> Value {
        let r = self
            .send(
                "GET",
                &format!("/w/api.php?{query}&format=json&formatversion=2"),
                None,
                None,
                &[],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn api_post(&mut self, form: &str) -> Value {
        let r = self
            .send(
                "POST",
                "/w/api.php",
                Some("application/x-www-form-urlencoded"),
                Some(format!("{form}&format=json&formatversion=2")),
                &[],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn rest(&mut self, method: &str, path: &str, body: Option<Value>) -> Reply {
        let body = body.map(|b| b.to_string());
        let ct = body.as_ref().map(|_| "application/json");
        self.send(method, &format!("{REST}{path}"), ct, body, &[])
            .await
    }

    async fn csrf(&mut self) -> String {
        self.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

async fn app(database: &str) -> Client {
    let app = App::new(
        triplespace_db::scratch::pools(database, 4),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: triplespace_cli::common::farm_at(database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    }
}

async fn log_in(c: &mut Client) {
    let t = c.api("action=query&meta=tokens&type=login").await;
    let login = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c
        .api_post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&login)
        ))
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
}

/// Rows of `sql` on each database, as text, for comparing.
async fn rows(database: &str, sql: &str) -> Vec<Vec<Option<String>>> {
    let c = triplespace_cli::common::connect(database).await.unwrap();
    c.query(&format!("SELECT * FROM ({sql}) q"), &[])
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (0..r.len())
                .map(|i| r.get::<_, Option<String>>(i))
                .collect()
        })
        .collect()
}

fn fixture() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/backup-0038"
    ))
}

/// Writes the fixture: `TRIPLESPACE_WRITE_BACKUP_FIXTURE={dir} cargo test -p
/// triplespace-server --test restore_old -- --ignored write_the_fixture`.
#[tokio::test]
#[ignore = "writes the fixture; run once, at the commit the fixture stands for"]
async fn write_the_fixture() {
    let Ok(out) = std::env::var("TRIPLESPACE_WRITE_BACKUP_FIXTURE") else {
        return;
    };
    let out = PathBuf::from(out);
    let original = fresh_database("tsrestore_fixture").await;
    let dir = std::env::temp_dir().join(format!("tsrestore-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: original.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: Some("Library Base".into()),
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    let mut c = app(&original).await;
    log_in(&mut c).await;
    let token = c.csrf().await;
    let r = c
        .api_post(&format!(
            "action=edit&title=Restored&text=Before%20the%20move.&watchlist=watch&token={}",
            enc(&token)
        ))
        .await;
    assert_eq!(r["edit"]["result"], "Success", "{r}");
    let r = c
        .api_post(&format!(
            "action=options&change=language=de&token={}",
            enc(&token)
        ))
        .await;
    assert_eq!(r["options"], "success", "{r}");
    forwarder::run(forwarder::Forwarder::Create(forwarder::Create {
        target: forwarder::Target {
            database: original.clone(),
        },
        label: "edge".into(),
    }))
    .await
    .unwrap();
    let holder = SecretKey::generate();
    let r = c
        .rest(
            "PUT",
            "/backup-keys",
            Some(json!({"keys": [holder.public().to_text()], "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let _ = std::fs::remove_dir_all(&out);
    log_bundle::export(&log_bundle::Export {
        database: original.clone(),
        scope: log_bundle::Scope::Backup,
        tenant: None,
        out: out.clone(),
        without_private: false,
    })
    .await
    .unwrap();
    let key_file = dir.join("holder.key");
    std::fs::write(&key_file, holder.to_text()).unwrap();
    backup_key::run(BackupKey::Unseal(backup_key::Unseal {
        sealed: restore::sealed_key_path(&out),
        key: key_file,
        out: out.join("holder.share"),
    }))
    .await
    .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

fn args(database: &str) -> restore::Restore {
    restore::Restore {
        database: database.to_string(),
        bundle: fixture(),
        shares: vec![fixture().join("holder.share")],
        without_private: false,
    }
}

/// The restored instance is the one backed up: the owner logs in with the password of
/// before, has the preference of before, and reads the page of before.
async fn serves(database: &str) {
    let mut c = app(database).await;
    log_in(&mut c).await;
    let ui = c.api("action=query&meta=userinfo&uiprop=options").await;
    assert_eq!(ui["query"]["userinfo"]["options"]["language"], "de", "{ui}");
    let page = c
        .api("action=query&prop=revisions&rvprop=content&rvslots=main&titles=Restored")
        .await;
    assert!(page.to_string().contains("Before the move."), "{page}");
}

// 0103 §4 (A11): a backup made by an older binary restores with this one, every migration
// since run over its tables.
#[tokio::test]
async fn a_backup_of_an_older_binary_restores() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database("tsrestore_old").await;
    let summary = restore::restore(&args(&database)).await.unwrap();
    assert!(
        summary.partitions >= 9 && summary.slots > 0 && summary.tables > 10,
        "{summary:?}"
    );
    // Every migration of this binary is recorded, the fixture's 40 and any since.
    let pending = triplespace_db::migrator()
        .pending(&triplespace_cli::common::connect(&database).await.unwrap())
        .await
        .unwrap();
    assert!(pending.is_empty(), "{pending:?}");
    assert_eq!(
        rows(
            &database,
            "SELECT label FROM ops.forwarder_key UNION ALL
             SELECT nspname::text FROM pg_namespace WHERE nspname = 'ts_restore'"
        )
        .await,
        vec![vec![Some("edge".to_string())]],
        "the carried row is back, and the restore's own schema is gone"
    );
    serves(&database).await;

    // Every tenant has an epoch no instance before the restore can have counted to,
    // whether or not the backup held a row for it (0103 A4).
    assert_eq!(
        rows(
            &database,
            "SELECT DISTINCT p.tenant FROM log.partition p
             LEFT JOIN ops.visibility_epoch e ON e.tenant = p.tenant
             WHERE p.tenant IS NOT NULL AND coalesce(e.epoch, 0) < 1000000000000000"
        )
        .await,
        Vec::<Vec<Option<String>>>::new()
    );
    // A database a restore has touched is not empty, finished or not.
    let refused = restore::restore(&args(&database)).await.unwrap_err();
    assert!(format!("{refused:#}").contains("empty"), "{refused:#}");
}

/// A migration no binary has yet: it changes a sealed table and a carried one, each from
/// the rows the table holds.
static LATER: [triplespace_db::Migration; 1] = [triplespace_db::Migration {
    name: "0001_later",
    sql: "ALTER TABLE private.preference ADD COLUMN shout text;
          UPDATE private.preference SET shout = upper(key);
          ALTER TABLE private.preference ALTER COLUMN shout SET NOT NULL;
          ALTER TABLE ops.forwarder_key ADD COLUMN note text;
          UPDATE ops.forwarder_key SET note = 'was ' || label;",
}];

// 0103 §4 (A11): the tables are loaded into the schema the backup records and the later
// migrations convert them, rather than the restore: a migration that fills a new column
// from the old rows finds the backup's rows there.
#[tokio::test]
async fn later_migrations_run_over_the_backups_rows() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database("tsrestore_later").await;
    let migrator = triplespace_db::migrator().with("restore-test", &LATER);
    restore::restore_with(&args(&database), &migrator)
        .await
        .unwrap();
    assert_eq!(
        rows(
            &database,
            "SELECT key, value::text, shout FROM private.preference WHERE key = 'language'"
        )
        .await,
        vec![vec![
            Some("language".to_string()),
            Some("\"de\"".to_string()),
            Some("LANGUAGE".to_string())
        ]]
    );
    assert_eq!(
        rows(&database, "SELECT label, note FROM ops.forwarder_key").await,
        vec![vec![Some("edge".to_string()), Some("was edge".to_string())]]
    );
    assert_eq!(
        rows(
            &database,
            "SELECT name FROM ops.migration WHERE crate_name = 'restore-test'"
        )
        .await,
        vec![vec![Some("0001_later".to_string())]]
    );
    serves(&database).await;

    // A backup that names a migration this binary lacks was written by a newer one.
    let newer = fresh_database("tsrestore_newer").await;
    let without = triplespace_db::Migrator::new()
        .with("scatter-log-postgres", scatter_log_postgres::MIGRATIONS)
        .with_grants();
    let refused = restore::restore_with(&args(&newer), &without)
        .await
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("does not have"),
        "{refused:#}"
    );
}
