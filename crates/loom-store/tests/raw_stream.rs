//! Behaviour of the raw stream store against a throwaway PostgreSQL.

use std::time::Duration;

use loom_core::run::{RawStreamStore, RunId, StreamKind};
use loom_store::testing::TestDb;
use loom_store::{PgRawStreamStore, prune_raw_streams};

// AC: TU-03.2
#[tokio::test]
async fn appended_chunks_read_back_byte_for_byte() {
    let db = TestDb::create().await.unwrap();
    let store = PgRawStreamStore::new(db.pool().clone());
    let run = RunId::new();
    store
        .append(run, StreamKind::Stdout, b"{\"a\":1}\n")
        .await
        .unwrap();
    store
        .append(run, StreamKind::Stderr, b"warn\n")
        .await
        .unwrap();
    store
        .append(run, StreamKind::Stdout, b"\xff\x00raw\n")
        .await
        .unwrap();
    assert_eq!(
        store.read(run, StreamKind::Stdout).await.unwrap(),
        b"{\"a\":1}\n\xff\x00raw\n"
    );
    assert_eq!(
        store.read(run, StreamKind::Stderr).await.unwrap(),
        b"warn\n"
    );
    db.drop_db().await.unwrap();
}

#[tokio::test]
async fn chunks_of_two_runs_stay_apart() {
    let db = TestDb::create().await.unwrap();
    let store = PgRawStreamStore::new(db.pool().clone());
    let (a, b) = (RunId::new(), RunId::new());
    store.append(a, StreamKind::Stdout, b"a1").await.unwrap();
    store.append(b, StreamKind::Stdout, b"b1").await.unwrap();
    store.append(a, StreamKind::Stdout, b"a2").await.unwrap();
    assert_eq!(store.read(a, StreamKind::Stdout).await.unwrap(), b"a1a2");
    assert_eq!(store.read(b, StreamKind::Stdout).await.unwrap(), b"b1");
    db.drop_db().await.unwrap();
}

#[tokio::test]
async fn prune_deletes_only_older_chunks() {
    let db = TestDb::create().await.unwrap();
    let store = PgRawStreamStore::new(db.pool().clone());
    let run = RunId::new();
    store.append(run, StreamKind::Stdout, b"old").await.unwrap();
    store.append(run, StreamKind::Stdout, b"new").await.unwrap();
    sqlx::query(
        "update run_stream_chunks set written_at = now() - interval '31 days' where seq = 0",
    )
    .execute(db.pool())
    .await
    .unwrap();
    let deleted = prune_raw_streams(db.pool(), Duration::from_hours(720))
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(store.read(run, StreamKind::Stdout).await.unwrap(), b"new");
    db.drop_db().await.unwrap();
}
