//! The error of this crate.

/// What can go wrong in the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A database call failed.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    /// Running the migrations failed.
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// A test database could not be set up.
    #[error("{0}")]
    Testing(String),
}
