//! Registration through the site (ADR 0087 §5, §6), over the real API: the header's link,
//! `Special:CreateAccount` with its challenge, its `<noscript>` explanation and the error
//! a submission without a solution gets, the honeypot, the login that follows a creation,
//! the `badlogin` challenge on `Special:UserLogin`, and with approval on, the request,
//! the reviewer's menu item and `Special:ConfirmAccounts`. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;
use triplespace_client::{Client, ServiceTransport};

const DB: &str = "tsregpages_test";
const HOST: &str = "librarybase.org";

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

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

struct Got {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Got {
    fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }

    /// The value of the first input of a name.
    fn field(&self, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let at = self
            .body
            .find(&marker)
            .unwrap_or_else(|| panic!("no {name} field in {}", self.body));
        let rest = &self.body[at + marker.len()..];
        rest[..rest.find('"').unwrap()]
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }
}

/// A browser: the site's router, one cookie, one address.
struct Browser {
    router: Router,
    cookie: Option<String>,
    address: [u8; 4],
}

impl Browser {
    async fn send(
        &mut self,
        method: &str,
        path: &str,
        body: Option<String>,
        json_body: bool,
    ) -> Got {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if body.is_some() {
            req = req.header(
                header::CONTENT_TYPE,
                if json_body {
                    "application/json"
                } else {
                    "application/x-www-form-urlencoded"
                },
            );
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((self.address, 50000))));
        let r = self.router.clone().oneshot(req).await.unwrap();
        let status = r.status();
        let headers = r.headers().clone();
        for sc in headers.get_all(header::SET_COOKIE) {
            if let Some(v) = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
            {
                self.cookie = (!v.is_empty()).then(|| v.to_string());
            }
        }
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        Got {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&mut self, path: &str) -> Got {
        self.send("GET", path, None, false).await
    }

    async fn post(&mut self, path: &str, pairs: &[(&str, &str)]) -> Got {
        self.send("POST", path, Some(form(pairs)), false).await
    }

    /// Saves settings through the REST API with the browser's session.
    async fn set(&mut self, settings: Value) {
        let token = self.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
        let r = self
            .send(
                "PATCH",
                "/w/rest.php/triplespace/v0/settings",
                Some(json!({"set": settings, "token": token}).to_string()),
                true,
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    }

    async fn api(&mut self, query: &str) -> Value {
        let r = self
            .get(&format!("/w/api.php?{query}&format=json&formatversion=2"))
            .await;
        serde_json::from_str(&r.body).unwrap()
    }
}

/// The solution of the challenge a page's form carries.
fn solve(page: &Got) -> (String, String) {
    let id = page.field("tschallengeid");
    let bits = scatter_challenge::Challenge::parse(&id).unwrap().bits;
    let n = scatter_challenge::solve(&id, bits).to_string();
    (id, n)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn registration_through_the_site() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsregpages-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "lbwiki".into(),
        base: format!("https://{HOST}"),
        name: Some("Librarybase".into()),
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
        triplespace_db::pool::pool(&database, 4).unwrap(),
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
    let api = router_with(app, triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(Client::new(ServiceTransport::new(
            api.clone(),
        ))));

    // The owner logs in through the site and lowers the difficulty for the test.
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
        address: [198, 51, 100, 1],
    };
    let login = owner.get("/wiki/Special:UserLogin").await;
    let r = owner
        .post(
            "/w/index.php?title=Special:UserLogin",
            &[
                ("logintoken", &login.field("logintoken")),
                ("returnto", "Special:SpecialPages"),
                ("username", "Alice"),
                ("password", "owner-secret-1234"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    owner
        .set(json!({"challenge.bits": 8, "challenge.max_bits": 8}))
        .await;

    // A reader: the header offers the way in, and the form carries its challenge.
    let mut reader = Browser {
        router: site.clone(),
        cookie: None,
        address: [203, 0, 113, 20],
    };
    let home = reader.get("/wiki/Special:SpecialPages").await;
    assert!(
        home.body
            .contains("href=\"/wiki/Special:CreateAccount?returnto=Special:SpecialPages\""),
        "the header's link"
    );
    let page = reader
        .get("/wiki/Special:CreateAccount?returnto=Special:SpecialPages")
        .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(
        page.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("private")
    );
    assert!(page.body.contains("data-ts-challenge=\"8\""));
    assert!(page.body.contains("<noscript>"));
    assert!(page.body.contains("Turn on JavaScript for this site"));
    assert!(page.body.contains("name=\"wpWebsite\""), "the honeypot");
    let token = page.field("createtoken");

    // Without a solution, as a browser without JavaScript sends it: the explanation.
    let r = reader
        .post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("createtoken", &token),
                ("returnto", "Special:SpecialPages"),
                ("username", "Bob"),
                ("password", "bob-password-1"),
                ("retype", "bob-password-1"),
                ("tschallengeid", &page.field("tschallengeid")),
                ("tschallengesolution", ""),
                ("wpWebsite", ""),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("cdx-message--error"), "{}", r.body);
    assert!(r.body.contains("Turn on JavaScript for this site"));
    assert!(r.body.contains("value=\"Bob\""), "the name is kept");

    // The honeypot: refused, and the API is not asked.
    let (id, n) = solve(&r);
    let honey = reader
        .post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("createtoken", &token),
                ("returnto", "Special:SpecialPages"),
                ("username", "Bob"),
                ("password", "bob-password-1"),
                ("retype", "bob-password-1"),
                ("tschallengeid", &id),
                ("tschallengesolution", &n),
                ("wpWebsite", "http://spam.example"),
            ],
        )
        .await;
    assert!(honey.body.contains("The account could not be created."));

