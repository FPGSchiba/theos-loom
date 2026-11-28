//! Waiting for a deadline that may not be set.

use tokio::time::{Instant, sleep_until};

/// Sleeps until `due`; without one, never wakes.
pub(crate) async fn sleep_until_some(due: Option<Instant>) {
    match due {
        Some(due) => sleep_until(due).await,
        None => std::future::pending().await,
    }
}
