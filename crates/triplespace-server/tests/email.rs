//! Email addresses and verification codes end to end (ADR 0100): the `/auth/email` routes
//! over a real database with the `log` transport; a code read back from the `.eml` the
//! delivery worker wrote; wrong, spent and correct codes; the sending limits; the primary
//! and the removal rules; the security notices staged to every verified address; the
//! Postmark webhook marking a bounced address; and `Special:Account`'s Email addresses
//! section drawn without JavaScript (0100 §2, §3, §4, §5, §6, §7, §8, §9, §10; 0100 A1,
//! A2).
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
use triplespace_api_action::{App, Config as ServerConfig, MailSettings, Mode, router_with};
use triplespace_cli::instance;
use triplespace_client::{Client as SiteClient, ServiceTransport};

const DB: &str = "tsemail_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "lbwiki";
const REST: &str = "/w/rest.php/triplespace/v0";
const FROM: &str = "noreply@librarybase.org";
const HOOK_SECRET: &str = "hook-secret-9";

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

    /// One of the email writes, with the token.
    async fn email(&mut self, action: &str, mut body: Value) -> Reply {
        let token = self.csrf().await;
        body["token"] = json!(token);
        let path = if action.is_empty() {
            "/auth/email".to_string()
        } else {
            format!("/auth/email/{action}")
        };
        self.rest("POST", &path, Some(body)).await
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

/// Runs the delivery worker once over the `log` transport and returns every message
/// written so far, oldest first, as `(recipient, text)`.
async fn deliver(
    database: &str,
    transport: &scatter_mail::log::LogTransport,
) -> (triplespace_notify::mail::Pass, Vec<(String, String)>) {
    let (mut client, connection) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let pass = triplespace_notify::mail::deliver_once(&mut client, transport)
        .await
        .unwrap();
    (pass, scatter_mail::log::read_all(transport.dir()).unwrap())
}

/// Moves an address's last code back two minutes, past the once-a-minute limit.
async fn backdate(database: &str, address: &str) {
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
        .execute(
            "UPDATE private.email SET code_sent = code_sent - interval '2 minutes' WHERE address_key = $1",
            &[&address],
        )
        .await
        .unwrap();
}

/// The code in a verification message: the six digits after `Your code: `.
fn code_in(text: &str) -> String {
    let at = text.find("Your code: ").expect("a code line");
    text[at + "Your code: ".len()..]
        .chars()
        .take(7)
        .collect::<String>()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn email_addresses_and_codes() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsemail-{}", std::process::id()));
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
    // 0100 §5: the log transport, the development sink; the handlers know only that
    // mail is sent and from where.
    app.set_mail(MailSettings {
        transport: "log".into(),
        from: Some(FROM.into()),
        postmark_webhook_secret: Some(HOOK_SECRET.into()),
    });
    let mail_dir = dir.join("mail");
    let transport = scatter_mail::log::LogTransport::new(&mail_dir).unwrap();
    let api = router_with(app.clone(), triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(SiteClient::new(
            ServiceTransport::new(api.clone()),
        )));
    let mut c = Client {
        router: site,
        cookie: None,
    };

    // 0100 §7: the addresses belong to a login; a reader has none to see.
    let r = c.rest("GET", "/auth/email", None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);

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

    let r = c.rest("GET", "/auth/email", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["addresses"], json!([]));
    assert_eq!(r.body["sends_mail"], true);
    assert_eq!(r.body["max_addresses"], 3);
    assert_eq!(r.body["required"], false);

    // A write needs its token, and an address its grammar (07 §2.9).
    let r = c
        .rest("POST", "/auth/email", Some(json!({"address": "a@b.org"})))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "badtoken");
    let r = c.email("", json!({"address": "alice at example"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "ts-email-invalid");

    // 0100 §2: adding an address stores it unverified and stages a code; the worker
    // writes the message to the log sink, from which the code is read.
    let r = c.email("", json!({"address": "Alice@Example.org"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["address"], "Alice@example.org");
    assert_eq!(r.body["verified"], false);
    assert_eq!(r.body["sent"], true);
    assert!(r.body["code_expires"].is_u64());
    let (pass, mails) = deliver(&database, &transport).await;
    assert_eq!(pass.accepted, 1, "{pass:?}");
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].0, "Alice@example.org");
    let text = &mails[0].1;
    assert!(
        text.contains("Subject: Your verification code for Library Base"),
        "{text}"
    );
    assert!(
        text.contains("From: \"Library Base\" <noreply@librarybase.org>"),
        "{text}"
    );
    assert!(text.contains("Auto-Submitted: auto-generated"), "{text}");
    assert!(
        !text.contains("Alice\r\n"),
        "the login name is never in a message: {text}"
    );
    let code = code_in(text);
    assert_eq!(code.len(), 7, "{code:?}");
    assert_eq!(&code[3..4], " ");
    // The subject carries no code.
    let subject = text.lines().find(|l| l.starts_with("Subject:")).unwrap();
    assert!(!subject.contains(&code));

    // Once a minute per address: a second code at once is refused with the wait.
    let r = c.email("", json!({"address": "alice@example.org"})).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    assert_eq!(r.body["code"], "ratelimited");
    assert!(r.body["wait"].as_u64().unwrap() <= 60);

    // Wrong entries count down; the right one, typed with its space, verifies and makes
    // the first verified address the primary.
    let r = c
        .email(
            "verify",
            json!({"address": "alice@example.org", "code": "000000"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "ts-email-code");
    assert_eq!(r.body["left"], 4);
    let r = c
        .email(
            "verify",
            json!({"address": "ALICE@example.org", "code": code}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["verified"], true);
    assert_eq!(r.body["primary"], true);
    assert!(r.body.get("code_expires").is_none());
    // 0100 §4: the security notice went to every verified address, this one included.
    let (pass, mails) = deliver(&database, &transport).await;
    assert_eq!(pass.accepted, 1, "{pass:?}");
    assert_eq!(mails.len(), 2);
    assert!(
        mails[1]
            .1
            .contains("Subject: Security notice from Library Base")
    );
    assert!(
        mails[1]
            .1
            .contains("was verified on your login on Library Base")
    );
    assert!(
        mails[1]
            .1
            .contains("https://librarybase.org/wiki/Special:Account")
    );

    // A verified address is left alone by another add, with nothing sent.
    let r = c.email("", json!({"address": "alice@example.org"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["sent"], false);

    // 0100 §3: a second address; spent codes; Make primary; the removal rules.
    let r = c.email("", json!({"address": "bob@example.org"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (_, mails) = deliver(&database, &transport).await;
    let bob_code = code_in(&mails[2].1);
    for left in (0..5).rev() {
        let r = c
            .email(
                "verify",
                json!({"address": "bob@example.org", "code": "999999"}),
            )
            .await;
        assert_eq!(r.body["left"], left, "{}", r.body);
    }
    // Spent: even the right code no longer works, and the page learns it has no code.
    let r = c
        .email(
            "verify",
            json!({"address": "bob@example.org", "code": bob_code}),
        )
        .await;
    assert_eq!(r.body["code"], "ts-email-code");
    assert_eq!(r.body["left"], 0);
    let r = c.rest("GET", "/auth/email", None).await;
    let bob = &r.body["addresses"][1];
    assert_eq!(bob["address"], "bob@example.org");
    assert!(bob.get("code_expires").is_none(), "{bob}");
    // The primary cannot go while an unverified address is the only other one? It can:
    // the rule counts verified addresses, and Alice has a password and no external
    // method, so nothing blocks it.
    assert_eq!(r.body["primary_removal"], Value::Null, "{}", r.body);
    // An unverified address cannot be the primary.
    let r = c
        .email("primary", json!({"address": "bob@example.org"}))
        .await;
    assert_eq!(r.body["code"], "ts-email-unverified");

    // A new code after the minute: the limit is per address, so reset the clock in the
    // store rather than wait.
    backdate(&database, "bob@example.org").await;
    let r = c.email("", json!({"address": "bob@example.org"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (_, mails) = deliver(&database, &transport).await;
    let bob_code = code_in(&mails[3].1);
    let r = c
        .email(
            "verify",
            json!({"address": "bob@example.org", "code": bob_code}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["verified"], true);
    assert_eq!(r.body["primary"], false);
    // Two verified addresses: the primary cannot be removed now.
    let r = c.rest("GET", "/auth/email", None).await;
    assert_eq!(r.body["primary_removal"], "ts-email-primary");
    let r = c
        .email("remove", json!({"address": "alice@example.org"}))
        .await;
    assert_eq!(r.body["code"], "ts-email-primary");
    // Make Bob primary: notices to both.
    let r = c
        .email("primary", json!({"address": "bob@example.org"}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["primary"], true);
    let (pass, mails) = deliver(&database, &transport).await;
    // The verification notice to both addresses (2), then the primary notice to both (2).
    assert_eq!(pass.accepted, 4, "{pass:?}");
    let last: Vec<&str> = mails[mails.len() - 2..]
        .iter()
        .map(|m| m.0.as_str())
        .collect();
    assert!(last.contains(&"bob@example.org") && last.contains(&"Alice@example.org"));
    assert!(
        mails[mails.len() - 1]
            .1
            .contains("was made the primary address")
    );
    // Remove Alice's: the notice reaches the removed address too.
    let r = c
        .email("remove", json!({"address": "alice@example.org"}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["removed"], true);
    let (pass, mails) = deliver(&database, &transport).await;
    assert_eq!(pass.accepted, 2, "{pass:?}");
    let last: Vec<&str> = mails[mails.len() - 2..]
        .iter()
        .map(|m| m.0.as_str())
        .collect();
    assert!(last.contains(&"Alice@example.org"), "{last:?}");
    assert!(
        mails[mails.len() - 1]
            .1
            .contains("was removed from your login")
    );
    let r = c.rest("GET", "/auth/email", None).await;
    assert_eq!(r.body["addresses"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["addresses"][0]["primary"], true);

    // 0100 A2: Postmark's webhook. Unauthenticated, refused; a hard bounce marks the
    // address, after which nothing is delivered to it and the page says so; a transient
    // bounce changes nothing.
    let bounce = |kind: &str, inactive: bool| {
        json!({"RecordType": "Bounce", "Type": kind, "TypeCode": 1, "Email": "bob@example.org",
               "Inactive": inactive, "MessageID": "m-1", "MessageStream": "outbound",
               "Description": "d"})
    };
    let r = c
        .send(
            "POST",
            &format!("{REST}/mail/postmark"),
            Some("application/json"),
            Some(bounce("HardBounce", true).to_string()),
            &[],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);
    let auth = {
        use base64::Engine as _;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("postmark:{HOOK_SECRET}"))
        )
    };
    let r = c
        .send(
            "POST",
            &format!("{REST}/mail/postmark"),
            Some("application/json"),
            Some(bounce("Transient", false).to_string()),
            &[(header::AUTHORIZATION, auth.clone())],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["marked"], 0);
    let r = c
        .send(
            "POST",
            &format!("{REST}/mail/postmark"),
            Some("application/json"),
            Some(bounce("HardBounce", true).to_string()),
            &[(header::AUTHORIZATION, auth)],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["marked"], 1);
    let r = c.rest("GET", "/auth/email", None).await;
    assert_eq!(r.body["addresses"][0]["undeliverable"], "bounce");
    assert_eq!(r.body["addresses"][0]["primary"], true);
    // Confirming it again: a new code, and the right one clears the mark.
    backdate(&database, "bob@example.org").await;
    let r = c.email("", json!({"address": "bob@example.org"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["sent"], true);
    let (_, mails) = deliver(&database, &transport).await;
    let bob_code = code_in(&mails[mails.len() - 1].1);
    let r = c
        .email(
            "verify",
            json!({"address": "bob@example.org", "code": bob_code}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.get("undeliverable").is_none(), "{}", r.body);

    // 0100 §6: Special:Account draws the section, and its forms post back to the page.
    let r = c.page("/wiki/Special:Account").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("id=\"email-addresses\""), "{}", r.text);
    assert!(r.text.contains("bob@example.org"));
    assert!(r.text.contains(">primary<"), "{}", r.text);
    assert!(
        r.text.contains("name=\"email_action\" value=\"add\""),
        "{}",
        r.text
    );
    let token = c.csrf().await;
    let r = c
        .page_post(
            "/w/index.php?title=Special:Account",
            &[
                ("email_action", "add"),
                ("address", "carol@example.org"),
                ("token", &token),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.text);
    let r = c
        .page("/wiki/Special:Account?tsnotice=ts-email-added")
        .await;
    assert!(
        r.text.contains("A code was sent to the address"),
        "{}",
        r.text
    );
    assert!(r.text.contains("carol@example.org"));
    assert!(r.text.contains("name=\"code\""), "{}", r.text);
    let (_, mails) = deliver(&database, &transport).await;
    let carol_code = code_in(&mails[mails.len() - 1].1);
    let r = c
        .page_post(
            "/w/index.php?title=Special:Account",
            &[
                ("email_action", "verify"),
                ("address", "carol@example.org"),
                ("code", "123 123"),
                ("token", &token),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.text.contains("Wrong code. 4 more entries"), "{}", r.text);
    let r = c
        .page_post(
            "/w/index.php?title=Special:Account",
            &[
                ("email_action", "verify"),
                ("address", "carol@example.org"),
                ("code", &carol_code),
                ("token", &token),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.text);
    let r = c.page("/wiki/Special:Account").await;
    assert!(r.text.contains("Make primary"), "{}", r.text);
    assert!(
        r.text
            .contains("Make another address primary before removing"),
        "{}",
        r.text
    );

    // Removing the last verified address: Alice has a password and no external method,
    // so both can go, and the page then warns that the login cannot be recovered.
    let r = c
        .email("remove", json!({"address": "carol@example.org"}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c
        .email("remove", json!({"address": "bob@example.org"}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c.page("/wiki/Special:Account").await;
    assert!(r.text.contains("no verified email address"), "{}", r.text);

    // The queue holds nothing it has sent (16 §5.7).
    {
        let (client, connection) = database
            .parse::<Config>()
            .unwrap()
            .connect(NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let _ = deliver(&database, &transport).await;
        let queued = triplespace_notify::mail::queued(&client).await.unwrap();
        assert!(queued.is_empty(), "{queued:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
