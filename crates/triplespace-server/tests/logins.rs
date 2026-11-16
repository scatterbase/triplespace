//! Instance logins end to end (ADR 0089 §2, §3, §4, §12, §15; 07 §2.8): `instance create`
//! opens the `logins` partition and gives the owner a login whose password `clientlogin`
//! checks; `createaccount` makes a login per account, named after it; a login's sign-in
//! methods are what `GET /account` reports; the session carries the login; and `logins
//! migrate` turns an account's old actor-keyed password into a login, idempotently. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::{Secret, bindings, logins, password};
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;

const DB: &str = "tslogins_test";
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

async fn db(database: &str) -> tokio_postgres::Client {
    let (client, connection) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

struct Client {
    router: Router,
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Client {
    async fn send(&mut self, method: &str, uri: &str, body: Option<String>) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        for sc in response.headers().get_all(header::SET_COOKIE) {
            if let Some(v) = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
            {
                self.cookie = (!v.is_empty()).then(|| v.to_string());
            }
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply { status, body }
    }

    async fn api(&mut self, query: &str) -> Value {
        self.send(
            "GET",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            None,
        )
        .await
        .body
    }

    async fn api_post(&mut self, form: &str) -> Value {
        self.send(
            "POST",
            "/w/api.php",
            Some(format!("{form}&format=json&formatversion=2")),
        )
        .await
        .body
    }

    async fn token(&mut self, kind: &str) -> String {
        self.api(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    async fn clientlogin(&mut self, name: &str, pw: &str) -> Value {
        let t = self.token("login").await;
        self.api_post(&format!(
            "action=clientlogin&username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(name),
            enc(pw),
            enc(&t)
        ))
        .await
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn instance_logins() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tslogins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    let key_file = dir.join("triplespace.key");
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
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
    let router = router_with(app, triplespace_api_rest::routes());
    let store = db(&database).await;

    // 0089 §2, §12: `instance create` opened the logins partition and made the owner's
    // login, named after the owner, with the password keyed by it (§3) and the account
    // bound through the `login` issuer (§4).
    let partition: Option<tokio_postgres::Row> = store
        .query_opt(
            "SELECT 1 FROM log.partition WHERE tenant IS NULL AND name = 'logins'",
            &[],
        )
        .await
        .unwrap();
    assert!(partition.is_some(), "the logins partition");
    let alice = logins::by_name(&store, "alice")
        .await
        .unwrap()
        .expect("Alice's login");
    assert_eq!(alice.name, "Alice");
    let owner_key = format!(
        "{}:7",
        store.query_one(
            "SELECT config->>'issuer' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&TENANT]
        )
        .await
        .unwrap()
        .get::<_, String>(0)
    );
    assert_eq!(
        logins::login_of(&store, TENANT, &owner_key).await.unwrap(),
        Some(alice.id)
    );
    assert_eq!(
        logins::account_of(&store, TENANT, alice.id)
            .await
            .unwrap()
            .as_deref(),
        Some(owner_key.as_str())
    );
    assert!(password::has(&store, alice.id).await.unwrap());
    let unmigrated: i64 = store
        .query_one(
            "SELECT count(*) FROM private.password WHERE actor_key IS NOT NULL",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(unmigrated, 0);
    // The login record is in the logins partition, keyed `login:{n}`. A partition ID is
    // 64 random bits, stored as the `bigint` with the same bits.
    #[allow(clippy::cast_possible_wrap)]
    let partition_db = alice.partition as i64;
    let record: String = store
        .query_one(
            "SELECT key FROM log.record WHERE partition = $1 AND \"offset\" = $2",
            &[&partition_db, &i64::try_from(alice.offset).unwrap()],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(record, logins::key_of(alice.id));

    // §3, §11: clientlogin checks the login's password; the session carries the login.
    let mut owner = Client {
        router: router.clone(),
        cookie: None,
    };
    let r = owner.clientlogin("Alice", "wrong").await;
    assert_eq!(r["clientlogin"]["status"], "FAIL", "{r}");
    let r = owner.clientlogin("Alice", "owner-secret-1234").await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let session: Option<i64> = store
        .query_one(
            "SELECT login FROM private.session WHERE id = $1",
            &[&owner.cookie.clone().unwrap()],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(session, Some(alice.id));
    let a = owner.send("GET", &format!("{REST}/account"), None).await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.body);
    assert_eq!(a.body["password"], true);
    assert_eq!(a.body["ways"], 1);
    assert!(
        logins::get(&store, alice.id)
            .await
            .unwrap()
            .unwrap()
            .last_used
            .is_some(),
        "a login notes when it authenticated"
    );

    // Creating an account makes a login per account (§8, §12), named after the account;
    // the owner, who holds `skipcaptcha`, creates Bob's without a challenge.
    let t = owner.token("createaccount").await;
    let r = owner
        .api_post(&format!(
            "action=createaccount&username=Bob&password=bob-secret-1234&retype=bob-secret-1234&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&t)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    let mut bob = Client {
        router: router.clone(),
        cookie: None,
    };
    let bob_login = logins::by_name(&store, "Bob")
        .await
        .unwrap()
        .expect("Bob's login");
    assert_ne!(bob_login.id, alice.id);
    let r = bob.clientlogin("Bob", "bob-secret-1234").await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let logins_made: i64 = store
        .query_one("SELECT count(*) FROM private.login", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(logins_made, 2);

    // §15: a credential still keyed by an account, as an instance from before logins
    // holds it, becomes a login under `logins migrate`; a second run changes nothing.
    let bob_key: String = store
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = 'Bob'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    store
        .execute(
            "UPDATE private.password SET actor_key = $1, login = NULL WHERE login = $2",
            &[&bob_key, &bob_login.id],
        )
        .await
        .unwrap();
    store
        .execute(
            "DELETE FROM private.binding WHERE tenant = $1 AND issuer = 'login' AND subject = $2",
            &[&TENANT, &bob_login.id.to_string()],
        )
        .await
        .unwrap();
    store
        .execute("DELETE FROM private.login WHERE id = $1", &[&bob_login.id])
        .await
        .unwrap();
    assert_eq!(
        logins::login_of(&store, TENANT, &bob_key).await.unwrap(),
        None
    );
    let r = bob.clientlogin("Bob", "bob-secret-1234").await;
    assert_eq!(r["clientlogin"]["status"], "FAIL", "no login, no login");
    let done = triplespace_cli::logins::run_migration(&database, &key_file)
        .await
        .expect("logins migrate");
    assert!(!done.partition_opened);
    assert_eq!(done.logins, 1, "{done:?}");
    assert_eq!(done.hashes_moved, 1);
    assert!(done.skipped.is_empty(), "{done:?}");
    let migrated = logins::login_of(&store, TENANT, &bob_key)
        .await
        .unwrap()
        .expect("Bob's new login");
    assert!(password::has(&store, migrated).await.unwrap());
    assert_eq!(bindings::ways_to_log_in(&store, migrated).await.unwrap(), 1);
    // The name was held by the deleted row no longer, so the login is `Bob` again.
    assert_eq!(
        logins::get(&store, migrated).await.unwrap().unwrap().name,
        "Bob"
    );
    let r = bob.clientlogin("Bob", "bob-secret-1234").await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let again = triplespace_cli::logins::run_migration(&database, &key_file)
        .await
        .unwrap();
    assert_eq!(
        (again.logins, again.hashes_moved, again.identities),
        (0, 0, 0)
    );

    // A login name held by another login's account is qualified with the tenant (§15).
    assert_eq!(
        logins::name_for(&store, TENANT, "Alice").await.unwrap(),
        format!("Alice ({TENANT})")
    );
    let _ = json!(null);
    let _ = std::fs::remove_dir_all(&dir);
}
