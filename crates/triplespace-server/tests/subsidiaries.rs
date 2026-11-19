//! Subsidiary accounts as their operator manages them (ADR 0024 §2, §4, §8, §9): the
//! `/account/subsidiaries` routes over a real database: creating one, the name rules and
//! the CSRF token, issuing a key whose secret is shown once and logs in as a bot password
//! and as a bearer key, revoking it, renaming, retiring (which revokes every key) and
//! reactivating; a subsidiary operates none; and `Special:Account`'s Subsidiaries section
//! drawn and posted without JavaScript (07 §4.2, §4.4, §5; 19 §2.4).
//! Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

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
use triplespace_client::{Client as SiteClient, ServiceTransport};

const DB: &str = "tssubsidiaries_test";
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
    bearer: Option<String>,
}

struct Reply {
    status: StatusCode,
    body: Value,
    text: String,
}

impl Client {
    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Option<String>,
        extra: &[(header::HeaderName, String)],
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        for (name, value) in extra {
            req = req.header(name, value);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        if let Some(b) = &self.bearer {
            req.headers_mut().insert(
                header::AUTHORIZATION,
                format!("Bearer {b}").parse().unwrap(),
            );
        }
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
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
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply { status, body, text }
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

    /// A write with the session's CSRF token.
    async fn write(&mut self, method: &str, path: &str, mut body: Value) -> Reply {
        let token = self.csrf().await;
        body["token"] = json!(token);
        self.rest(method, path, Some(body)).await
    }

    /// `action=login` with a bot password; the answer's `result`.
    async fn bot_login(&mut self, lgname: &str, password: &str) -> Value {
        let t = self.api("action=query&meta=tokens&type=login").await;
        let token = t["query"]["tokens"]["logintoken"]
            .as_str()
            .unwrap()
            .to_string();
        self.api_post(&format!(
            "action=login&lgname={}&lgpassword={}&lgtoken={}",
            enc(lgname),
            enc(password),
            enc(&token)
        ))
        .await["login"]
            .clone()
    }

    async fn page(&mut self, path: &str) -> Reply {
        self.send("GET", path, None, None, &[]).await
    }

    async fn page_post(&mut self, path: &str, pairs: &[(&str, &str)]) -> Reply {
        let form = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.send(
            "POST",
            path,
            Some("application/x-www-form-urlencoded"),
            Some(form),
            &[],
        )
        .await
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn subsidiaries_and_their_keys() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssubsidiaries-{}", std::process::id()));
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
    let api = router_with(app.clone(), triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(SiteClient::new(
            ServiceTransport::new(api.clone()),
        )));
    let mut c = Client {
        router: site.clone(),
        cookie: None,
        bearer: None,
    };

    // Subsidiaries belong to an account (0024 §8).
    let r = c.rest("GET", "/account/subsidiaries", None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);

    let tokens = c.api("action=query&meta=tokens&type=login").await;
    let login = tokens["query"]["tokens"]["logintoken"]
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

    let r = c.rest("GET", "/account/subsidiaries", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let before = r.body["subsidiaries"].as_array().unwrap().len();
    assert_eq!(r.body["can_create"], true);
    assert_eq!(r.body["max"], 10);
    assert_eq!(r.body["suggested"], "AliceBot");
    assert!(
        r.body["grants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["name"] == "editentity"),
        "{}",
        r.body
    );

    // 07 §4.2: a write needs its token, and a name the name rules.
    let r = c
        .rest(
            "POST",
            "/account/subsidiaries",
            Some(json!({"name": "AliceBot"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "badtoken");
    let r = c
        .write("POST", "/account/subsidiaries", json!({"name": "Bad@Name"}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "invaliduser");
    let r = c
        .write("POST", "/account/subsidiaries", json!({"name": "aliceBot"}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["created"]["name"], "AliceBot");
    let bot = r.body["created"]["actor"].as_str().unwrap().to_string();
    let r = c
        .write("POST", "/account/subsidiaries", json!({"name": "Alice"}))
        .await;
    assert_eq!(r.body["code"], "userexists", "{}", r.body);

    // It is active, never pending, and holds no group until a bureaucrat flags it (07 §4.3).
    let r = c.rest("GET", "/account/subsidiaries", None).await;
    let subs = r.body["subsidiaries"].as_array().unwrap();
    assert_eq!(subs.len(), before + 1);
    let mine = subs.iter().find(|s| s["actor"] == bot.as_str()).unwrap();
    assert_eq!(mine["name"], "AliceBot");
    assert_eq!(mine["status"], "active");
    assert_eq!(mine["groups"], json!([]));
    assert_eq!(mine["keys"], json!([]));
    // 07 §5: a key; the secret is in this answer alone.
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/AliceBot/keys",
            json!({"label": "bad label"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "ts-bad-key");
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/AliceBot/keys",
            json!({"label": "laptop", "grants": ["editentity"], "expires_days": 30}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let issued = r.body["issued"].clone();
    assert_eq!(issued["login"], "AliceBot@laptop");
    let secret = issued["secret"].as_str().unwrap().to_string();
    let bearer = issued["bearer"].as_str().unwrap().to_string();
    let key_id = issued["key_id"].as_str().unwrap().to_string();
    let r = c
        .rest("GET", "/account/subsidiaries/AliceBot/keys", None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!r.text.contains(&secret), "the secret is never listed");
    let listed = &r.body["keys"][0];
    assert_eq!(listed["label"], "laptop");
    assert_eq!(listed["grants"], json!(["basic", "editentity"]));
    assert!(
        listed["expires"].is_u64() && listed["revoked"].is_null(),
        "{listed}"
    );

    // The key logs in as a bot password (07 §5.2), and the subsidiary operates none.
    let mut tool = Client {
        router: site.clone(),
        cookie: None,
        bearer: None,
    };
    let answer = tool.bot_login("AliceBot@laptop", &secret).await;
    assert_eq!(answer["result"], "Success", "{answer}");
    assert_eq!(answer["lgusername"], "AliceBot");
    let r = tool.rest("GET", "/account/subsidiaries", None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "ts-not-primary");
    let mut keyed = Client {
        router: site.clone(),
        cookie: None,
        bearer: Some(bearer.clone()),
    };
    let ui = keyed.api("action=query&meta=userinfo").await;
    assert_eq!(ui["query"]["userinfo"]["name"], "AliceBot", "{ui}");

    // Revoking ends the key and its sessions (07 §5.5).
    let token = c.csrf().await;
    let r = c
        .rest(
            "DELETE",
            &format!(
                "/account/subsidiaries/AliceBot/keys?key_id={key_id}&token={}",
                enc(&token)
            ),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let ui = tool.api("action=query&meta=userinfo").await;
    assert_ne!(ui["query"]["userinfo"]["name"], "AliceBot", "{ui}");
    let answer = tool.bot_login("AliceBot@laptop", &secret).await;
    assert_eq!(answer["result"], "Failed", "{answer}");
    let r = c
        .rest(
            "DELETE",
            &format!(
                "/account/subsidiaries/AliceBot/keys?key_id={key_id}&token={}",
                enc(&token)
            ),
            None,
        )
        .await;
    assert_eq!(r.body["code"], "ts-no-key", "{}", r.body);

    // 07 §4.5: renaming releases the old name for good.
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/AliceBot/rename",
            json!({"to": "Alice Indexer"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["subsidiary"]["name"], "Alice Indexer");
    let r = c
        .write("POST", "/account/subsidiaries", json!({"name": "AliceBot"}))
        .await;
    assert_eq!(r.body["code"], "ts-name-reserved", "{}", r.body);
    let r = c
        .write("POST", "/account/subsidiaries/AliceBot/retire", json!({}))
        .await;
    assert_eq!(r.body["code"], "ts-no-subsidiary", "{}", r.body);

    // 07 §4.4: retiring revokes every key; a retired one takes none until reactivated.
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/keys",
            json!({"label": "server"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let secret2 = r.body["issued"]["secret"].as_str().unwrap().to_string();
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/retire",
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["subsidiary"]["status"], "retired");
    let answer = tool.bot_login("Alice Indexer@server", &secret2).await;
    assert_eq!(answer["result"], "Failed", "{answer}");
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/keys",
            json!({"label": "again"}),
        )
        .await;
    assert_eq!(r.body["code"], "ts-subsidiary-retired", "{}", r.body);
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/retire",
            json!({}),
        )
        .await;
    assert_eq!(r.body["code"], "ts-subsidiary-retired", "{}", r.body);
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/reactivate",
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["subsidiary"]["status"], "active");
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/reactivate",
            json!({}),
        )
        .await;
    assert_eq!(r.body["code"], "ts-subsidiary-active", "{}", r.body);

    // 0024 §9: Special:Account draws the section, and its forms post back to the page.
    let r = c.page("/wiki/Special:Account").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.text.contains("id=\"subsidiaries\""), "{}", r.text);
    assert!(r.text.contains("Alice Indexer"), "{}", r.text);
    assert!(
        r.text.contains("value=\"AliceBot\""),
        "the suggestion: {}",
        r.text
    );
    let token = c.csrf().await;
    let r = c
        .page_post(
            "/w/index.php?title=Special:Account",
            &[
                ("subsidiary_action", "create"),
                ("name", "Alice Reports"),
                ("token", &token),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.text);
    let r = c
        .page("/wiki/Special:Account?tsnotice=ts-subsidiary-created")
        .await;
    assert!(r.text.contains("Subsidiary created."), "{}", r.text);
    assert!(r.text.contains("Alice Reports"), "{}", r.text);
    let token = c.csrf().await;
    let r = c
        .page_post(
            "/w/index.php?title=Special:Account",
            &[
                ("subsidiary_action", "issue"),
                ("subsidiary", "Alice Reports"),
                ("label", "cron"),
                ("grant_editpage", "1"),
                ("token", &token),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.text.contains("Alice Reports@cron"), "{}", r.text);
    assert!(r.text.contains("shown only this once"), "{}", r.text);
    let shown = {
        let at = r
            .text
            .find("Bot password</dt><dd><code>")
            .expect("the secret");
        let rest = &r.text[at + "Bot password</dt><dd><code>".len()..];
        rest[..rest.find("</code>").unwrap()].to_string()
    };
    let answer = tool.bot_login("Alice Reports@cron", &shown).await;
    assert_eq!(answer["result"], "Success", "{answer}");
    // Shown once: the next view of the page has the key but not its secret.
    let r = c.page("/wiki/Special:Account").await;
    assert!(r.text.contains("Alice Reports@cron"), "{}", r.text);
    assert!(!r.text.contains(&shown), "{}", r.text);
    // A refusal is said on the page.
    let token = c.csrf().await;
    let r = c
        .page_post(
            "/w/index.php?title=Special:Account",
            &[
                ("subsidiary_action", "create"),
                ("name", "Alice"),
                ("token", &token),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.text.contains("That name is taken."), "{}", r.text);

    // 0024 §9 (19 §1.3; 0024 A24): the subsidiary's user page names its operator, even
    // before the page is written; the operator's does not.
    let mut reader = Client {
        router: site.clone(),
        cookie: None,
        bearer: None,
    };
    let r = reader.page("/wiki/User:Alice_Reports").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "a page not yet written");
    assert!(
        r.text
            .contains("Bot operated by <a href=\"/wiki/User:Alice\">Alice</a>"),
        "{}",
        r.text
    );
    assert!(!r.text.contains("ts-chip--bot"), "{}", r.text);
    assert!(!r.text.contains("Request bot approval"), "{}", r.text);
    let r = reader.page("/wiki/User:Alice").await;
    assert!(!r.text.contains("Bot operated by"), "{}", r.text);
    let u = reader
        .api(&format!(
            "action=query&list=users&ususers={}&usprop=operator|subsidiaries",
            enc("Alice|Alice Reports")
        ))
        .await;
    let users = u["query"]["users"].as_array().unwrap();
    let alice = users.iter().find(|x| x["name"] == "Alice").unwrap();
    assert!(
        alice["subsidiaries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "Alice Reports" && x["status"] == "active"),
        "{u}"
    );
    let reports = users.iter().find(|x| x["name"] == "Alice Reports").unwrap();
    assert_eq!(reports["operator"], "Alice", "{u}");
    assert_eq!(reports["subsidiarystatus"], "active", "{u}");
    assert!(reports.get("subsidiaries").is_none(), "{u}");

    // With `subsidiaries.request_page` set, the operator sees where to ask for the flag.
    let token = c.csrf().await;
    let base = c.rest("GET", "/settings", None).await.body["base"].clone();
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({
                "set": {"subsidiaries.request_page": "Project:Bot requests"},
                "base": base,
                "token": token,
            })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c.page("/wiki/User:Alice_Reports").await;
    assert!(
        r.text
            .contains("href=\"/wiki/Project:Bot_requests\">Request bot approval</a>"),
        "{}",
        r.text
    );
    let r = reader.page("/wiki/User:Alice_Reports").await;
    assert!(!r.text.contains("Request bot approval"), "{}", r.text);

    // A bureaucrat flags it: the Bot chip, and nothing left to request.
    let t = c
        .api("action=query&meta=tokens&type=userrights")
        .await["query"]["tokens"]["userrightstoken"]
        .as_str()
        .unwrap()
        .to_string();
    let ur = c
        .api_post(&format!(
            "action=userrights&user={}&add=bot&token={}",
            enc("Alice Reports"),
            enc(&t)
        ))
        .await;
    assert!(ur.get("error").is_none(), "{ur}");
    let r = c.page("/wiki/User:Alice_Reports").await;
    assert!(r.text.contains("ts-chip ts-chip--bot"), "{}", r.text);
    assert!(!r.text.contains("Request bot approval"), "{}", r.text);
    // Retired, it says so.
    let r = c
        .write(
            "POST",
            "/account/subsidiaries/Alice_Indexer/retire",
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = reader.page("/wiki/User:Alice_Indexer").await;
    assert!(r.text.contains("Retired bot operated by"), "{}", r.text);

    let _ = std::fs::remove_dir_all(&dir);
}
