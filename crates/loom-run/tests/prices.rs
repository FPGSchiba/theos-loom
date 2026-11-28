//! The price book and the price-list refresh.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;

use loom_core::run::{PriceListStore, Usage};
use loom_run::prices::{self, ModelPrice, PriceBook, PriceError};
use loom_store::PgPriceListStore;
use loom_store::testing::TestDb;

const FIXTURE: &str = include_str!("fixtures/prices/litellm_subset.json");

fn book(overrides: &BTreeMap<String, ModelPrice>) -> PriceBook {
    let parsed = PriceBook::from_litellm(FIXTURE, overrides);
    assert!(parsed.is_ok(), "the fixture parses");
    parsed.unwrap_or_default()
}

fn one(model: &str, usage: Usage) -> BTreeMap<String, Usage> {
    BTreeMap::from([(model.to_owned(), usage)])
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}

// AC: TU-03.4
#[test]
fn spend_prices_each_kind_at_its_own_price() {
    let usage = Usage {
        input: 1_000,
        output: 2_000,
        cache_write_5m: 3_000,
        cache_write_1h: 4_000,
        cache_read: 5_000,
    };
    let spend = book(&BTreeMap::new())
        .spend(&one("model-a", usage))
        .expect("priced");
    // 1000*3e-6 + 2000*15e-6 + 3000*3.75e-6 + 4000*6e-6 + 5000*3e-7
    close(spend, 0.003 + 0.03 + 0.01125 + 0.024 + 0.0015);
}

#[test]
fn an_override_beats_the_list() {
    let overrides = BTreeMap::from([(
        "model-a".to_owned(),
        ModelPrice {
            input: Some(0.5),
            ..ModelPrice::default()
        },
    )]);
    let usage = Usage {
        input: 2,
        output: 1,
        ..Usage::default()
    };
    let spend = book(&overrides)
        .spend(&one("model-a", usage))
        .expect("priced");
    close(spend, 1.0 + 0.000_015);
}

// AC: TU-03.4
#[test]
fn a_model_without_a_price_is_no_price() {
    let usage = Usage {
        input: 1,
        ..Usage::default()
    };
    let err = book(&BTreeMap::new())
        .spend(&one("unknown", usage))
        .expect_err("no price");
    assert_eq!(err.model, "unknown");
}

#[test]
fn a_missing_1h_price_is_no_price_only_when_used() {
    let book = book(&BTreeMap::new());
    let without = Usage {
        input: 1,
        cache_write_5m: 1,
        ..Usage::default()
    };
    assert!(book.spend(&one("model-b", without)).is_ok());
    let with = Usage {
        cache_write_1h: 1,
        ..without
    };
    assert_eq!(
        book.spend(&one("model-b", with)).expect_err("1h").model,
        "model-b"
    );
}

#[test]
fn the_anthropic_prefix_is_tried_second() {
    let usage = Usage {
        input: 1,
        output: 1,
        ..Usage::default()
    };
    let spend = book(&BTreeMap::new())
        .spend(&one("model-c", usage))
        .expect("priced");
    close(spend, 0.00001 + 0.00005);
}

/// Serves each body once, in order, one connection each.
fn serve(bodies: Vec<&'static str>) -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/prices.json", listener.local_addr()?);
    std::thread::spawn(move || {
        for body in bodies {
            let Ok((mut conn, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 2048];
            let _ = conn.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = conn.write_all(head.as_bytes());
            let _ = conn.write_all(body.as_bytes());
        }
    });
    Ok(url)
}

/// The store keeps JSON as `jsonb`, so the saved copy equals the fixture as a value.
async fn assert_saved_is_fixture(store: &PgPriceListStore) {
    let saved = store.latest().await.ok().flatten().unwrap_or_default();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap_or_default();
    let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap_or_default();
    assert_ne!(fixture, serde_json::Value::Null, "the fixture parses");
    assert_eq!(saved, fixture);
}

#[tokio::test]
async fn refresh_saves_a_good_list() {
    let db = TestDb::create().await.expect("db");
    let store = PgPriceListStore::new(db.pool().clone());
    let url = serve(vec![FIXTURE]).expect("serve");
    let book = prices::refresh(&url, &store, &BTreeMap::new())
        .await
        .expect("refresh");
    let usage = Usage {
        input: 1,
        ..Usage::default()
    };
    assert!(book.spend(&one("model-a", usage)).is_ok());
    assert_saved_is_fixture(&store).await;
    db.drop_db().await.expect("drop");
}

#[tokio::test]
async fn refresh_falls_back_to_the_last_good_copy() {
    let db = TestDb::create().await.expect("db");
    let store = PgPriceListStore::new(db.pool().clone());
    let url = serve(vec![FIXTURE, "this is not json"]).expect("serve");
    prices::refresh(&url, &store, &BTreeMap::new())
        .await
        .expect("first");
    let book = prices::refresh(&url, &store, &BTreeMap::new())
        .await
        .expect("fallback");
    let usage = Usage {
        input: 1,
        ..Usage::default()
    };
    assert!(book.spend(&one("model-a", usage)).is_ok());
    assert_saved_is_fixture(&store).await;
    db.drop_db().await.expect("drop");
}

#[tokio::test]
async fn refresh_without_any_copy_is_no_price_list() {
    let db = TestDb::create().await.expect("db");
    let store = PgPriceListStore::new(db.pool().clone());
    let url = serve(vec!["garbage"]).expect("serve");
    let err = prices::refresh(&url, &store, &BTreeMap::new())
        .await
        .expect_err("none");
    assert!(matches!(err, PriceError::NoPriceList));
    db.drop_db().await.expect("drop");
}
