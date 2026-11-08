//! The `fs` backend over a temporary directory (0039 §3, §4): put and get, ranged and
//! streamed reads, staged writes with the size limit, copy-if-absent idempotence, listing
//! per scope, prefix deletion, and the `full` check (0039 §14).

use std::convert::Infallible;

use bytes::Bytes;
use futures::StreamExt;
use scatter_blob::{BlobError, BlobStore, Key, Kind, Prefix, Scope, Sha256, Verified};

fn lb() -> Scope {
    Scope::tenant("librarybase").unwrap()
}

fn store() -> (tempfile::TempDir, BlobStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = BlobStore::filesystem(dir.path().join("blobs")).unwrap();
    (dir, store)
}

fn chunks(
    bytes: &'static [u8],
    size: usize,
) -> impl futures::Stream<Item = Result<Bytes, Infallible>> {
    futures::stream::iter(bytes.chunks(size).map(|c| Ok(Bytes::from_static(c))))
}

#[tokio::test]
async fn put_then_get_by_hash() {
    // 0039 §3: bytes are addressed by SHA-256 under {scope}/sha256/{h[0..2]}/{h[2..4]}/{h}.
    let (dir, store) = store();
    let stored = store.put(&lb(), Bytes::from_static(b"abc")).await.unwrap();
    assert!(!stored.existed);
    assert_eq!(stored.digest.size, 3);
    assert_eq!(stored.digest.sha256, Sha256::of(b"abc"));
    let h = stored.digest.sha256.hex();
    assert!(
        dir.path()
            .join("blobs")
            .join("librarybase/sha256")
            .join(&h[0..2])
            .join(&h[2..4])
            .join(&h)
            .is_file(),
        "the fs backend lays the key out as a path"
    );

    let key = Key::sha256(lb(), stored.digest.sha256);
    assert_eq!(store.get(&key).await.unwrap().as_ref(), b"abc");
    let meta = store.head(&key).await.unwrap().unwrap();
    assert_eq!(meta.size, 3);
    assert!(store.exists(&key).await.unwrap());

    let missing = Key::sha256(lb(), Sha256::of(b"never stored"));
    assert!(store.head(&missing).await.unwrap().is_none());
    assert!(store.get(&missing).await.unwrap_err().is_not_found());
}

