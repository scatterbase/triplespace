//! Entity search without a search index (0014 §8; 03 §11.6): how `wbsearchentities`
//! orders what it finds, and that it lists one entity per identity cluster. The rows of
//! `view` are written directly, so that the cases are exactly these. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;

const DB: &str = "tssearch_test";
const TENANT: &str = "searchwiki";
const HOST: &str = "search.example";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database(name: &str) -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(name);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap()
}

/// The entities searched: `(tenant, id, cluster, canonical, deleted)`.
const ENTITIES: &[(&str, &str, Option<i64>, &str, bool)] = &[
    (TENANT, "Q9001", None, "Q9001", false),
    (TENANT, "Q9002", None, "Q9002", false),
    (TENANT, "Q9003", None, "Q9003", false),
    // A local item and the mirrored one it is clustered with; the local one is canonical.
    (TENANT, "Q9005", Some(5), "Q9005", false),
    ("", "WDQ9", Some(5), "Q9005", false),
    // A mirrored item in no cluster.
    ("", "WDQ10", None, "WDQ10", false),
    (TENANT, "Q9006", None, "Q9006", true),
    (TENANT, "Q9007", None, "Q9007", false),
];

/// Their terms: `(tenant, id, kind, language, text)`; 1 a label, 3 an alias.
const TERMS: &[(&str, &str, i16, &str, &str)] = &[
    (TENANT, "Q9001", 1, "en", "Zylib"),
    (TENANT, "Q9002", 1, "en", "Zylib of Congress"),
    (TENANT, "Q9003", 1, "en", "Zylibrarian"),
    (TENANT, "Q9005", 1, "en", "LT"),
    ("", "WDQ9", 1, "en", "Zylibrarything"),
    ("", "WDQ10", 3, "en", "Zylibris"),
    ("", "WDQ10", 1, "en", "Ex libris"),
    (TENANT, "Q9006", 1, "en", "Zylib deleted"),
    (TENANT, "Q9007", 1, "de", "Zylibrarium"),
];

/// How many entities refer to each: `(tenant, target, count)`, as the worker folds
/// them into `view.entity_ref_count` (03 §4.2). The mirrored member of a cluster has
/// references of its own, which count for the cluster.
const REFERENCES: &[(&str, &str, i64)] = &[
    (TENANT, "Q9002", 3),
    (TENANT, "Q9003", 1),
    ("", "WDQ9", 1),
    (TENANT, "WDQ9", 1),
];

async fn seed(database: &str) {
    let (client, connection) = tokio_postgres::connect(database, NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    for (page_id, (tenant, id, cluster, canonical, deleted)) in (9000_i64..).zip(ENTITIES) {
        client
            .execute(
                "INSERT INTO view.entity (tenant, id, type, page_id, cluster_id, canonical_id,
                   first_seen, resolved_version, resolved_kind, resolved_size, deleted)
                 VALUES ($1, $2, 'item', $3, $4, $5, now(), 1, 'source', 0, $6)",
                &[tenant, id, &page_id, cluster, canonical, deleted],
            )
            .await
            .unwrap();
    }
    for (tenant, id, kind, lang, text) in TERMS {
        client
            .execute(
                "INSERT INTO view.term (tenant, entity_id, kind, lang, text) VALUES ($1, $2, $3, $4, $5)",
                &[tenant, id, kind, lang, text],
            )
            .await
            .unwrap();
    }
    for (tenant, target, count) in REFERENCES {
        client
            .execute(
                "INSERT INTO view.entity_ref_count (tenant, target_id, refs) VALUES ($1, $2, $3)",
                &[tenant, target, count],
            )
            .await
            .unwrap();
    }
}

