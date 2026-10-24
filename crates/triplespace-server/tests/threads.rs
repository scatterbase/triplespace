//! The thread routes end to end (0019 §9; 18 §3.2): starting a thread on a page's talk
//! page, reading it in tree order, replies, statuses, collapsing, editing with a base,
//! pinning and the talk page's listing, and the refusals. Needs
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

const DB: &str = "tsthreads_test";
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
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn threads_over_rest() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsthreads-{}", std::process::id()));
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
    let writer_app = app.clone();
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    };

    // Anyone may read; writing needs an account.
    let r = c
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Hi", "target": {"kind": "page", "id": 1}, "text": "x"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(
        c.rest("GET", "/thread/999999", None).await.status,
        StatusCode::NOT_FOUND
    );

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

    // A page to talk about.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=What%20we%20cover.&createonly=1&token={}",
            enc(&token)
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().expect("the page was created");

    // A session's write needs its token.
    let start = json!({"subject": "why_is P31 wrong", "target": {"kind": "page", "id": page}, "text": "Because."});
    let r = c.rest("POST", "/thread", Some(start.clone())).await;
    assert_eq!(r.body["code"], "badtoken", "{}", r.body);

    let mut with = start.clone();
    with["token"] = json!(token);
    let r = c.rest("POST", "/thread", Some(with)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let opening = r.body["revid"].as_u64().unwrap();
    assert!(
        r.body["title"]
            .as_str()
            .unwrap()
            .ends_with("/Why is P31 wrong"),
        "{}",
        r.body
    );

    // Replies, a status, and a collapse.
    let path = format!("/thread/{thread}");
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"inReplyTo": opening, "text": "Really?", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let reply = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"inReplyTo": reply, "text": "Yes.", "status": "resolved", "token": token})),
        )
        .await;
    let answer = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest("POST", &format!("{path}/post"), Some(json!({"text": "Off topic.", "collapse": {"first": reply, "last": answer}, "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"inReplyTo": 12_345_678, "text": "?", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-not-a-post", "{}", r.body);
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"text": "x", "status": "nonsense", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-unknown-status", "{}", r.body);

    // The thread in tree order.
    let r = c.rest("GET", &path, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let t = &r.body;
    assert_eq!(t["subject"], "Why is P31 wrong");
    assert_eq!(t["status"], "resolved");
    assert_eq!(t["category"], "closed");
    assert_eq!(t["home"]["talk_page"]["ns"], 5, "{t}");
    assert_eq!(t["home"]["talk_page"]["title"], "Scope");
    let posts = t["posts"].as_array().unwrap();
    let ids: Vec<u64> = posts.iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids[..3], [opening, reply, answer]);
    assert_eq!(posts[2]["depth"], 2);
    assert_eq!(posts[1]["collapsed"], true);
    assert_eq!(posts[0]["content"], "Because.");
    assert_eq!(posts[3]["collapse"]["first"], reply);
    let base = t["base"].as_u64().unwrap();

    // An edit needs a base, and the thread as it stands.
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{opening}"),
            Some(json!({"text": "Because, really.", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-base-required", "{}", r.body);
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{opening}"),
            Some(json!({"text": "Because, really.", "base": 1, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{opening}"),
            Some(json!({"text": "Because, really.", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let base = r.body["base"].as_u64().unwrap();
    let r = c.rest("GET", &path, None).await;
    assert_eq!(r.body["posts"][0]["content"], "Because, really.");
    assert!(r.body["posts"][0]["updated"].is_string());

    // Pin it, and read the talk page's listing: a second thread, pinned first.
    let r = c
        .rest("POST", &format!("{path}/pin"), Some(json!({"target": {"kind": "page", "id": page}, "base": base, "summary": "FAQ", "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let mut second = start.clone();
    second["subject"] = json!("Another question");
    second["token"] = json!(token);
    assert_eq!(
        c.rest("POST", "/thread", Some(second)).await.status,
        StatusCode::CREATED
    );
    let talk = c.rest("GET", &path, None).await.body["home"]["talk_page_id"]
        .as_u64()
        .unwrap();
    let r = c.rest("GET", &format!("/page/{talk}/threads"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let listed = r.body["threads"].as_array().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["id"], thread);
    assert!(listed[0]["pinned"].is_string());
    assert_eq!(listed[0]["category"], "closed");
    assert_eq!(listed[1]["subject"], "Another question");
    assert_eq!(
        c.rest("GET", &format!("/page/{page}/threads"), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Rename keeps the date.
    let base = c.rest("GET", &path, None).await.body["base"]
        .as_u64()
        .unwrap();
    let r = c
        .rest(
            "POST",
            &format!("{path}/rename"),
            Some(json!({"subject": "P31 is fine", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body["title"].as_str().unwrap().ends_with("/P31 is fine"));

    // Every thread record is an activity row on the thread (16 §6.1). The talk page's
    // history is composite: its threads' rows, oldest first here.
    let r = c
        .rest("GET", &format!("/page/{talk}/history?dir=newer"), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let rows = r.body["rows"].as_array().unwrap();
    let ops: Vec<&str> = rows
        .iter()
        .map(|row| row["summary"]["op"].as_str().unwrap_or("?"))
        .collect();
    assert_eq!(
        ops,
        [
            "create", "post", "post", "post", "edit", "pin", "create", "rename"
        ],
        "{}",
        r.body
    );
    assert_eq!(rows[0]["target"]["id"], thread.to_string());
    assert_eq!(rows[0]["flags"]["new"], true);
    assert_eq!(rows[0]["log"]["type"], "create");
    assert!(rows[0]["logid"].is_u64(), "{}", rows[0]);
    assert!(rows[1].get("logid").is_none() && rows[1].get("log").is_none());
    assert_eq!(rows[5]["log"]["action"], "pin");
    assert_eq!(rows[7]["log"]["type"], "move");
    // A thread's own history is its rows; the subject page's is its own.
    let r = c
        .rest("GET", &format!("/page/{thread}/history"), None)
        .await;
    assert_eq!(r.body["rows"].as_array().unwrap().len(), 7, "{}", r.body);
    let r = c.rest("GET", &format!("/page/{page}/history"), None).await;
    assert_eq!(r.body["rows"].as_array().unwrap().len(), 1, "{}", r.body);
    assert_eq!(
        c.rest("GET", "/page/999999/history", None).await.status,
        StatusCode::NOT_FOUND
    );
    // The log events, in the log; posts are not log events.
    let l = c
        .api("action=query&list=logevents&leprop=type%7Ctitle%7Cids&lelimit=50")
        .await;
    let events: Vec<String> = l["query"]["logevents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            format!(
                "{}/{}",
                e["type"].as_str().unwrap_or("?"),
                e["action"].as_str().unwrap_or("?")
            )
        })
        .collect();
    for want in ["create/create", "thread/pin", "move/move"] {
        assert!(events.iter().any(|e| e == want), "{want} in {events:?}");
    }
    assert!(!events.iter().any(|e| e.starts_with("thread/post")));
    // Recent changes show posts as edits to the thread.
    let rc = c
        .api("action=query&list=recentchanges&rcprop=title%7Cids&rclimit=50")
        .await;
    assert!(
        rc["query"]["recentchanges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["revid"] == reply && e["ns"] == 214),
        "{rc}"
    );

    // `prop=revisions` lists a thread's revisions, and each one's content is the thread as
    // it stood after it (0041 §5): the `GET /thread` body.
    let title = c.rest("GET", &path, None).await.body["title"]
        .as_str()
        .unwrap()
        .to_string();
    let r = c
        .api(&format!(
            "action=query&prop=revisions&titles={}&rvprop=ids%7Ccontent&rvslots=main&rvlimit=50&rvdir=newer",
            enc(&format!("Thread:{title}"))
        ))
        .await;
    let revs = r["query"]["pages"][0]["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 7, "{r}");
    let as_of = |k: usize| -> Value {
        let slot = &revs[k]["slots"]["main"];
        assert_eq!(slot["contentmodel"], "triplespace-thread", "{slot}");
        serde_json::from_str(slot["content"].as_str().unwrap()).unwrap()
    };
    assert_eq!(as_of(0)["posts"].as_array().unwrap().len(), 1);
    assert_eq!(as_of(0)["status"], "open");
    assert_eq!(as_of(2)["status"], "resolved");
    assert!(as_of(4)["home"]["pinned"].is_null());
    assert!(as_of(5)["home"]["pinned"].is_string());
    assert_eq!(as_of(6)["subject"], "P31 is fine");
    let cmp = c
        .api(&format!("action=compare&fromrev={opening}&torev={reply}"))
        .await;
    assert!(cmp["compare"]["body"].is_string(), "{cmp}");
    let r = c
        .send(
            "GET",
            &format!("/w/api.php?action=parse&pageid={thread}&format=json&formatversion=2"),
            None,
            None,
            &[],
        )
        .await;
    assert_eq!(r.body["error"]["code"], "ts-not-supported", "{}", r.body);

    // A post's text can be revision-deleted now that it has an activity row.
    let off_topic = c.rest("GET", &path, None).await.body["posts"][3]["id"]
        .as_u64()
        .unwrap();
    let h = c
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={off_topic}&hide=content&reason=Spam&token={}",
            enc(&token)
        ))
        .await;
    assert_eq!(h["revisiondelete"]["items"][0]["texthidden"], true, "{h}");
    let mut viewer = Client {
        router: c.router.clone(),
        cookie: None,
    };
    let r = viewer.rest("GET", &path, None).await;
    let hidden = r.body["posts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == off_topic)
        .unwrap();
    assert_eq!(hidden["texthidden"], true, "{hidden}");
    assert!(hidden["content"].is_null(), "{hidden}");

    // Another actor's post, written through the one writer as Bob: Alice holds
    // `ts-editpost` (as `owner`), so she may edit it, and the post then names her.
    let bob = {
        let store = writer_app.store();
        let mut cx = scatter_projection::Backend::begin(store).await.unwrap();
        let w = triplespace_api_ingest::threads::write(
            store,
            writer_app.pipeline(),
            &mut cx,
            &scatter_ingest::Request::new(
                TENANT,
                scatter_ingest::Attestation::by("lbwiki:8"),
                triplespace_api_action::edit::now_micros(),
            ),
            triplespace_api_ingest::threads::ThreadWrite {
                thread: Some(thread),
                op: scatter_threads::ThreadOp::Post {
                    in_reply_to: Some(opening),
                    status: None,
                    media_type: None,
                    heading: None,
                    collapse: None,
                    uncollapse: None,
                    imported_from: None,
                },
                text: Some("Bob here.".into()),
                writer: scatter_threads::Writer::Editor,
                limits: scatter_threads::Limits::default(),
                date: None,
            },
        )
        .await
        .unwrap();
        scatter_projection::Backend::commit(store, cx)
            .await
            .unwrap();
        w.revid
    };
    let read = c.rest("GET", &path, None).await;
    let alice = read.body["posts"][0]["attributedTo"].clone();
    let base = read.body["base"].as_u64().unwrap();
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{bob}"),
            Some(json!({"text": "Bob here, typo fixed.", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let read = c.rest("GET", &path, None).await;
    let post = read.body["posts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == bob)
        .unwrap()
        .clone();
    assert_eq!(post["attributedTo"], "lbwiki:8", "{post}");
    assert_eq!(post["content"], "Bob here, typo fixed.");
    assert_eq!(post["editedBy"], json!([alice]), "{post}");
    // Her own edits name nobody.
    assert!(read.body["posts"][0].get("editedBy").is_none());

    // ACLs, as the moderation modules will write them (0023 §3–§5); written to view.acl
    // directly here, since those modules do not exist yet.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let acl =
        |target: String, group: &'static str, kind: &'static str, extra: Option<Value>| {
            let db = &db;
            async move {
                db.execute(
                "INSERT INTO view.acl (tenant, target, restrictions, read_kind, extra, \"offset\")
                 VALUES ($1, $2, $3, $4, $5, 0)
                 ON CONFLICT (tenant, target) DO UPDATE SET restrictions = EXCLUDED.restrictions,
                   read_kind = EXCLUDED.read_kind, extra = EXCLUDED.extra",
                &[&TENANT, &target, &json!({"read": {"group": group}}), &kind, &extra],
            )
            .await
            .unwrap();
            }
        };
    let pages: i64 = db
        .query_one(
            "SELECT partition FROM log.partition WHERE tenant = $1 AND name = 'pages'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    // Partition IDs are stored as `bigint`, bit for bit.
    let pages = pages.cast_unsigned();
    let offset_of = |revid: u64| {
        let db = &db;
        async move {
            let r = i64::try_from(revid).unwrap();
            db.query_one(
                "SELECT latest_offset FROM view.post WHERE tenant = $1 AND revid = $2",
                &[&TENANT, &r],
            )
            .await
            .unwrap()
            .get::<_, i64>(0)
        }
    };
    let mut anon = Client {
        router: c.router.clone(),
        cookie: None,
    };

    // A reply's text hidden from everyone but `owner`, which Alice is in; the answer's
    // author hidden by the suppression group, which nobody is in and `owner` does not
    // read as (0016 A37).
    let reply_offset = offset_of(reply).await;
    acl(
        format!("acl:record:{pages}:{reply_offset}"),
        "owner",
        "moderation",
        Some(json!({"parts": ["text"]})),
    )
    .await;
    let answer_offset = offset_of(answer).await;
    acl(
        format!("acl:record:{pages}:{answer_offset}"),
        "suppress",
        "moderation",
        Some(json!({"parts": ["attestation"]})),
    )
    .await;
    let note = |body: &Value, id: u64| {
        body["posts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .cloned()
            .unwrap()
    };
    let seen = anon.rest("GET", &path, None).await.body;
    assert_eq!(note(&seen, reply)["texthidden"], true, "{seen}");
    assert_eq!(note(&seen, reply)["content"], Value::Null);
    assert_eq!(note(&seen, answer)["userhidden"], true);
    assert_eq!(note(&seen, answer)["attributedTo"], Value::Null);
    assert_eq!(note(&seen, answer)["content"], "Yes.");
    let seen = c.rest("GET", &path, None).await.body;
    assert_eq!(note(&seen, reply)["texthidden"], true);
    assert_eq!(
        note(&seen, reply)["content"],
        "Really?",
        "the group sees the text"
    );
    assert_eq!(note(&seen, answer)["attributedTo"], Value::Null);

    // A deleted thread: 410 outside its deletion group, readable and marked for it, off
    // the listing for everyone else, and closed to every write.
    acl(format!("acl:page:{thread}"), "owner", "moderation", None).await;
    let r = anon.rest("GET", &path, None).await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.body);
    assert_eq!(r.body["code"], "ts-thread-deleted");
    let r = c.rest("GET", &path, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["deleted"], true);
    let listed = anon
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await
        .body;
    assert_eq!(listed["threads"].as_array().unwrap().len(), 1, "{listed}");
    let listed = c
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await
        .body;
    assert_eq!(listed["threads"][0]["deleted"], true, "{listed}");
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"text": "Still here?", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.body);
    assert_eq!(r.body["code"], "ts-thread-deleted");

    // A confidential thread is absent for those outside its group; `owner` reads it by
    // definition (0016 A37).
    let other = listed["threads"][1]["id"].as_u64().unwrap();
    acl(format!("acl:page:{other}"), "board", "confidential", None).await;
    let r = anon.rest("GET", &format!("/thread/{other}"), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(r.body["code"], "ts-thread-not-found");
    let listed = anon
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await
        .body;
    assert_eq!(listed["threads"].as_array().unwrap().len(), 0, "{listed}");
    let r = c.rest("GET", &format!("/thread/{other}"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    // A deleted page takes no new threads, and its talk page answers 410 outside the
    // group that deleted it.
    acl(
        format!("acl:page:{page}"),
        "suppress",
        "moderation",
        Some(json!({"talk": true})),
    )
    .await;
    let mut fresh = start.clone();
    fresh["subject"] = json!("A third question");
    fresh["token"] = json!(token);
    let r = c.rest("POST", "/thread", Some(fresh)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(r.body["code"], "ts-no-target");
    let r = c.rest("GET", &format!("/page/{talk}/threads"), None).await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.body);
}
