//! Entity sources end to end (0078 §1–§4, §10): a tenant declares a source, loads it from a
//! Wikibase dump through `sync --tenant --source`, and reads its entities by their
//! colon-form IDs through the Action API and the REST API, on that tenant only. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::{instance, source, sync};

const DB: &str = "tssources_test";
const TENANT: &str = "reader";
const HOST: &str = "reader.example";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
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

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/librarybase/dump/librarybase-sample.xml.gz")
}

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let config: Config = database.parse().unwrap();
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client.query(sql, &[]).await.unwrap()
}

async fn connect_to(database: &str) -> tokio_postgres::Client {
    let config: Config = database.parse().unwrap();
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

struct Client {
    router: Router,
}

impl Client {
    async fn raw(&self, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A request with a session cookie, answering the reply and the cookie it set.
    async fn send(
        &self,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        form: Option<(&str, String)>,
    ) -> (StatusCode, Value, Option<String>) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let body = match form {
            Some((ct, b)) => {
                req = req.header(header::CONTENT_TYPE, ct);
                Body::from(b)
            }
            None => Body::empty(),
        };
        let mut req = req.body(body).unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [203, 0, 113, 9],
                50000,
            ))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let set = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("triplespace_session="))
            .and_then(|v| v.split(';').next())
            .map(str::to_string);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            set,
        )
    }

    /// The owner's session cookie and a CSRF token for it.
    async fn owner_session(&self) -> (String, String) {
        let api = |q: &str| format!("/w/api.php?{q}&format=json&formatversion=2");
        let (_, v, cookie) = self
            .send(
                "GET",
                &api("action=query&meta=tokens&type=login"),
                None,
                None,
            )
            .await;
        let mut cookie = cookie.expect("a session for the login");
        let token = v["query"]["tokens"]["logintoken"]
            .as_str()
            .unwrap()
            .to_string();
        let form = format!(
            "action=clientlogin&username=Reader&password=owner-secret-1234&logintoken={}\
             &loginreturnurl=https://{HOST}/&format=json&formatversion=2",
            token.replace('+', "%2B").replace('\\', "%5C")
        );
        let (_, v, set) = self
            .send(
                "POST",
                "/w/api.php",
                Some(&cookie),
                Some(("application/x-www-form-urlencoded", form)),
            )
            .await;
        assert_eq!(v["clientlogin"]["status"], "PASS", "{v}");
        if let Some(c) = set {
            cookie = c;
        }
        let (_, v, _) = self
            .send("GET", &api("action=query&meta=tokens"), Some(&cookie), None)
            .await;
        let csrf = v["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
        (cookie, csrf)
    }

    async fn get(&self, q: &str) -> Value {
        let (status, v) = self
            .raw(&format!("/w/api.php?{q}&format=json&formatversion=2"))
            .await;
        assert_eq!(status, StatusCode::OK, "{q}: {v}");
        v
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_source_is_declared_loaded_and_read_on_its_tenant() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssources-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    let key_file = dir.join("triplespace.key");
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(1),
        owner_name: "Reader".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");

    // 1. The declaration: numbered, with its partition (0078 §1, §6.4).
    let at = || source::At {
        database: database.clone(),
        tenant: TENANT.into(),
    };
    source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: at(),
            name: "lb".into(),
            reason: "the test".into(),
        },
        label: "Librarybase as a source".into(),
        types: vec!["Q=item".into(), "P=property".into()],
        iri: "https://lb.example/entity/{upstream_id}".into(),
        api: Some("https://librarybase.org/w/api.php".into()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        namespace: None,
        namespace_aliases: vec![],
    }))
    .await
    .expect("source add");
    let declared = query(
        &database,
        "SELECT config->>'number' FROM view.registry WHERE tenant = 'reader' AND kind = 'entity-source' AND code = 'lb'",
    )
    .await;
    assert_eq!(declared[0].get::<_, String>(0), "4096");
    assert_eq!(
        query(
            &database,
            "SELECT count(*)::int FROM log.partition WHERE tenant = 'reader' AND name = 'source/lb'"
        )
        .await[0]
            .get::<_, i32>(0),
        1
    );
    // Declared again, the source is updated: the record changes, the number and the
    // partition stay.
    source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: at(),
            name: "lb".into(),
            reason: "relabelled".into(),
        },
        label: "Librarybase".into(),
        types: vec!["Q=item".into(), "P=property".into()],
        iri: "https://lb.example/entity/{upstream_id}".into(),
        api: Some("https://librarybase.org/w/api.php".into()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        namespace: None,
        namespace_aliases: vec![],
    }))
    .await
    .expect("source add again");
    let redeclared = query(
        &database,
        "SELECT config->>'number', config->>'label' FROM view.registry WHERE tenant = 'reader' AND kind = 'entity-source' AND code = 'lb'",
    )
    .await;
    assert_eq!(
        (
            redeclared[0].get::<_, String>(0),
            redeclared[0].get::<_, String>(1)
        ),
        ("4096".to_string(), "Librarybase".to_string())
    );
    assert_eq!(
        query(
            &database,
            "SELECT count(*)::int FROM log.partition WHERE tenant = 'reader' AND name = 'source/lb'"
        )
        .await[0]
            .get::<_, i32>(0),
        1
    );
    // A registry name is refused (0078 §3 rule 1).
    let taken = source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: at(),
            name: "librarybase".into(),
            reason: String::new(),
        },
        label: "x".into(),
        types: vec!["Q=item".into()],
        iri: "https://x.example/{upstream_id}".into(),
        api: None,
        entity_data: None,
        revision_ids: false,
        licence: None,
        namespace: None,
        namespace_aliases: vec![],
    }))
    .await;
    assert!(
        taken
            .as_ref()
            .is_err_and(|e| e.to_string().contains("ts-source-name-taken")),
        "{taken:?}"
    );

    // A source with a registry provider's templates is refused: the tenant adds the
    // provider instead (05 §6.8).
    let duplicate = source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: at(),
            name: "lbcopy".into(),
            reason: String::new(),
        },
        label: "x".into(),
        types: vec!["Q=item".into()],
        iri: "https://librarybase.org/entity/{upstream_id}".into(),
        api: None,
        entity_data: None,
        revision_ids: false,
        licence: None,
        namespace: None,
        namespace_aliases: vec![],
    }))
    .await;
    assert!(
        duplicate
            .as_ref()
            .is_err_and(|e| e.to_string().contains("ts-source-is-provider")),
        "{duplicate:?}"
    );

    // 2. The load from the dump (0092 §4), recording where it ended (0092 §5).
    sync::run(sync::Sync {
        job_id: None,
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        dump: Some(fixture()),
        version: Some("sample".into()),
        snapshot: false,
        threshold: 100,
        batch: 50,
        rejects: None,
        no_bootstrap: false,
        writers: sync::WRITERS,
        no_gap_check: true,
    })
    .await
    .expect("sync into the source");
    // The source's own snak and reference hashes are recomputed, IDs in the form the source
    // wrote them (`lb:P1` as `P1`), so none is kept as a mismatch.
    let mismatches = query(
        &database,
        "SELECT counts->'hash_mismatches' FROM view.job WHERE tenant = 'reader' AND graph = 'source/lb'",
    )
    .await;
    assert_eq!(mismatches.len(), 1);
    assert_eq!(
        mismatches[0].get::<_, Option<Value>>(0),
        None,
        "every hash matches the source's own"
    );
    let loaded = query(
        &database,
        "SELECT entity_id FROM view.entity_source WHERE tenant = 'reader' AND graph = 'source/lb' ORDER BY entity_id",
    )
    .await;
    assert_eq!(
        loaded.len(),
        254,
        "every entity of the sample, as tenant rows"
    );
    assert!(
        loaded
            .iter()
            .all(|r| r.get::<_, String>(0).starts_with("lb:"))
    );
    assert_eq!(
        query(
            &database,
            "SELECT count(*)::int FROM view.entity_source WHERE tenant = '' AND graph = 'source/lb'"
        )
        .await[0]
            .get::<_, i32>(0),
        0,
        "no shared row (0078 §4)"
    );
    let cursor = query(
        &database,
        "SELECT kind, name FROM ops.repo_cursor WHERE tenant = 'reader' AND loaded_to IS NOT NULL",
    )
    .await;
    assert_eq!(
        (cursor[0].get::<_, String>(0), cursor[0].get::<_, String>(1)),
        ("source".to_string(), "lb".to_string())
    );

    // 3. Reads on the tenant.
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(&key_file).unwrap();
    let app = App::new(
        triplespace_db::scratch::pools(&database, 4),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
    };
    let v = c
        .get("action=wbgetentities&ids=LB:q1&props=info|labels|claims")
        .await;
    let e = &v["entities"]["lb:Q1"];
    assert_eq!(
        e["id"], "lb:Q1",
        "canonicalized from `LB:q1` (0078 §2): {v}"
    );
    assert_eq!(e["type"], "item");
    assert_eq!(e["title"], "Item:lb:Q1");
    assert_eq!(e["ns"], 120);
    assert!(e["labels"]["en"]["value"].is_string());
    // Statements and values are in the source's form too.
    let claims = e["claims"].as_object().unwrap();
    assert!(claims.keys().all(|p| p.starts_with("lb:P")), "{claims:?}");
    let v = c
        .get("action=wbgetentities&ids=lb:Q999999&props=info")
        .await;
    assert!(v["entities"]["lb:Q999999"]["missing"].is_string(), "{v}");
    let v = c.get("action=wbgetentities&ids=lb:X1").await;
    assert_eq!(v["error"]["code"], "invalid-entity-id", "{v}");
    let v = c.get("action=wbgetentities&ids=nosuch:Q1").await;
    assert_eq!(v["error"]["code"], "invalid-entity-id", "{v}");
    let v = c
        .get("action=wbsearchentities&search=Librarybase&language=en&limit=3")
        .await;
    let hit = v["search"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|r| r["id"].as_str().is_some_and(|s| s.starts_with("lb:")))
        })
        .cloned();
    assert!(hit.is_some(), "the source's entities are searchable: {v}");
    let (status, p) = c
        .raw("/w/rest.php/triplespace/v0/entity/lb:Q1/provenance")
        .await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["minted_by"]["kind"], "source", "{p}");
    assert_eq!(p["minted_by"]["upstream_id"], "Q1");
    assert_eq!(p["dominant"], "source/lb");
    let (status, _) = c
        .raw("/w/rest.php/triplespace/v0/entity/lb:Q999999/provenance")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // `registry check` reads the tenant's sources (0078 §9): `lb` names no registry
    // provider's graph, so there is nothing to report; the check changes nothing.
    triplespace_cli::registry::run(triplespace_cli::registry::RegistryCommand::Check(
        triplespace_cli::registry::Check {
            database: database.clone(),
            tenant: Some("reader".into()),
        },
    ))
    .await
    .expect("registry check");
    let declared =
        triplespace_projections::entity_sources::declared(&connect_to(&database).await, "reader")
            .await
            .unwrap();
    assert!(
        triplespace_api_ingest::sources::findings(
            &declared[0],
            scatter_providers::Registry::default_registry()
        )
        .is_empty()
    );

    // The tenant's sources (0078 §10): `GET /entity-sources`, and the same list in
    // `siprop=triplespace`.
    let (status, listed) = c.raw("/w/rest.php/triplespace/v0/entity-sources").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let sources = listed["sources"].as_array().expect("a list");
    assert_eq!(sources.len(), 1, "{listed}");
    assert_eq!(sources[0]["name"], "lb");
    assert_eq!(sources[0]["label"], "Librarybase");
    assert!(
        sources[0]["number"].as_u64().is_some_and(|n| n >= 4096),
        "{listed}"
    );
    assert_eq!(
        sources[0]["types"][0]["iri"],
        "https://lb.example/entity/{upstream_id}"
    );
    assert_eq!(sources[0]["promoted_to"], Value::Null);
    assert_eq!(sources[0]["retired"], false);
    let site = c.get("action=query&meta=siteinfo&siprop=triplespace").await;
    assert_eq!(
        site["query"]["triplespace"]["entity_sources"], listed["sources"],
        "{site}"
    );

    // The source's mirror status (18 §3.2; 0092 A15): loaded, not yet followed.
    let mirror = "/w/rest.php/triplespace/v0/provider/lb/mirror";
    let (status, body) = c.raw(mirror).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "source");
    assert_eq!(body["followed_by"], "recentchanges");
    assert_eq!(body["status"], "not-followed");
    assert!(body["follow"].is_null() && body["gap"].is_null(), "{body}");
    // The read of `lb:Q999999` above found nothing and queued it (0070 A13).
    assert_eq!(
        body["queued"], 1,
        "the unheld read waits to be fetched: {body}"
    );
    assert_eq!(
        query(
            &database,
            "SELECT entity_id || ' ' || reason FROM ops.entity_fetch WHERE tenant = 'reader'"
        )
        .await
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect::<Vec<_>>(),
        ["Q999999 read"]
    );
    // Per type, the setting the follower applies and the set's size (0070 §9, A11, A12):
    // a loaded source with no setting is followed whole.
    assert_eq!(body["types"]["item"]["setting"], "all", "{body}");
    assert!(
        body["types"]["item"]["size"].as_i64().unwrap() > 0,
        "{body}"
    );
    assert!(body["types"]["property"]["size"].is_i64(), "{body}");

    // Queuing by hand needs `ts-runjob` (0070 §9): the owner has it.
    let fetch = "/w/rest.php/triplespace/v0/provider/lb/mirror/fetch";
    let (status, body, _) = c
        .send(
            "POST",
            fetch,
            None,
            Some(("application/json", r#"{"ids": ["Q5"]}"#.into())),
        )
        .await;
    assert!(status.is_client_error(), "anonymous: {status} {body}");
    let (cookie, csrf) = c.owner_session().await;
    let post = |ids: Value| {
        (
            "application/json",
            serde_json::json!({"token": csrf, "ids": ids}).to_string(),
        )
    };
    let (status, body, _) = c
        .send(
            "POST",
            fetch,
            Some(&cookie),
            Some(post(serde_json::json!(["Q5", "lb:Q6", "Q5"]))),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["queued"].as_u64(), body["asked"].as_u64()),
        (Some(2), Some(3))
    );
    let (status, body, _) = c
        .send(
            "POST",
            fetch,
            Some(&cookie),
            Some(post(serde_json::json!(["WDQ5"]))),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "ts-not-this-provider", "{body}");
    let (_, body) = c.raw(mirror).await;
    assert_eq!(body["queued"], 3, "{body}");
    connect_to(&database)
        .await
        .execute("DELETE FROM ops.entity_fetch", &[])
        .await
        .unwrap();
    assert!(body["load"]["loaded_to"].is_string(), "{body}");
    assert_eq!(body["load"]["handed_over"], false);
    // A gap the follower found is reported until a position is saved or a load records a
    // new end.
    let pg = connect_to(&database).await;
    triplespace_api_ingest::follow::record_gap(
        &pg,
        TENANT,
        "source",
        "lb",
        1_700_000_000,
        1_790_000_000,
        1_795_000_000,
    )
    .await
    .unwrap();
    let (_, body) = c.raw(mirror).await;
    assert_eq!(body["status"], "follow-gap", "{body}");
    assert_eq!(body["gap"]["position"], "2023-11-14T22:13:20Z", "{body}");
    triplespace_api_ingest::follow::record_load(&pg, TENANT, "source", "lb", 1_900_000_000)
        .await
        .unwrap();
    let (_, body) = c.raw(mirror).await;
    assert_eq!(
        body["status"], "not-followed",
        "a new load clears the gap: {body}"
    );
    // A registry provider by its code, followed by the instance; anything else is not
    // found.
    let (status, body) = c.raw("/w/rest.php/triplespace/v0/provider/WD/mirror").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["kind"].as_str(), body["slug"].as_str()),
        (Some("provider"), Some("wikidata"))
    );
    assert_eq!(body["status"], "not-followed");
    for nosuch in ["ZZ", "nosuch"] {
        let (status, body) = c
            .raw(&format!(
                "/w/rest.php/triplespace/v0/provider/{nosuch}/mirror"
            ))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{nosuch}: {body}");
        assert_eq!(body["code"], "provider-not-found", "{body}");
    }

    clusters(&c).await;

    // 4. What the first record fixed stays fixed (05 §6.4).
    let add = |name: &str, iri: &str| source::Add {
        which: source::Which {
            at: at(),
            name: name.into(),
            reason: String::new(),
        },
        label: "Librarybase".into(),
        types: vec!["Q=item".into(), "P=property".into()],
        iri: iri.into(),
        api: Some("https://librarybase.org/w/api.php".into()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        namespace: None,
        namespace_aliases: vec![],
    };
    let moved = source::run(source::SourceCommand::Add(add(
        "lb",
        "https://elsewhere.example/entity/{upstream_id}",
    )))
    .await;
    assert!(
        moved
            .as_ref()
            .is_err_and(|e| e.to_string().contains("ts-source-binding")),
        "{moved:?}"
    );

    // 5. Retired, the record stays: its IDs still resolve, it loads nothing, and its number
    //    is not given to another source.
    source::run(source::SourceCommand::Retire(source::Which {
        at: at(),
        name: "lb".into(),
        reason: "the test".into(),
    }))
    .await
    .expect("retire");
    let retired = query(
        &database,
        "SELECT config->>'number', config->>'retired' FROM view.registry WHERE tenant = 'reader' AND kind = 'entity-source' AND code = 'lb'",
    )
    .await;
    assert_eq!(
        (
            retired[0].get::<_, String>(0),
            retired[0].get::<_, String>(1)
        ),
        ("4096".to_string(), "true".to_string())
    );
    let v = c.get("action=wbgetentities&ids=lb:Q1&props=info").await;
    assert_eq!(v["entities"]["lb:Q1"]["id"], "lb:Q1", "{v}");
    let again = source::run(source::SourceCommand::Retire(source::Which {
        at: at(),
        name: "lb".into(),
        reason: String::new(),
    }))
    .await;
    assert!(again.is_err(), "retired twice");
    let load = sync::run(sync::Sync {
        job_id: None,
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        dump: Some(fixture()),
        version: Some("sample".into()),
        snapshot: false,
        threshold: 100,
        batch: 50,
        rejects: None,
        no_bootstrap: false,
        writers: sync::WRITERS,
        no_gap_check: true,
    })
    .await;
    assert!(
        load.as_ref()
            .is_err_and(|e| e.to_string().contains("retired")),
        "{load:?}"
    );
    source::run(source::SourceCommand::Add(add(
        "other",
        "https://other.example/entity/{upstream_id}",
    )))
    .await
    .expect("another source");
    let number = |name: &'static str| {
        let database = database.clone();
        async move {
            query(
                &database,
                &format!("SELECT config->>'number' FROM view.registry WHERE tenant = 'reader' AND kind = 'entity-source' AND code = '{name}'"),
            )
            .await[0]
                .get::<_, String>(0)
        }
    };
    assert_eq!(number("other").await, "4097", "4096 stays lb's");

    // 6. Declared again, the retired source is current under its own number.
    source::run(source::SourceCommand::Add(add(
        "lb",
        "https://lb.example/entity/{upstream_id}",
    )))
    .await
    .expect("lb again");
    assert_eq!(number("lb").await, "4096");
    assert!(
        query(
            &database,
            "SELECT config->'retired' FROM view.registry WHERE tenant = 'reader' AND kind = 'entity-source' AND code = 'lb'",
        )
        .await[0]
            .get::<_, Option<serde_json::Value>>(0)
            .is_none()
    );
}

