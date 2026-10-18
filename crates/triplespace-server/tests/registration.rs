//! Registration and challenges end to end (ADR 0087): `siteinfo`'s report, the
//! challenge in `meta=authmanagerinfo`, `action=createaccount` with its refusals, the
//! spent set, the rising difficulty and the throttle, the `badlogin` challenge in front
//! of `action=clientlogin`, and approval: requests filed, refused names, the pending and
//! rejected answers at login, and the review routes. Needs
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
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;
use triplespace_projections::Farm;

const DB: &str = "tsregistration_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "librarybase";
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
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A browser: one session cookie, one address.
struct Client {
    router: Router,
    cookie: Option<String>,
    address: SocketAddr,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Client {
    fn new(router: &Router, address: [u8; 4]) -> Self {
        Self {
            router: router.clone(),
            cookie: None,
            address: SocketAddr::from((address, 50000)),
        }
    }

    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        ct: Option<&str>,
        body: Option<String>,
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = ct {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut().insert(ConnectInfo(self.address));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
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
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn post(&mut self, form: &str) -> Value {
        let r = self
            .send(
                "POST",
                "/w/api.php",
                Some("application/x-www-form-urlencoded"),
                Some(format!("{form}&format=json&formatversion=2")),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn rest(&mut self, method: &str, path: &str, body: Option<Value>) -> Reply {
        let body = body.map(|b| b.to_string());
        let ct = body.as_ref().map(|_| "application/json");
        self.send(method, &format!("{REST}{path}"), ct, body).await
    }

    async fn token(&mut self, kind: &str) -> String {
        self.api(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    /// The challenge `authmanagerinfo` offers for an act, if any.
    async fn challenge(&mut self, requestsfor: &str) -> Option<(String, u8)> {
        let v = self
            .api(&format!(
                "action=query&meta=authmanagerinfo&amirequestsfor={requestsfor}&amimergerequestfields=1"
            ))
            .await;
        let f = &v["query"]["authmanagerinfo"]["fields"];
        let id = f["tschallengeid"]["value"].as_str()?.to_string();
        let bits = scatter_challenge::Challenge::parse(&id).unwrap().bits;
        Some((id, bits))
    }

    async fn create(&mut self, name: &str, password: &str, extra: &str) -> Value {
        let token = self.token("createaccount").await;
        self.post(&format!(
            "action=createaccount&username={}&password={}&retype={}&createtoken={}&createreturnurl=https://{HOST}/{extra}",
            enc(name),
            enc(password),
            enc(password),
            enc(&token)
        ))
        .await
    }

    /// Creates an account, solving the challenge offered.
    async fn create_solved(&mut self, name: &str, password: &str, extra: &str) -> Value {
        let (id, bits) = self.challenge("create").await.expect("a challenge");
        let n = scatter_challenge::solve(&id, bits);
        self.create(
            name,
            password,
            &format!("&tschallengeid={}&tschallengesolution={n}{extra}", enc(&id)),
        )
        .await
    }

    async fn login(&mut self, name: &str, password: &str, extra: &str) -> Value {
        let token = self.token("login").await;
        self.post(&format!(
            "action=clientlogin&username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/{extra}",
            enc(name),
            enc(password),
            enc(&token)
        ))
        .await
    }

    async fn set(&mut self, settings: Value) {
        let token = self.token("csrf").await;
        let base = self.rest("GET", "/settings", None).await.body["base"].clone();
        let r = self
            .rest(
                "PATCH",
                "/settings",
                Some(json!({"set": settings, "base": base, "token": token})),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn registration_and_challenges() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsregistration-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: Some("Library Base".into()),
        farm_slug: None,
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
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: Farm {
                slug: TENANT.into(),
                base: format!("https://{HOST}"),
            },
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let router = router_with(app, triplespace_api_rest::routes());

    // siteinfo says what a client will be asked, before it asks.
    let mut anon = Client::new(&router, [203, 0, 113, 9]);
    let si = anon
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    let t = &si["query"]["triplespace"];
    assert_eq!(
        t["challenge"]["triggers"],
        json!(["createaccount", "tempaccount", "badlogin"])
    );
    assert_eq!(t["challenge"]["algorithm"], "sha256-leading-zero-bits");
    assert_eq!(t["challenge"]["bits"], 20);
    assert_eq!(t["account"]["approval"], false);

    // The owner lowers the difficulty for the test.
    let mut owner = Client::new(&router, [198, 51, 100, 1]);
    assert_eq!(
        owner.login("Alice", "owner-secret-1234", "").await["clientlogin"]["status"],
        "PASS"
    );
    owner
        .set(json!({"challenge.bits": 8, "challenge.max_bits": 10}))
        .await;

    // authmanagerinfo lists the fields, the challenge among them, bound to a session.
    let v = anon
        .api("action=query&meta=authmanagerinfo&amirequestsfor=create&amimergerequestfields=1")
        .await;
    let info = &v["query"]["authmanagerinfo"];
    assert_eq!(info["cancreateaccounts"], true);
    for f in [
        "username",
        "password",
        "retype",
        "tschallengeid",
        "tschallengesolution",
    ] {
        assert!(info["fields"].get(f).is_some(), "{f}: {info}");
    }
    let ids: Vec<&str> = info["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert!(ids.contains(&"TriplespaceChallengeAuthenticationRequest"));
    assert!(anon.cookie.is_some(), "a session to bind the challenge to");

    // Without a solution: UI, with a fresh challenge.
    let r = anon.create("Bob", "bob-password-1", "").await;
    assert_eq!(r["createaccount"]["status"], "UI", "{r}");
    assert_eq!(r["createaccount"]["messagecode"], "ts-challenge-required");
    assert_eq!(r["createaccount"]["challenge"]["bits"], 8);
    // Fields are checked first, so a typo spends no solution.
    let (id, bits) = anon.challenge("create").await.unwrap();
    let n = scatter_challenge::solve(&id, bits);
    let solved = format!("&tschallengeid={}&tschallengesolution={n}", enc(&id));
    let token = anon.token("createaccount").await;
    let r = anon
        .post(&format!(
            "action=createaccount&username=Bob&password=bob-password-1&retype=other-password&createtoken={}&createreturnurl=https://{HOST}/{solved}",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "badretype", "{r}");
    let r = anon.create("Bob", "short", &solved).await;
    assert_eq!(r["createaccount"]["messagecode"], "passwordtooshort");
    let r = anon.create("Bad|Name", "bob-password-1", &solved).await;
    assert_eq!(r["createaccount"]["messagecode"], "noname");
    let r = anon.create("alice", "bob-password-1", &solved).await;
    assert_eq!(
        r["createaccount"]["messagecode"], "userexists",
        "names compare case-insensitively"
    );
    // A wrong solution is refused, and spends nothing.
    let wrong = (0..u64::MAX)
        .find(|m| !scatter_challenge::meets(&id, *m, bits))
        .unwrap();
    let r = anon
        .create(
            "Bob",
            "bob-password-1",
            &format!("&tschallengeid={}&tschallengesolution={wrong}", enc(&id)),
        )
        .await;
    assert_eq!(r["createaccount"]["status"], "UI");
    // The solution: the account is made.
    let r = anon.create("Bob", "bob-password-1", &solved).await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    assert_eq!(r["createaccount"]["username"], "Bob");
    // Spent: the same solution again is refused.
    let r = anon.create("Carol", "carol-password-1", &solved).await;
    assert_eq!(r["createaccount"]["status"], "UI");
    assert!(
        r["createaccount"]["message"]
            .as_str()
            .unwrap()
            .contains("used already")
    );
    // Another session cannot use a challenge issued to this one.
    let mut other = Client::new(&router, [203, 0, 113, 9]);
    let r = other.create("Carol", "carol-password-1", &solved).await;
    assert_eq!(r["createaccount"]["status"], "UI");
    // The new account logs in; its first record is its own.
    let mut bob = Client::new(&router, [203, 0, 113, 9]);
    assert_eq!(
        bob.login("Bob", "bob-password-1", "").await["clientlogin"]["status"],
        "PASS"
    );
    let ui = bob.api("action=query&meta=userinfo").await;
    assert_eq!(ui["query"]["userinfo"]["name"], "Bob");

    // The difficulty rises with the accounts an address has made, up to the ceiling.
    let (_, bits) = anon.challenge("create").await.unwrap();
    assert_eq!(bits, 9);
    for i in 0..5 {
        let r = anon
            .create_solved(&format!("Many {i}"), "many-password-1", "")
            .await;
        assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    }
    let (_, bits) = anon.challenge("create").await.unwrap();
    assert_eq!(bits, 10, "the ceiling");
    // Six a day from one address.
    let r = anon.create_solved("Seventh", "many-password-1", "").await;
    assert_eq!(
        r["createaccount"]["messagecode"], "acct_creation_throttle_hit",
        "{r}"
    );
    // An IPv6 caller is counted by its /64 (unit-tested in triplespace-accounts).

    // badlogin: three failures from an address, then a challenge.
    let mut guesser = Client::new(&router, [192, 0, 2, 50]);
    for _ in 0..3 {
        let r = guesser.login("Bob", "wrong-password", "").await;
        assert_eq!(r["clientlogin"]["messagecode"], "wrongpassword", "{r}");
    }
    let r = guesser.login("Bob", "bob-password-1", "").await;
    assert_eq!(r["clientlogin"]["status"], "UI", "{r}");
    assert_eq!(r["clientlogin"]["messagecode"], "ts-challenge-required");
    let id = r["clientlogin"]["challenge"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let bits = u8::try_from(r["clientlogin"]["challenge"]["bits"].as_u64().unwrap()).unwrap();
    let n = scatter_challenge::solve(&id, bits);
    let r = guesser
        .login(
            "Bob",
            "bob-password-1",
            &format!("&tschallengeid={}&tschallengesolution={n}", enc(&id)),
        )
        .await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    // authmanagerinfo for login lists the challenge to an address that has failed.
    let mut guesser2 = Client::new(&router, [192, 0, 2, 50]);
    assert!(guesser2.challenge("login").await.is_some());
    let mut fresh = Client::new(&router, [192, 0, 2, 51]);
    assert!(fresh.challenge("login").await.is_none());

    // Approval: a request is filed, holds its name, and is learned of at login.
    owner
        .set(json!({"account.approval": true, "account.request_statement": "required"}))
        .await;
    let si = fresh
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(si["query"]["triplespace"]["account"]["approval"], true);
    let v = fresh
        .api("action=query&meta=authmanagerinfo&amirequestsfor=create&amimergerequestfields=1")
        .await;
    assert!(
        v["query"]["authmanagerinfo"]["fields"]
            .get("tsstatement")
            .is_some()
    );
    let r = fresh.create_solved("Dora", "dora-password-1", "").await;
    assert_eq!(
        r["createaccount"]["messagecode"], "ts-account-statement-required",
        "{r}"
    );
    let r = fresh
        .create_solved(
            "Dora",
            "dora-password-1",
            &format!("&tsstatement={}", enc("A librarian at the county library.")),
        )
        .await;
    assert_eq!(r["createaccount"]["status"], "FAIL", "{r}");
    assert_eq!(r["createaccount"]["messagecode"], "ts-account-requested");
    let r = fresh
        .create_solved("dora", "dora-password-2", "&tsstatement=x")
        .await;
    assert_eq!(
        r["createaccount"]["messagecode"], "userexists",
        "a pending request holds its name"
    );
    let r = fresh.login("Dora", "dora-password-1", "").await;
    assert_eq!(r["clientlogin"]["messagecode"], "ts-account-pending", "{r}");
    let r = fresh.login("Dora", "not-her-password", "").await;
    assert_eq!(
        r["clientlogin"]["messagecode"], "wrongpassword",
        "a stranger learns nothing"
    );
    let r = fresh
        .create_solved("Eve", "eve-password-1", "&tsstatement=spam")
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "ts-account-requested");

    // Review: only with confirmaccount.
    let r = bob.rest("GET", "/account/requests", None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = owner.rest("GET", "/account/requests", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["count"], 2);
    let first = &r.body["requests"][0];
    assert_eq!(first["name"], "Dora");
    assert_eq!(first["statement"], "A librarian at the county library.");
    assert_eq!(first["address"], "192.0.2.51", "the owner holds ts-viewip");
    let dora = first["id"].as_str().unwrap().to_string();
    let eve = r.body["requests"][1]["id"].as_str().unwrap().to_string();
    let ui = owner
        .api("action=query&meta=userinfo&uiprop=accountrequests")
        .await;
    assert_eq!(ui["query"]["userinfo"]["accountrequests"], 2);
    let token = owner.token("csrf").await;
    let r = owner
        .rest(
            "POST",
            &format!("/account/requests/{dora}/approve"),
            Some(json!({})),
        )
        .await;
    assert_eq!(r.body["code"], "badtoken", "{}", r.body);
    let r = owner
        .rest(
            "POST",
            &format!("/account/requests/{dora}/approve"),
            Some(json!({"token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["approved"]["name"], "Dora");
    let r = fresh.login("Dora", "dora-password-1", "").await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let r = owner
        .rest(
            "POST",
            &format!("/account/requests/{eve}/reject"),
            Some(json!({"token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-reason-required");
    let r = owner
        .rest(
            "POST",
            &format!("/account/requests/{eve}/reject"),
            Some(json!({"token": token, "reason": "Not a librarian."})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = fresh.login("Eve", "eve-password-1", "").await;
    assert_eq!(r["clientlogin"]["messagecode"], "ts-account-rejected");
    assert_eq!(r["clientlogin"]["reason"], "Not a librarian.");
    let r = owner
        .rest(
            "POST",
            &format!("/account/requests/{eve}/approve"),
            Some(json!({"token": token})),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::NOT_FOUND,
        "a decided request is closed"
    );

    // A holder of confirmaccount and skipcaptcha creates for another person at once.
    let r = owner.create("Frank", "frank-password-1", "").await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    let mut frank = Client::new(&router, [192, 0, 2, 52]);
    assert_eq!(
        frank.login("Frank", "frank-password-1", "").await["clientlogin"]["status"],
        "PASS"
    );

    // With the triggers empty, nothing is asked.
    owner
        .set(json!({"challenge.triggers": [], "account.approval": false}))
        .await;
    let mut plain = Client::new(&router, [192, 0, 2, 53]);
    assert!(plain.challenge("create").await.is_none());
    let r = plain.create("Gina", "gina-password-1", "").await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
}
