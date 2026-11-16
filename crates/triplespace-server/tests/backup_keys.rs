//! The backup key set (0103 A3, A5; SCA-362): `GET` and `PUT /backup-keys` at the farm base,
//! the owner alone writing it, and `triplespace backup-key` registering it only where there is
//! no owner, unsealing a backup's shares and resealing a backup to another set.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_integrity::backup_key::{KeySet, PublicKey, SecretKey, Share, seal_new};
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::backup_key::{self, BackupKey};
use triplespace_cli::{instance, log_bundle, sealed};

const DB: &str = "tsbackupkeys_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "lbwiki";
const REST: &str = "/w/rest.php/triplespace/v0";

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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_backup_key_set() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsbackupkeys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
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
    let app = App::new(
        triplespace_db::scratch::pools(&database, 4),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: triplespace_cli::common::farm_at(&database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    };

    // No set yet.
    let r = c.rest("GET", "/backup-keys", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["set"], Value::Null);

    // Without a set, a backup refuses to leave `private` out unless told; told, it says so in
    // the bundle; and a backup of one tenant carries no `private` (0103 A7, A8).
    let export = |out: std::path::PathBuf, tenant: Option<&str>, without_private: bool| {
        let args = log_bundle::Export {
            database: database.clone(),
            scope: log_bundle::Scope::Backup,
            tenant: tenant.map(str::to_owned),
            out,
            without_private,
        };
        async move { log_bundle::export(&args).await }
    };
    let refused = export(dir.join("refused"), None, false).await.unwrap_err();
    assert!(
        format!("{refused:#}").contains("no backup key set"),
        "{refused:#}"
    );
    let refused = export(dir.join("tenant"), Some(TENANT), false)
        .await
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("--without-private"),
        "{refused:#}"
    );
    export(dir.join("log-only"), None, true).await.unwrap();
    let log_only = scatter_integrity::Bundle::open(dir.join("log-only")).unwrap();
    assert_eq!(log_only.manifest().sealed, Some(false));
    assert!(!dir.join("log-only").join(sealed::DIR).exists());

    // Three holders make their keys off the instance.
    let secrets: Vec<SecretKey> = (0..3).map(|_| SecretKey::generate()).collect();
    let publics: Vec<String> = secrets.iter().map(|s| s.public().to_text()).collect();

    // A reader without an account may not register a set.
    let r = c
        .rest("PUT", "/backup-keys", Some(json!({"keys": publics})))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);

    // The instance has an owner, so the command line does not register the set (0103 A3).
    let refused = backup_key::run(BackupKey::Register(backup_key::Register {
        target: backup_key::Target {
            database: database.clone(),
        },
        keys: publics.clone(),
        threshold: 1,
    }))
    .await
    .unwrap_err();
    assert!(refused.to_string().contains("owner"), "{refused}");

    // The owner logs in and registers the set: any two of three keys restore.
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
    let token = c.csrf().await;
    let r = c
        .rest(
            "PUT",
            "/backup-keys",
            Some(json!({"keys": publics, "threshold": 4, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "ts-backup-keys-invalid");
    let r = c
        .rest(
            "PUT",
            "/backup-keys",
            Some(json!({"keys": publics, "threshold": 2, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let offset = r.body["set"]["offset"].as_u64().unwrap();
    let r = c.rest("GET", "/backup-keys", None).await;
    assert_eq!(r.body["set"]["threshold"], 2);
    assert_eq!(r.body["set"]["keys"], json!(publics));
    assert_eq!(r.body["set"]["offset"], offset);
    assert_eq!(
        r.body["set"]["key_ids"][0],
        secrets[0].public().key_id(),
        "{}",
        r.body
    );

    // A real backup, made under the set: `private` and `ops.delivery` sealed into it, from the
    // backup's snapshot (0103 A7, A8; SCA-354).
    let full = dir.join("full");
    export(full.clone(), None, false).await.unwrap();
    assert_eq!(
        scatter_integrity::Bundle::open(&full)
            .unwrap()
            .manifest()
            .sealed,
        Some(true)
    );
    // Its carried sections: the carried `ops` tables in the clear, and every `log` sequence
    // as it stood (0103 A4, A9).
    let carried = triplespace_cli::carried::Carried::open(&full).unwrap();
    let carried_names: Vec<String> = carried
        .manifest
        .tables
        .iter()
        .map(|t| format!("{}.{}", t.schema, t.table))
        .collect();
    assert_eq!(
        carried_names,
        [
            "ops.job_queue",
            "ops.forwarder_key",
            "ops.repo_cursor",
            "ops.visibility_epoch"
        ]
    );
    let live_sequences: Vec<(String, i64, bool)> = triplespace_cli::common::connect(&database)
        .await
        .unwrap()
        .query(
            "SELECT sequencename::text, coalesce(last_value, start_value), last_value IS NOT NULL
             FROM pg_sequences WHERE schemaname = 'log' ORDER BY 1",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    let carried_sequences: Vec<(String, i64, bool)> = carried
        .manifest
        .sequences
        .iter()
        .map(|s| (s.name.clone(), s.value, s.called))
        .collect();
    assert_eq!(carried_sequences, live_sequences);
    assert!(
        carried_sequences
            .iter()
            .any(|(n, v, called)| n == &format!("{TENANT}.user_id") && *called && *v == 7),
        "{carried_sequences:?}"
    );
    let opened = sealed::Sealed::open(&full).unwrap();
    assert_eq!(opened.manifest.set, offset);
    assert_eq!(opened.key.threshold, 2);
    let names: Vec<String> = opened
        .manifest
        .tables
        .iter()
        .map(|t| format!("{}.{}", t.schema, t.table))
        .collect();
    for t in [
        "private.password",
        "private.session",
        "private.login",
        "ops.delivery",
    ] {
        assert!(names.contains(&t.to_string()), "{t} in {names:?}");
    }
    assert!(
        !names.contains(&"ops.job_queue".to_string()),
        "carried, not sealed"
    );
    assert!(opened.manifest.migrations.len() > 30);
    // No table's bytes are in the clear: the owner's password hash is not in any file.
    let db = triplespace_cli::common::connect(&database).await.unwrap();
    let hash: String = db
        .query_one("SELECT hash FROM private.password LIMIT 1", &[])
        .await
        .unwrap()
        .get(0);
    for entry in std::fs::read_dir(full.join(sealed::DIR)).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes.windows(hash.len()).any(|w| w == hash.as_bytes()),
            "a sealed file holds the hash in the clear"
        );
    }
    // Two holders unseal their shares; one share alone does not open the backup.
    let mut shares = Vec::new();
    for i in [1, 2] {
        let key_file = dir.join(format!("full-holder{i}.key"));
        std::fs::write(&key_file, secrets[i].to_text()).unwrap();
        let out = dir.join(format!("full-holder{i}.share"));
        backup_key::run(BackupKey::Unseal(backup_key::Unseal {
            sealed: opened.key_path(),
            key: key_file,
            out: out.clone(),
        }))
        .await
        .unwrap();
        shares.push(Share::parse(&std::fs::read_to_string(&out).unwrap()).unwrap());
    }
    assert!(opened.key.combine(&shares[..1]).is_err());
    let data_key = opened.key.combine(&shares).unwrap();
    // Each table opens to the binary COPY of the table as it was.
    for (schema, table) in [("private", "password"), ("private", "login")] {
        let t = opened
            .manifest
            .tables
            .iter()
            .find(|t| t.schema == schema && t.table == table)
            .unwrap();
        let cols = t
            .columns
            .iter()
            .map(|c| format!("\"{}\"", c.name))
            .collect::<Vec<_>>()
            .join(", ");
        let live: Vec<u8> = {
            use futures_util::TryStreamExt as _;
            let s = db
                .copy_out(&format!(
                    "COPY {schema}.{table} ({cols}) TO STDOUT (FORMAT binary)"
                ))
                .await
                .unwrap();
            let parts: Vec<bytes::Bytes> = s.try_collect().await.unwrap();
            parts.concat()
        };
        assert_eq!(
            opened.table(&data_key, schema, table).unwrap(),
            live,
            "{schema}.{table}"
        );
    }

    // A backup made under the set: its data key sealed to the three keys.
    let registered = triplespace_api_ingest::backup_keys::current(
        &triplespace_cli::common::connect(&database).await.unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(registered.offset, offset);
    let context = format!("backup:{offset}:test");
    let (data_key, sealed) = seal_new(&registered.set, &context).unwrap();
    let blob = data_key.encrypt(b"private", b"the private section");
    let sealed_path = dir.join("sealed-key.cbor");
    std::fs::write(&sealed_path, sealed.encode().unwrap()).unwrap();

    // Two holders unseal their shares offline.
    let mut share_files = Vec::new();
    for i in [0, 2] {
        let key_file = dir.join(format!("holder{i}.key"));
        std::fs::write(&key_file, secrets[i].to_text()).unwrap();
        let out = dir.join(format!("holder{i}.share"));
        backup_key::run(BackupKey::Unseal(backup_key::Unseal {
            sealed: sealed_path.clone(),
            key: key_file,
            out: out.clone(),
        }))
        .await
        .unwrap();
        share_files.push(out);
    }

    // The operator reseals the backup, by hand, to a new one-key set (0103 A3).
    let newcomer = SecretKey::generate();
    backup_key::run(BackupKey::Reseal(backup_key::Reseal {
        sealed: sealed_path.clone(),
        shares: share_files,
        keys: vec![newcomer.public().to_text()],
        threshold: 1,
    }))
    .await
    .unwrap();
    let resealed =
        scatter_integrity::backup_key::SealedKey::decode(&std::fs::read(&sealed_path).unwrap())
            .unwrap();
    assert_eq!(resealed.context, context, "the backup keeps its context");
    let newcomer_file = dir.join("newcomer.key");
    std::fs::write(&newcomer_file, newcomer.to_text()).unwrap();
    let newcomer_share = dir.join("newcomer.share");
    backup_key::run(BackupKey::Unseal(backup_key::Unseal {
        sealed: sealed_path.clone(),
        key: newcomer_file,
        out: newcomer_share.clone(),
    }))
    .await
    .unwrap();
    let share = Share::parse(&std::fs::read_to_string(&newcomer_share).unwrap()).unwrap();
    let rebuilt = resealed.combine(&[share]).unwrap();
    assert_eq!(
        rebuilt.decrypt(b"private", &blob).unwrap(),
        b"the private section"
    );
    assert!(
        resealed.unseal(&secrets[0]).is_err(),
        "the old holders hold no share of the resealed backup"
    );

    // With no owner account left, the operator registers a set through the command line.
    triplespace_cli::common::connect(&database)
        .await
        .unwrap()
        .batch_execute("DELETE FROM view.membership WHERE \"group\" = 'owner'")
        .await
        .unwrap();
    backup_key::run(BackupKey::Register(backup_key::Register {
        target: backup_key::Target {
            database: database.clone(),
        },
        keys: vec![newcomer.public().to_text()],
        threshold: 1,
    }))
    .await
    .unwrap();
    let now = triplespace_api_ingest::backup_keys::current(
        &triplespace_cli::common::connect(&database).await.unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(now.offset > offset);
    assert_eq!(
        now.set,
        KeySet::new(
            vec![PublicKey::parse(&newcomer.public().to_text()).unwrap()],
            1
        )
        .unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
