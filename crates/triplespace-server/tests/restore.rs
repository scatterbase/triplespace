//! `triplespace restore` (0103 A2, A9; SCA-357): an instance backed up, then restored into
//! an empty database with one holder's share, compared with the original table by table, and
//! served: the owner logs in with the password of before and reads the page of before.

use std::net::SocketAddr;

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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_backup_restores_into_an_empty_database() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let original = fresh_database("tsrestore_from").await;
    let dir = std::env::temp_dir().join(format!("tsrestore-{}", std::process::id()));
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

    // Life on the instance: an edit, a preference, a watch, a forwarder key, a key set.
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

    // The backup, and the holder's share of it.
    let bundle = dir.join("backup");
    log_bundle::export(&log_bundle::Export {
        database: original.clone(),
        scope: log_bundle::Scope::Backup,
        tenant: None,
        out: bundle.clone(),
        without_private: false,
    })
    .await
    .unwrap();
    let key_file = dir.join("holder.key");
    std::fs::write(&key_file, holder.to_text()).unwrap();
    let share = dir.join("holder.share");
    backup_key::run(BackupKey::Unseal(backup_key::Unseal {
        sealed: restore::sealed_key_path(&bundle),
        key: key_file,
        out: share.clone(),
    }))
    .await
    .unwrap();

    // A sealed backup needs a share; a database that is not empty is refused.
    let restored = fresh_database("tsrestore_to").await;
    let args = |database: &str, shares: Vec<std::path::PathBuf>| restore::Restore {
        database: database.to_string(),
        bundle: bundle.clone(),
        shares,
        without_private: false,
    };
    let refused = restore::restore(&args(&restored, vec![]))
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("--share"), "{refused:#}");
    let refused = restore::restore(&args(&original, vec![share.clone()]))
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("empty"), "{refused:#}");

    // The restore.
    let summary = restore::restore(&args(&restored, vec![share]))
        .await
        .unwrap();
    assert!(
        summary.partitions >= 9 && summary.slots > 0 && summary.tables > 10,
        "{summary:?}"
    );

    // The same log, record for record, with its checkpoints and manifests; the same
    // sequences; the same projections; the same private rows; the same carried ops rows.
    for sql in [
        "SELECT partition::text, \"offset\"::text, encode(leaf, 'hex'), erased::text FROM log.record ORDER BY 1, 2",
        "SELECT partition::text, tenant, name, graph_iri, history, integrity, export, hash, origin, next_offset::text FROM log.partition ORDER BY 1",
        "SELECT partition::text, tree_size::text, signed_note FROM log.checkpoint ORDER BY 1, 2",
        "SELECT sequencename::text, last_value::text FROM pg_sequences WHERE schemaname = 'log' ORDER BY 1",
        "SELECT tenant, actor_key, name FROM view.actor ORDER BY 1, 2",
        "SELECT tenant, kind, code, config::text FROM view.registry ORDER BY 1, 2, 3",
        "SELECT login::text, hash FROM private.password ORDER BY 1",
        "SELECT id::text, name FROM private.login ORDER BY 1",
        "SELECT actor_key, key, value::text FROM private.preference ORDER BY 1, 2",
        "SELECT actor_key, target_kind::text, target_id FROM private.watch ORDER BY 1, 2, 3",
        "SELECT key_id, label, hash FROM ops.forwarder_key ORDER BY 1",
    ] {
        assert_eq!(
            rows(&original, sql).await,
            rows(&restored, sql).await,
            "{sql}"
        );
    }
    // Every tenant's visibility epoch has moved past any the old instance used (0103 A4).
    let before = rows(
        &original,
        "SELECT tenant, epoch::text FROM ops.visibility_epoch ORDER BY 1",
    )
    .await;
    let after = rows(
        &restored,
        "SELECT tenant, epoch::text FROM ops.visibility_epoch ORDER BY 1",
    )
    .await;
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        let (b, a): (i64, i64) = (
            b[1].as_deref().unwrap().parse().unwrap(),
            a[1].as_deref().unwrap().parse().unwrap(),
        );
        assert!(a > b && a > 1_000_000_000_000_000, "{b} -> {a}");
    }

    // A fresh backup of the restored instance verifies (SCA-357): the log, its checkpoints
    // and its manifests came back whole.
    let again = dir.join("again");
    log_bundle::export(&log_bundle::Export {
        database: restored.clone(),
        scope: log_bundle::Scope::Backup,
        tenant: None,
        out: again.clone(),
        without_private: false,
    })
    .await
    .unwrap();
    let reports = log_bundle::verify(&log_bundle::Verify {
        bundle: again,
        level: log_bundle::VerifyLevel::Bodies,
        records: false,
        keys: None,
    })
    .await
    .unwrap();
    assert!(!reports.is_empty());
    for (p, r) in &reports {
        assert!(r.is_clean(), "{}: {:?}", p.origin, r.problems);
    }

    // The restored instance serves: the owner logs in with the password of before, has the
    // preference of before, and reads the page of before.
    let mut c = app(&restored).await;
    log_in(&mut c).await;
    let ui = c.api("action=query&meta=userinfo&uiprop=options").await;
    assert_eq!(ui["query"]["userinfo"]["options"]["language"], "de", "{ui}");
    let page = c
        .api("action=query&prop=revisions&rvprop=content&rvslots=main&titles=Restored")
        .await;
    assert!(page.to_string().contains("Before the move."), "{page}");
    let _ = std::fs::remove_dir_all(&dir);
}