#[tokio::test]
async fn the_same_bytes_are_stored_once() {
    // 0039 §3: identical bytes have one key; a second write skips and is harmless.
    let (_dir, store) = store();
    let first = store
        .put(&lb(), Bytes::from_static(b"twice"))
        .await
        .unwrap();
    let second = store
        .put(&lb(), Bytes::from_static(b"twice"))
        .await
        .unwrap();
    assert!(!first.existed);
    assert!(second.existed);
    assert_eq!(first.digest, second.digest);
    let listed: Vec<_> = store.list(&Prefix::scope(&lb())).collect().await;
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn scopes_keep_bytes_apart() {
    // 0039 §4: under `tenant` separation identical bytes in two tenants are stored twice;
    // the instance scope is a third.
    let (_dir, store) = store();
    let other = Scope::tenant("other").unwrap();
    let a = store
        .put(&lb(), Bytes::from_static(b"shared"))
        .await
        .unwrap();
    let b = store
        .put(&other, Bytes::from_static(b"shared"))
        .await
        .unwrap();
    let c = store
        .put(&Scope::instance(), Bytes::from_static(b"shared"))
        .await
        .unwrap();
    assert!(!a.existed && !b.existed && !c.existed);
    let keys = |scope: &Scope| {
        let prefix = Prefix::scope(scope);
        let store = store.clone();
        async move {
            store
                .list(&prefix)
                .map(|e| e.unwrap().key.unwrap())
                .collect::<Vec<_>>()
                .await
        }
    };
    assert_eq!(keys(&lb()).await, vec![Key::sha256(lb(), a.digest.sha256)]);
    assert_eq!(
        keys(&other).await,
        vec![Key::sha256(other.clone(), b.digest.sha256)]
    );
    assert_eq!(
        keys(&Scope::instance()).await,
        vec![Key::sha256(Scope::instance(), c.digest.sha256)]
    );
}

#[tokio::test]
async fn ranged_and_streamed_reads() {
    // 0039 §3: ranged and streamed reads.
    let (_dir, store) = store();
    let data: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
    let stored = store.put(&lb(), Bytes::from(data.clone())).await.unwrap();
    let key = Key::sha256(lb(), stored.digest.sha256);

    assert_eq!(
        store.get_range(&key, 10..20).await.unwrap().as_ref(),
        &data[10..20]
    );
    assert_eq!(
        store
            .get_range(&key, 99_990..200_000)
            .await
            .unwrap()
            .as_ref(),
        &data[99_990..],
        "a range past the end is clipped"
    );
    assert!(store.get_range(&key, 200_000..200_010).await.is_err());

    let whole = store.read(&key, None).await.unwrap();
    assert_eq!(whole.size, 100_000);
    assert_eq!(whole.range, 0..100_000);
    let mut out = Vec::new();
    let mut s = whole.stream;
    while let Some(chunk) = s.next().await {
        out.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(out, data);

    let part = store.read(&key, Some(256..512)).await.unwrap();
    assert_eq!(part.size, 100_000);
    assert_eq!(part.range, 256..512);
    let got: Vec<u8> = part.stream.map(|c| c.unwrap().to_vec()).concat().await;
    assert_eq!(got, &data[256..512]);
}

#[tokio::test]
async fn a_staged_write_hashes_as_it_streams_then_commits_once() {
    // 0039 §3: an upload streams into stash/ while SHA-256 and SHA-1 are computed, and is
    // copied to its sha256/ key unless an object is already there.
    let (_dir, store) = store();
    let staged = store
        .stash(&lb(), "upload-1", chunks(b"abc", 1), None)
        .await
        .unwrap();
    assert_eq!(staged.key, Key::stash(lb(), "upload-1").unwrap());
    assert_eq!(
        staged.digest.sha256.hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        staged.digest.sha1.hex(),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(staged.digest.size, 3);
    assert_eq!(store.get(&staged.key).await.unwrap().as_ref(), b"abc");
    let address = Key::sha256(lb(), staged.digest.sha256);
    assert!(!store.exists(&address).await.unwrap(), "not yet committed");

    let stored = store.commit(&staged).await.unwrap();
    assert!(!stored.existed);
    assert_eq!(store.get(&address).await.unwrap().as_ref(), b"abc");
    assert!(
        !store.exists(&staged.key).await.unwrap(),
        "the stash object is gone"
    );

    // A twin upload, staged under another key, finds the address taken: copy-if-absent
    // skips, and the twin's stash object is still cleaned up.
    let twin = store
        .stash(&lb(), "upload-2", chunks(b"abc", 2), None)
        .await
        .unwrap();
    let again = store.commit(&twin).await.unwrap();
    assert!(again.existed);
    assert_eq!(again.digest, stored.digest);
    assert!(!store.exists(&twin.key).await.unwrap());
    assert_eq!(store.get(&address).await.unwrap().as_ref(), b"abc");

    // A commit retried after its stash object is gone converges on the stored object.
    let third = store.commit(&twin).await.unwrap();
    assert!(third.existed);
    assert_eq!(third.digest, stored.digest);

    // Only when neither the stash object nor the address exists is a commit an error.
    let orphan = scatter_blob::Staged {
        key: Key::stash(lb(), "vanished").unwrap(),
        digest: scatter_blob::Hasher::new().finish(),
    };
    assert!(store.commit(&orphan).await.unwrap_err().is_not_found());

    let stash: Vec<_> = store
        .list(&Prefix::kind(&lb(), Kind::Stash))
        .collect()
        .await;
    assert!(stash.is_empty());
}

#[tokio::test]
async fn put_stream_stages_and_commits() {
    let (_dir, store) = store();
    let data: &'static [u8] = b"streamed in pieces";
    let stored = store
        .put_stream(&lb(), chunks(data, 5), None)
        .await
        .unwrap();
    assert!(!stored.existed);
    let key = Key::sha256(lb(), stored.digest.sha256);
    assert_eq!(store.get(&key).await.unwrap().as_ref(), data);
    let again = store
        .put_stream(&lb(), chunks(data, 7), None)
        .await
        .unwrap();
    assert!(again.existed);
    let stash: Vec<_> = store
        .list(&Prefix::kind(&lb(), Kind::Stash))
        .collect()
        .await;
    assert!(stash.is_empty(), "no temporary stash object is left behind");
}

#[tokio::test]
async fn the_size_limit_is_enforced_while_streaming() {
    // 0039 §3: the server enforces the size limit as the upload streams in.
    let (_dir, store) = store();
    let data: &'static [u8] = &[7u8; 1000];
    let err = store
        .stash(&lb(), "big", chunks(data, 100), Some(999))
        .await
        .unwrap_err();
    assert!(matches!(err, BlobError::TooLarge { limit: 999 }), "{err}");
    assert!(
        !store
            .exists(&Key::stash(lb(), "big").unwrap())
            .await
            .unwrap(),
        "nothing is left at the key"
    );
    let ok = store
        .stash(&lb(), "fits", chunks(data, 100), Some(1000))
        .await
        .unwrap();
    assert_eq!(ok.digest.size, 1000);
}

#[tokio::test]
async fn a_failed_source_leaves_nothing() {
    #[derive(Debug, thiserror::Error)]
    #[error("the connection dropped")]
    struct Dropped;

    let (_dir, store) = store();
    let stream = futures::stream::iter([Ok(Bytes::from_static(b"part")), Err(Dropped)]);
    let err = store
        .stash(&lb(), "dropped", stream, None)
        .await
        .unwrap_err();
    assert!(matches!(err, BlobError::Source(_)), "{err}");
    assert!(
        !store
            .exists(&Key::stash(lb(), "dropped").unwrap())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn derived_objects_are_replaceable_and_originals_are_not() {
    // 0039 §3: objects under sha256/ are immutable; thumbnails and cached bytes are
    // caches kept in the store (0039 §6, §11).
    let (_dir, store) = store();
    let stored = store
        .put(&lb(), Bytes::from_static(b"image"))
        .await
        .unwrap();
    let thumb = Key::thumb(lb(), stored.digest.sha256, "120px").unwrap();
    store
        .put_at(&thumb, Bytes::from_static(b"v1"))
        .await
        .unwrap();
    store
        .put_at(&thumb, Bytes::from_static(b"v2"))
        .await
        .unwrap();
    assert_eq!(store.get(&thumb).await.unwrap().as_ref(), b"v2");

    let original = Key::sha256(lb(), stored.digest.sha256);
    let err = store
        .put_at(&original, Bytes::from_static(b"tampered"))
        .await
        .unwrap_err();
    assert!(matches!(err, BlobError::Key(_)), "{err}");
    assert_eq!(store.get(&original).await.unwrap().as_ref(), b"image");
}

#[tokio::test]
async fn listing_walks_a_prefix_and_names_strays() {
    // 0039 §14: the listing the orphan sweep and `verify` walk, strays included.
    let (dir, store) = store();
    let a = store.put(&lb(), Bytes::from_static(b"a")).await.unwrap();
    let b = store.put(&lb(), Bytes::from_static(b"b")).await.unwrap();
    let thumb = Key::thumb(lb(), a.digest.sha256, "64px").unwrap();
    store
        .put_at(&thumb, Bytes::from_static(b"t"))
        .await
        .unwrap();
    let staged = store
        .stash(&lb(), "pending", chunks(b"p", 1), None)
        .await
        .unwrap();
    std::fs::write(dir.path().join("blobs/librarybase/sha256/stray.bin"), b"?").unwrap();

    let mut all: Vec<_> = store
        .list(&Prefix::scope(&lb()))
        .map(|e| e.unwrap())
        .collect()
        .await;
    all.sort_by(|x, y| x.location.cmp(&y.location));
    let keys: Vec<Option<Key>> = all.iter().map(|e| e.key.clone()).collect();
    let mut expected = vec![
        Some(Key::sha256(lb(), a.digest.sha256)),
        Some(Key::sha256(lb(), b.digest.sha256)),
        Some(thumb.clone()),
        Some(staged.key.clone()),
        None,
    ];
    expected.sort_by_key(|k| k.as_ref().map(ToString::to_string));
    let mut got = keys.clone();
    got.sort_by_key(|k| k.as_ref().map(ToString::to_string));
    assert_eq!(got, expected);
    let stray = all.iter().find(|e| e.key.is_none()).unwrap();
    assert_eq!(stray.location, "librarybase/sha256/stray.bin");
    assert_eq!(stray.meta.size, 1);
    assert!(all.iter().all(|e| e.meta.last_modified
        <= std::time::SystemTime::now() + std::time::Duration::from_secs(5)));

    let originals: Vec<_> = store
        .list(&Prefix::kind(&lb(), Kind::Sha256))
        .map(|e| e.unwrap())
        .collect()
        .await;
    assert_eq!(originals.len(), 3, "two objects and the stray");
    let thumbs: Vec<_> = store
        .list(&Prefix::thumbs(&lb(), a.digest.sha256))
        .map(|e| e.unwrap().key.unwrap())
        .collect()
        .await;
    assert_eq!(thumbs, vec![thumb]);
    let none: Vec<_> = store
        .list(&Prefix::scope(&Scope::tenant("empty").unwrap()))
        .collect()
        .await;
    assert!(
        none.is_empty(),
        "a scope with nothing lists nothing, without error"
    );
}

#[tokio::test]
async fn prefix_deletion_clears_a_scope_and_only_that_scope() {
    // 0039 §4, §14: a tenant's files leave with it as a set of objects under one prefix.
    let (_dir, store) = store();
    let other = Scope::tenant("other").unwrap();
    let a = store.put(&lb(), Bytes::from_static(b"a")).await.unwrap();
    store.put(&lb(), Bytes::from_static(b"b")).await.unwrap();
    store
        .put_at(
            &Key::thumb(lb(), a.digest.sha256, "64px").unwrap(),
            Bytes::from_static(b"t"),
        )
        .await
        .unwrap();
    let kept = store.put(&other, Bytes::from_static(b"a")).await.unwrap();

    assert_eq!(store.delete_prefix(&Prefix::scope(&lb())).await.unwrap(), 3);
    let left: Vec<_> = store.list(&Prefix::scope(&lb())).collect().await;
    assert!(left.is_empty());
    assert!(
        store
            .exists(&Key::sha256(other.clone(), kept.digest.sha256))
            .await
            .unwrap(),
        "the other scope is untouched"
    );
    assert_eq!(store.delete_prefix(&Prefix::scope(&lb())).await.unwrap(), 0);

    // Deleting one key, and deleting it again, both succeed.
    let key = Key::sha256(other, kept.digest.sha256);
    store.delete(&key).await.unwrap();
    store.delete(&key).await.unwrap();
    assert!(!store.exists(&key).await.unwrap());
}

#[tokio::test]
async fn the_full_check_hashes_every_byte() {
    // 0039 §14: `full` verifies that each object's bytes hash to its key.
    let (dir, store) = store();
    let stored = store
        .put(&lb(), Bytes::from_static(b"intact"))
        .await
        .unwrap();
    let hash = stored.digest.sha256;
    assert_eq!(
        store.verify(&lb(), hash).await.unwrap(),
        Verified::Intact(stored.digest)
    );
    assert_eq!(
        store.verify(&lb(), Sha256::of(b"absent")).await.unwrap(),
        Verified::Missing
    );

    // Corrupt the object on disk behind the store's back.
    let h = hash.hex();
    let path = dir
        .path()
        .join("blobs/librarybase/sha256")
        .join(&h[0..2])
        .join(&h[2..4])
        .join(&h);
    std::fs::write(&path, b"damaged").unwrap();
    match store.verify(&lb(), hash).await.unwrap() {
        Verified::Corrupt { expected, actual } => {
            assert_eq!(expected, hash);
            assert_eq!(actual.sha256, Sha256::of(b"damaged"));
            assert_eq!(actual.size, 7);
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[tokio::test]
async fn a_second_store_over_the_same_directory_reads_the_same_objects() {
    // 0039 §3: the fs backend is a directory; two servers over one mount see one store.
    let (dir, store) = store();
    let stored = store
        .put(&lb(), Bytes::from_static(b"shared disk"))
        .await
        .unwrap();
    let again = BlobStore::filesystem(dir.path().join("blobs")).unwrap();
    let key = Key::sha256(lb(), stored.digest.sha256);
    assert_eq!(again.get(&key).await.unwrap().as_ref(), b"shared disk");
    let dup = again
        .put(&lb(), Bytes::from_static(b"shared disk"))
        .await
        .unwrap();
    assert!(dup.existed);
}
