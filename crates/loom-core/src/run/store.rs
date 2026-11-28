//! The storage traits the run runtime writes through. `loom-store` implements them.

use std::future::Future;

use super::event::{RunId, StreamKind};

/// Keeps the raw bytes of a run's streams (T7).
pub trait RawStreamStore: Send + Sync {
    /// The store's error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Appends a chunk to the stream of a run.
    fn append(
        &self,
        run: RunId,
        stream: StreamKind,
        chunk: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Reads the whole stream of a run.
    fn read(
        &self,
        run: RunId,
        stream: StreamKind,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

/// Keeps the downloaded price list (T14).
pub trait PriceListStore: Send + Sync {
    /// The store's error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Saves a downloaded price list with the URL it came from.
    fn save(
        &self,
        source_url: &str,
        body: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// The latest saved price list body, if any.
    fn latest(&self) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;
}
