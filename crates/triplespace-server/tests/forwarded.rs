//! Proxy trust (0056 §10 line 5; 0057 §10): the client behind trusted proxies, by address
//! and by forwarder key, against a real database and over a real TCP listener. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_postgres::{Config, NoTls};
use triplespace_accounts::{Secret, forwarder};
use triplespace_api_action::forwarded::origin;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router};
use triplespace_projections::Farm;

const DB: &str = "tsforwarded_test";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn admin() -> tokio_postgres::Client {
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn fresh_database() -> String {
    let client = admin().await;
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

fn app(database: &str, trusted: &[&str]) -> Result<App, String> {
    App::new(
        triplespace_db::pool::pool(database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: trusted.iter().map(|s| (*s).to_string()).collect(),
            dev_tenant: None,
            farm: Farm {
                slug: "farm".into(),
                base: "https://farm.example".into(),
            },
            generator: "Triplespace test".into(),
        },
    )
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.append(
            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    h
}

/// Sends a raw HTTP/1.1 request and returns the whole response as text.
async fn raw(addr: SocketAddr, request: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(request.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn proxy_trust_by_address_and_by_key() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let conn = triplespace_cli::common::connect(&database).await.unwrap();
    let mut conn = conn;
    triplespace_db::migrator().apply(&mut conn).await.unwrap();

    // A typo in the trust list fails at start.
    assert!(app(&database, &["10.0.0.0/33"]).is_err());
    assert!(app(&database, &["edge.internal"]).is_err());

    // Two keys: the edge's and the web tier's.
    let edge = forwarder::issue(&conn, "edge").await.unwrap();
    let web = forwarder::issue(&conn, "web").await.unwrap();
    assert!(
        forwarder::issue(&conn, "web").await.is_err(),
        "live labels are unique"
    );

    // By address: the edge at 10.0.0.2 is trusted; the client forged an entry.
    let a = app(&database, &["10.0.0.2"]).unwrap();
    let o = origin(
        &a,
        Some(ip("10.0.0.2")),
        &headers(&[
            ("x-forwarded-for", "6.6.6.6, 198.51.100.7"),
            ("x-forwarded-host", "librarybase.org"),
            ("x-forwarded-proto", "HTTPS"),
        ]),
    )
    .await;
    assert_eq!(o.client, Some(ip("198.51.100.7")));
    assert!(o.peer_trusted);
    assert_eq!(o.forwarded_host.as_deref(), Some("librarybase.org"));
    assert_eq!(o.forwarded_proto.as_deref(), Some("https"));

    // An untrusted peer: nothing it forwards is believed.
    let o = origin(
        &a,
        Some(ip("10.244.9.9")),
        &headers(&[
            ("x-forwarded-for", "198.51.100.7"),
            ("x-forwarded-host", "librarybase.org"),
        ]),
    )
    .await;
    assert_eq!(o.client, Some(ip("10.244.9.9")));
    assert!(!o.peer_trusted);
    assert_eq!(o.forwarded_host, None);

    // By key: client → edge (10.244.1.5) → web (10.244.2.8) → server, nothing trusted by
    // address. The 0057 §10 example.
    let k = app(&database, &[]).unwrap();
    let both = format!("{}, {}", edge.key, web.key);
    let o = origin(
        &k,
        Some(ip("10.244.2.8")),
        &headers(&[
            ("x-forwarded-for", "6.6.6.6, 198.51.100.7, 10.244.1.5"),
            ("triplespace-forwarder", &both),
        ]),
    )
    .await;
    assert_eq!(o.client, Some(ip("198.51.100.7")));
    assert_eq!(o.forwarders, vec![web.key_id.clone(), edge.key_id.clone()]);

    // A rogue pod sends a made-up key to the web tier; the web tier appends the pod's
    // address and its own key. Belief stops at the pod.
    let forged = format!("{}.not-the-secret, {}", edge.key_id, web.key);
    let o = origin(
        &k,
        Some(ip("10.244.2.8")),
        &headers(&[
            ("x-forwarded-for", "192.0.2.1, 10.244.9.9"),
            ("triplespace-forwarder", &forged),
        ]),
    )
    .await;
    assert_eq!(o.client, Some(ip("10.244.9.9")));
    assert_eq!(o.forwarders, vec![web.key_id.clone()]);

    // A revoked key is refused (a fresh state, since checks are remembered for 30 s).
    assert!(forwarder::revoke(&conn, &web.key_id).await.unwrap());
    assert!(!forwarder::revoke(&conn, &web.key_id).await.unwrap());
    let fresh = app(&database, &[]).unwrap();
    let o = origin(
        &fresh,
        Some(ip("10.244.2.8")),
        &headers(&[
            ("x-forwarded-for", "198.51.100.7"),
            ("triplespace-forwarder", &web.key),
        ]),
    )
    .await;
    assert_eq!(o.client, Some(ip("10.244.2.8")));
    assert!(o.forwarders.is_empty());
    let listed = forwarder::list(&conn).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .iter()
            .any(|k| k.key_id == web.key_id && k.revoked_at.is_some())
    );

    over_a_listener(&database).await;

    drop(conn);
    let _ = admin()
        .await
        .batch_execute(&format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)"))
        .await;
}

/// Over a real listener the peer comes from `ConnectInfo`.
async fn over_a_listener(database: &str) {
    // With nothing registered,
    // every host is 421, and the 421 names the host the server believed: the forwarded
    // one only when the peer (127.0.0.1) is trusted.
    for (trusted, expect) in [(vec!["127.0.0.1"], "b.example"), (vec![], "a.example")] {
        let a = app(database, &trusted).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router(a).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let resp = raw(
            addr,
            "GET /w/api.php?action=query&meta=siteinfo&format=json HTTP/1.1\r\nHost: a.example\r\nX-Forwarded-Host: b.example\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(resp.starts_with("HTTP/1.1 421"), "{resp}");
        assert!(
            resp.contains(&format!("{expect} is not a tenant")),
            "trusted {trusted:?}: {resp}"
        );
        server.abort();
    }
}
