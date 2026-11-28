//! The downloaded price list (T14).

use loom_core::run::PriceListStore;
use sqlx::{PgPool, Row};

use crate::StoreError;

/// The price list store in PostgreSQL.
#[derive(Debug, Clone)]
pub struct PgPriceListStore {
    pool: PgPool,
}

impl PgPriceListStore {
    /// A store on `pool`; the migrations must have run.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PriceListStore for PgPriceListStore {
    type Error = StoreError;

    async fn save(&self, source_url: &str, body: &str) -> Result<(), StoreError> {
        sqlx::query("insert into price_lists (source_url, body) values ($1, $2::jsonb)")
            .bind(source_url)
            .bind(body)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn latest(&self) -> Result<Option<String>, StoreError> {
        let row = sqlx::query(
            "select body::text as body from price_lists order by fetched_at desc limit 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.try_get("body")).transpose()?)
    }
}
