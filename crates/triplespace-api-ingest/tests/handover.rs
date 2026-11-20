//! Whether a follower can take over from a load (0092 §5; 05 §5.9), against a stub wiki
//! that answers `siteinfo` and the oldest recent change.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use triplespace_api_ingest::follow::handover_gap;
use triplespace_upstream::Wiki;

const DAY: u64 = 86_400;
/// 2026-11-20T00:00:00Z.
const NOW: u64 = 1_795_132_800;

/// A wiki whose oldest recent change is `oldest`, and a count of the requests it answered.
async fn stub(oldest: &'static str) -> (Wiki, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(AtomicUsize::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let body = if request.contains("siteinfo") {
                r#"{"query":{"namespaces":{"0":{"id":0,"defaultcontentmodel":"wikibase-item"},"120":{"id":120,"canonical":"Property","defaultcontentmodel":"wikibase-property"}}}}"#.to_string()
            } else {
                format!(r#"{{"query":{{"recentchanges":[{{"timestamp":"{oldest}"}}]}}}}"#)
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes()).await;
        }
    });
    let wiki = Wiki::new(&format!("http://127.0.0.1:{port}/w/api.php"), "test").unwrap();
    (wiki, asked)
}

/// A load that ends before the oldest change the wiki keeps, beyond `follow.retention`,
/// cannot hand over; one the wiki still covers can.
#[tokio::test]
async fn a_load_older_than_the_wikis_recent_changes_cannot_hand_over() {
    // The wiki keeps changes from 2026-08-22, 90 days back.
    let (wiki, _) = stub("2026-08-22T00:00:00Z").await;
    let oldest = NOW - 90 * DAY;
    let tail = 900;
    assert_eq!(
        handover_gap(&wiki, NOW - 120 * DAY, tail, 30 * DAY, NOW)
            .await
            .unwrap(),
        Some(oldest),
        "a load 120 days old"
    );
    assert_eq!(
        handover_gap(&wiki, NOW - 60 * DAY, tail, 30 * DAY, NOW)
            .await
            .unwrap(),
        None,
        "a load the wiki's changes still cover"
    );
    // The follower starts `tail` before the end: a load ending just after the oldest
    // change starts before it.
    assert_eq!(
        handover_gap(&wiki, oldest + 60, tail, 30 * DAY, NOW)
            .await
            .unwrap(),
        Some(oldest),
        "the follower starts a tail before the load's end"
    );
}

/// Within `follow.retention` a load can always hand over, and the wiki is not asked.
#[tokio::test]
async fn a_load_within_the_retention_asks_nothing() {
    let (wiki, asked) = stub("2026-11-19T00:00:00Z").await;
    assert_eq!(
        handover_gap(&wiki, NOW - 2 * DAY, 900, 30 * DAY, NOW)
            .await
            .unwrap(),
        None
    );
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}
