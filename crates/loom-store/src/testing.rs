//! Throwaway databases for tests (T4b). Used by the tests of `loom-store` and `loom-run`.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

use crate::{StoreError, migrate};

const VARIABLE: &str = "LOOM_TEST_DATABASE_URL";

/// A database created for one test, migrated, and dropped with [`TestDb::drop_db`].
#[derive(Debug)]
pub struct TestDb {
    pool: PgPool,
    admin: PgConnectOptions,
    name: String,
}

fn url_from(value: Option<String>) -> Result<String, StoreError> {
    value.ok_or_else(|| {
        StoreError::Testing(format!(
            "{VARIABLE} is not set; start a throwaway PostgreSQL (see crates/loom-store/README.md)"
        ))
    })
}

impl TestDb {
    /// Creates `loom_test_<uuid>` on the server named by `LOOM_TEST_DATABASE_URL` and migrates it.
    ///
    /// # Errors
    /// Fails when the variable is not set or the server refuses.
    pub async fn create() -> Result<Self, StoreError> {
        let url = url_from(std::env::var(VARIABLE).ok())?;
        let admin = PgConnectOptions::from_str(&url)?;
        let name = format!("loom_test_{}", Uuid::new_v4().simple());
        let mut conn = PgConnection::connect_with(&admin).await?;
        sqlx::query(&format!("create database {name}"))
            .execute(&mut conn)
            .await?;
        conn.close().await?;
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(admin.clone().database(&name))
            .await?;
        migrate(&pool).await?;
        Ok(Self { pool, admin, name })
    }

    /// The pool of the throwaway database.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Closes the pool and drops the database.
    ///
    /// # Errors
    /// Fails when the server refuses.
    pub async fn drop_db(self) -> Result<(), StoreError> {
        self.pool.close().await;
        let mut conn = PgConnection::connect_with(&self.admin).await?;
        sqlx::query(&format!(
            "drop database if exists {} with (force)",
            self.name
        ))
        .execute(&mut conn)
        .await?;
        conn.close().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testing_without_the_variable_says_how_to_start_one() {
        let err = url_from(None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "LOOM_TEST_DATABASE_URL is not set; start a throwaway PostgreSQL (see crates/loom-store/README.md)"
        );
    }
}