/// Percent-encodes a form value.
fn enc(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A local item minted as the same as a source's entity (04 §4.4, §4.9, §4.12; 0004
/// A25–A27): each reads as the cluster's fused body under its own ID, the source's with
/// `canonical` naming the local item, and a search finds the cluster once.
async fn clusters(c: &Client) {
    let source = c
        .get("action=wbgetentities&ids=lb:Q1&props=labels|claims")
        .await;
    let label = source["entities"]["lb:Q1"]["labels"]["en"]["value"]
        .as_str()
        .expect("lb:Q1 has an English label")
        .to_string();
    let (cookie, csrf) = c.owner_session().await;
    let data = serde_json::json!({"labels": {"en": {"language": "en", "value": "Clustered"}}});
    let (_, made, _) = c
        .send(
            "POST",
            "/w/api.php",
            Some(&cookie),
            Some((
                "application/x-www-form-urlencoded",
                format!(
                    "action=wbeditentity&new=item&data={}&ts-sameas=lb:Q1&token={}&format=json&formatversion=2",
                    enc(&data.to_string()),
                    enc(&csrf)
                ),
            )),
        )
        .await;
    let local = made["entity"]["id"].as_str().expect("minted").to_string();

    // The source's entity: the local label first, `canonical` naming the local item.
    let v = c.get("action=wbgetentities&ids=lb:Q1").await;
    let e = &v["entities"]["lb:Q1"];
    assert_eq!(e["canonical"], local.as_str(), "{e}");
    assert_eq!(e["labels"]["en"]["value"], "Clustered");
    // The local item: the source's statements, under its own ID.
    let v = c.get(&format!("action=wbgetentities&ids={local}")).await;
    let e = &v["entities"][local.as_str()];
    assert!(e.get("canonical").is_none(), "{e}");
    let guids: Vec<&str> = e["claims"]
        .as_object()
        .expect("the source's claims")
        .values()
        .flat_map(|g| g.as_array().into_iter().flatten())
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert!(!guids.is_empty(), "{e}");
    assert!(
        guids.iter().all(|g| g.starts_with(&format!("{local}$"))),
        "{guids:?}"
    );
    // A search finds the cluster once, as its canonical member.
    let v = c
        .get(&format!(
            "action=wbsearchentities&search={}&language=en&limit=50",
            enc(&label)
        ))
        .await;
    let ids: Vec<&str> = v["search"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["id"].as_str())
        .filter(|id| *id == "lb:Q1" || *id == local)
        .collect();
    assert_eq!(ids, [local.as_str()], "{v}");
    // The page bundle the site reads says so too.
    let (status, bundle) = c
        .raw("/w/rest.php/triplespace/v0/entity/lb:Q1/page?prefer=local")
        .await;
    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(bundle["entity"]["canonical"], local.as_str());
}