    // A typo keeps the challenge and its solution, which were not spent.
    let typo = reader
        .post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("createtoken", &token),
                ("returnto", "Special:SpecialPages"),
                ("username", "Bob"),
                ("password", "bob-password-1"),
                ("retype", "bob-password-2"),
                ("tschallengeid", &id),
                ("tschallengesolution", &n),
                ("wpWebsite", ""),
            ],
        )
        .await;
    assert!(
        typo.body
            .contains("The passwords you entered do not match.")
    );
    assert_eq!(typo.field("tschallengesolution"), n);

    // Solved: created, logged in, and taken back.
    let r = reader
        .post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("createtoken", &token),
                ("returnto", "Special:SpecialPages"),
                ("username", "Bob"),
                ("password", "bob-password-1"),
                ("retype", "bob-password-1"),
                ("tschallengeid", &id),
                ("tschallengesolution", &n),
                ("wpWebsite", ""),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(r.location(), "/wiki/Special:SpecialPages");
    let me = reader.get("/wiki/Special:SpecialPages").await;
    assert!(me.body.contains(">Bob</summary>"), "logged in as Bob");

    // badlogin: after three failures from an address, the login form carries a challenge.
    let mut guesser = Browser {
        router: site.clone(),
        cookie: None,
        address: [192, 0, 2, 9],
    };
    for _ in 0..3 {
        let l = guesser.get("/wiki/Special:UserLogin").await;
        assert!(!l.body.contains("tschallengeid"));
        let r = guesser
            .post(
                "/w/index.php?title=Special:UserLogin",
                &[
                    ("logintoken", &l.field("logintoken")),
                    ("username", "Bob"),
                    ("password", "wrong-password"),
                ],
            )
            .await;
        assert!(r.body.contains("cdx-message--error"));
    }
    let l = guesser.get("/wiki/Special:UserLogin").await;
    assert!(l.body.contains("data-ts-challenge=\"8\""), "{}", l.body);
    assert!(l.body.contains("wait a few minutes"));
    let (id, n) = solve(&l);
    let r = guesser
        .post(
            "/w/index.php?title=Special:UserLogin",
            &[
                ("logintoken", &l.field("logintoken")),
                ("username", "Bob"),
                ("password", "bob-password-1"),
                ("tschallengeid", &id),
                ("tschallengesolution", &n),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);

    // Approval: the form says so, a request is filed, and the reviewer sees it.
    owner
        .set(json!({"account.approval": true, "account.request_statement": "required"}))
        .await;
    let mut asker = Browser {
        router: site.clone(),
        cookie: None,
        address: [192, 0, 2, 77],
    };
    let page = asker.get("/wiki/Special:RequestAccount").await;
    assert!(page.body.contains("This wiki reviews new accounts."));
    assert!(page.body.contains("name=\"tsstatement\""));
    let (id, n) = solve(&page);
    let r = asker
        .post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("createtoken", &page.field("createtoken")),
                ("username", "Dora"),
                ("password", "dora-password-1"),
                ("retype", "dora-password-1"),
                ("tsstatementmode", "required"),
                ("tsstatement", "A librarian."),
                ("tschallengeid", &id),
                ("tschallengesolution", &n),
                ("wpWebsite", ""),
            ],
        )
        .await;
    assert!(
        r.body
            .contains("Your request for an account has been filed."),
        "{}",
        r.body
    );
    let menu = owner.get("/wiki/Special:SpecialPages").await;
    assert!(menu.body.contains("Account requests (1)"), "{}", menu.body);
    let review = owner.get("/wiki/Special:ConfirmAccounts").await;
    assert_eq!(review.status, StatusCode::OK, "{}", review.body);
    assert!(review.body.contains("<h2 id=\"req-"));
    assert!(review.body.contains("A librarian."));
    assert!(review.body.contains("192.0.2.77"));
    let r = owner
        .post(
            "/w/index.php?title=Special:ConfirmAccounts",
            &[
                ("token", &review.field("token")),
                ("id", &review.field("id")),
                ("decision", "approve"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(r.location(), "/wiki/Special:ConfirmAccounts?done=approved");
    let after = owner.get(r.location()).await;
    assert!(after.body.contains("The request has been approved"));
    assert!(after.body.contains("There are no account requests waiting"));
    // A reader who may not review sees no list.
    let denied = reader.get("/wiki/Special:ConfirmAccounts").await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.body);
}
