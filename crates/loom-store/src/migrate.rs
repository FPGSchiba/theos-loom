//! Runs the migrations of this crate.

use sqlx::PgPool;

use crate::StoreError;

/// Applies every migration in `migrations/` that the database has not seen yet.
///
/// # Errors
/// Fails when a migration cannot be applied.
pub async fn migrate(pool: &PgPool) -> Result<(), StoreError> {
    sqlx::migrate!().run(pool).await?;
    Ok(())
}
