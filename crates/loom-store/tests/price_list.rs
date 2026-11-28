//! Behaviour of the price list store against a throwaway PostgreSQL.

use loom_core::run::PriceListStore;
use loom_store::PgPriceListStore;
use loom_store::testing::TestDb;

#[tokio::test]
async fn latest_returns_the_newest_copy() {
    let db = TestDb::create().await.unwrap();
    let store = PgPriceListStore::new(db.pool().clone());
    store
        .save("https://example.test/p.json", "{\"v\":1}")
        .await
        .unwrap();
    store
        .save("https://example.test/p.json", "{\"v\":2}")
        .await
        .unwrap();
    let latest = store.latest().await.unwrap().unwrap();
    let value: serde_json::Value = serde_json::from_str(&latest).unwrap();
    assert_eq!(value["v"], 2);
    db.drop_db().await.unwrap();
}

#[tokio::test]
async fn latest_is_none_when_empty() {
    let db = TestDb::create().await.unwrap();
    let store = PgPriceListStore::new(db.pool().clone());
    assert!(store.latest().await.unwrap().is_none());
    db.drop_db().await.unwrap();
}
