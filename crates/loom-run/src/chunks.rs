//! Raw stream chunks (R-h): bytes of one stream are buffered and appended to the store every
//! 64 KiB, or one second after the first byte not yet stored, whichever comes first.

use std::sync::Arc;
use std::time::Duration;

use loom_core::run::{RawStreamStore, RunId, StreamKind};
use tokio::time::Instant;

/// Bytes buffered before they are appended.
pub const CHUNK_BYTES: usize = 64 * 1024;
/// Longest time a byte waits in the buffer.
pub const CHUNK_AGE: Duration = Duration::from_secs(1);

/// Buffers the bytes of one stream of a run and appends them to the store in chunks.
///
/// A failed append is logged and its bytes are dropped: a database hiccup does not stop a run.
#[derive(Debug)]
pub struct ChunkWriter<S: RawStreamStore> {
    store: Arc<S>,
    run: RunId,
    stream: StreamKind,
    buffer: Vec<u8>,
    /// When the oldest byte in `buffer` arrived.
    since: Option<Instant>,
}

impl<S: RawStreamStore> ChunkWriter<S> {
    /// A writer for `stream` of `run`.
    #[must_use]
    pub fn new(store: Arc<S>, run: RunId, stream: StreamKind) -> Self {
        Self {
            store,
            run,
            stream,
            buffer: Vec::new(),
            since: None,
        }
    }

    /// Buffers `bytes`, and appends the buffer when it is full or old enough.
    pub async fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.buffer.is_empty() {
            self.since = Some(Instant::now());
        }
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() >= CHUNK_BYTES {
            self.flush().await;
        } else {
            self.flush_if_due().await;
        }
    }

    /// When the buffer must be appended at the latest, if it holds anything.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.since.map(|since| since + CHUNK_AGE)
    }

    /// Appends the buffer if its oldest byte has waited [`CHUNK_AGE`].
    pub async fn flush_if_due(&mut self) {
        if self.deadline().is_some_and(|due| Instant::now() >= due) {
            self.flush().await;
        }
    }

    /// Appends what is left.
    pub async fn finish(mut self) {
        self.flush().await;
    }

    async fn flush(&mut self) {
        self.since = None;
        if self.buffer.is_empty() {
            return;
        }
        let chunk = std::mem::take(&mut self.buffer);
        if let Err(error) = self.store.append(self.run, self.stream, &chunk).await {
            tracing::error!(%error, run = %self.run, stream = ?self.stream, bytes = chunk.len(), "could not store a raw stream chunk");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    struct MemStore {
        chunks: Mutex<Vec<(StreamKind, Vec<u8>)>>,
    }

    impl MemStore {
        fn sizes(&self) -> Vec<usize> {
            self.chunks
                .lock()
                .unwrap()
                .iter()
                .map(|(_, c)| c.len())
                .collect()
        }
    }

    impl RawStreamStore for MemStore {
        type Error = std::io::Error;

        fn append(
            &self,
            _run: RunId,
            stream: StreamKind,
            chunk: &[u8],
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            self.chunks.lock().unwrap().push((stream, chunk.to_vec()));
            std::future::ready(Ok(()))
        }

        fn read(
            &self,
            _run: RunId,
            stream: StreamKind,
        ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
            let chunks = self.chunks.lock().unwrap();
            std::future::ready(Ok(chunks
                .iter()
                .filter(|(s, _)| *s == stream)
                .flat_map(|(_, c)| c.clone())
                .collect()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn chunks_flush_at_64_kib_and_after_a_second() {
        let store = Arc::new(MemStore::default());
        let mut writer = ChunkWriter::new(store.clone(), RunId::new(), StreamKind::Stderr);

        writer.write(b"small").await;
        assert!(store.sizes().is_empty(), "a small write waits");
        assert!(writer.deadline().is_some());
        tokio::time::advance(Duration::from_millis(999)).await;
        writer.flush_if_due().await;
        assert!(store.sizes().is_empty(), "not yet a second");
        tokio::time::advance(Duration::from_millis(1)).await;
        writer.flush_if_due().await;
        assert_eq!(store.sizes(), vec![5], "flushed after a second");
        assert_eq!(writer.deadline(), None);

        writer.write(&vec![b'x'; CHUNK_BYTES - 1]).await;
        assert_eq!(store.sizes(), vec![5], "one byte short of a chunk waits");
        writer.write(b"y").await;
        assert_eq!(
            store.sizes(),
            vec![5, CHUNK_BYTES],
            "a full chunk goes at once"
        );

        writer.write(b"a").await;
        tokio::time::advance(Duration::from_millis(600)).await;
        writer.write(b"b").await;
        assert_eq!(store.sizes(), vec![5, CHUNK_BYTES]);
        tokio::time::advance(Duration::from_millis(400)).await;
        writer.flush_if_due().await;
        assert_eq!(
            store.sizes(),
            vec![5, CHUNK_BYTES, 2],
            "the age counts from the first unflushed byte"
        );

        writer.write(b"tail").await;
        writer.finish().await;
        assert_eq!(
            store.sizes(),
            vec![5, CHUNK_BYTES, 2, 4],
            "finish appends the rest"
        );
        let all = store.read(RunId::new(), StreamKind::Stderr).await.unwrap();
        assert!(all.ends_with(b"abtail"));
    }
}