async fn app(database: &str, key_file: &std::path::Path) -> App {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(key_file).unwrap();
    App::new(
        triplespace_db::scratch::pools(database, 4),
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
    .unwrap()
}

async fn router(database: &str, key_file: &std::path::Path) -> Router {
    router_with(
        app(database, key_file).await,
        triplespace_api_rest::routes(),
    )
}

/// A fresh instance with one wiki and its owner: the database and the key file.
async fn instance_at(name: &str) -> (String, std::path::PathBuf) {
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
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
        owner_name: "Owner".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    (database, key_file)
}

async fn get(router: &Router, uri: &str) -> Value {
    let req = Request::builder()
        .uri(uri)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// The IDs `wbsearchentities` lists for a text, in order.
async fn found(router: &Router, query: &str) -> (Vec<String>, Value) {
    let v = get(
        router,
        &format!("/w/api.php?action=wbsearchentities&format=json&{query}"),
    )
    .await;
    let ids = v["search"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|h| h["id"].as_str().unwrap().to_string())
        .collect();
    (ids, v)
}

// 0014 §8 (03 §11.6): a prefix of a label or alias in the requested language and its
// fallbacks, ordered by references, collapsed by cluster.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn entities_are_ranked_and_one_per_cluster() {
    if admin_config().is_none() {
        eprintln!("skipping: TRIPLESPACE_TEST_DATABASE_URL is not set");
        return;
    }
    let (database, key_file) = instance_at(DB).await;
    seed(&database).await;
    let api = router(&database, &key_file).await;

    // The most referred to first, then the shorter text; a deleted entity is not found,
    // nor one whose label is in another language. The mirrored item's label finds the
    // local item it is clustered with, once, ranked by the references to either member,
    // the tenant's and the shared ones; an unclustered mirrored item is itself.
    let (ids, v) = found(&api, "search=zyli&language=en&limit=50").await;
    assert_eq!(ids, ["Q9002", "Q9005", "Q9003", "Q9001", "WDQ10"], "{v}");
    let cluster = &v["search"][1];
    assert_eq!(cluster["match"]["text"], "Zylibrarything", "{v}");
    assert_eq!(cluster["label"], "LT", "the result's own label: {v}");
    assert_eq!(cluster["title"], "Item:Q9005", "{v}");
    let alias = &v["search"][4];
    assert_eq!(alias["match"]["type"], "alias", "{v}");
    assert_eq!(alias["aliases"], serde_json::json!(["Zylibris"]), "{v}");
    assert_eq!(alias["label"], "Ex libris", "{v}");

    // An exact match leads whatever refers to the others, however cased.
    let (ids, v) = found(&api, "search=ZYLIB&language=en").await;
    assert_eq!(ids[..2], ["Q9001", "Q9002"], "{v}");

    // Continuation follows the same order.
    let (first, v) = found(&api, "search=zyli&language=en&limit=2").await;
    assert_eq!(first, ["Q9002", "Q9005"], "{v}");
    assert_eq!(v["search-continue"], 2, "{v}");
    let (rest, v) = found(&api, "search=zyli&language=en&limit=2&continue=2").await;
    assert_eq!(rest, ["Q9003", "Q9001"], "{v}");

    // The requested language and its fallbacks: `de-at` reads German, then `mul` and
    // English; `strictlanguage` reads the language alone.
    let (ids, v) = found(&api, "search=zylibrari&language=de-at").await;
    assert_eq!(ids, ["Q9003", "Q9007"], "{v}");
    let (ids, v) = found(&api, "search=zylibrari&language=de&strictlanguage=1").await;
    assert_eq!(ids, ["Q9007"], "{v}");

    // The suggest route lists the same, and a mirrored hit names its provider.
    let s = get(&api, "/w/rest.php/triplespace/v0/suggest?q=zyli&limit=20").await;
    let hits = s["groups"][0]["hits"].as_array().unwrap();
    let ids: Vec<&str> = hits.iter().map(|h| h["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["Q9002", "Q9005", "Q9003", "Q9001", "WDQ10"], "{s}");
    assert_eq!(hits[4]["provider"], "wikidata", "{s}");
    assert!(hits[0].get("provider").is_none(), "{s}");
    // A member of a cluster is a Go to for the cluster's page (19 §2.1).
    let s = get(&api, "/w/rest.php/triplespace/v0/suggest?q=wdq9").await;
    assert_eq!(
        s["goto"],
        serde_json::json!({"kind": "entity", "id": "Q9005", "title": "Item:Q9005"}),
        "{s}"
    );
}

/// A signed-in caller of the Action API.
struct Session {
    router: Router,
    cookie: Option<String>,
}

impl Session {
    async fn call(&mut self, method: &str, query: &str, form: Option<&str>) -> Value {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/w/api.php?{query}&format=json&formatversion=2"))
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match form {
            Some(f) => req
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(f.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.unwrap();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap_or_default()
                .to_string();
            self.cookie = (!value.is_empty()).then_some(value);
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    /// Creates an entity with `wbeditentity` and returns its ID.
    async fn create(&mut self, kind: &str, data: &Value, csrf: &str) -> String {
        let v = self
            .call(
                "POST",
                "action=wbeditentity",
                Some(&format!(
                    "new={kind}&data={}&token={}",
                    enc(&data.to_string()),
                    enc(csrf)
                )),
            )
            .await;
        v["entity"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{v}"))
            .to_string()
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// 03 §4.2, §4.3, §11.6 (0013 A108): the references that order a search are the ones
// composition wrote for real edits, once the worker has counted them. Nothing here
// writes `view` directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn references_written_by_edits_order_the_results() {
    if admin_config().is_none() {
        eprintln!("skipping: TRIPLESPACE_TEST_DATABASE_URL is not set");
        return;
    }
    let (database, key_file) = instance_at("tssearch_refs_test").await;
    let app = app(&database, &key_file).await;
    let mut api = Session {
        router: router_with(app.clone(), triplespace_api_rest::routes()),
        cookie: None,
    };
    let login = api
        .call("GET", "action=query&meta=tokens&type=login", None)
        .await;
    let login = login["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = api
        .call(
            "POST",
            "action=clientlogin",
            Some(&format!(
                "username=Owner&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
                enc(&login)
            )),
        )
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let csrf = api.call("GET", "action=query&meta=tokens", None).await;
    let csrf = csrf["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();

    let label =
        |text: &str| serde_json::json!({"labels": {"en": {"language": "en", "value": text}}});
    let mut property = label("cites");
    property["datatype"] = "wikibase-item".into();
    let p = api.create("property", &property, &csrf).await;
    let short = api.create("item", &label("Quorum"), &csrf).await;
    let long = api.create("item", &label("Quorum sensing"), &csrf).await;
    // Two items that cite the one with the longer label.
    for n in 0..2 {
        let mut citing = label(&format!("Citing work {n}"));
        citing["claims"] = serde_json::json!([{
            "mainsnak": {"snaktype": "value", "property": p,
                "datavalue": {"value": {"entity-type": "item", "id": long}, "type": "wikibase-entityid"}},
            "type": "statement", "rank": "normal"}]);
        api.create("item", &citing, &csrf).await;
    }
    let router = api.router.clone();

    // Before the worker has counted them, the shorter text leads.
    let (ids, v) = found(&router, "search=quorum%20&language=en&type=item").await;
    assert_eq!(
        ids,
        [long.as_str()],
        "only the longer label has the space: {v}"
    );
    let (ids, v) = found(&router, "search=quor&language=en&type=item").await;
    assert_eq!(ids, [short.as_str(), long.as_str()], "{v}");

    app.work_until_idle(1000).await.unwrap();
    let (ids, v) = found(&router, "search=quor&language=en&type=item").await;
    assert_eq!(
        ids,
        [long.as_str(), short.as_str()],
        "the cited item leads: {v}"
    );
    // The property is referred to by the two items that use it.
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    let refs: i64 = client
        .query_one(
            "SELECT refs FROM view.entity_ref_count WHERE tenant = $1 AND target_id = $2",
            &[&TENANT, &p],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(refs, 2);
}
