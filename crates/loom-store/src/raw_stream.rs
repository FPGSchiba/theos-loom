//! The raw streams of runs (T7), kept as chunks.

use std::time::Duration;

use loom_core::run::{RawStreamStore, RunId, StreamKind};
use sqlx::{PgPool, Row};

use crate::StoreError;

fn stream_name(stream: StreamKind) -> &'static str {
    match stream {
        StreamKind::Stdout => "stdout",
        StreamKind::Stderr => "stderr",
    }
}

/// The raw stream store in PostgreSQL.
#[derive(Debug, Clone)]
pub struct PgRawStreamStore {
    pool: PgPool,
}

impl PgRawStreamStore {
    /// A store on `pool`; the migrations must have run.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl RawStreamStore for PgRawStreamStore {
    type Error = StoreError;

    async fn append(&self, run: RunId, stream: StreamKind, chunk: &[u8]) -> Result<(), StoreError> {
        // The next `seq` is taken in the same statement; a concurrent append to one run that
        // picks the same `seq` loses on the primary key, so it is retried.
        let mut attempt = 0;
        loop {
            let result = sqlx::query(
                "insert into run_stream_chunks (run_id, seq, stream, body) \
                 select $1, coalesce(max(seq), -1) + 1, $2, $3 \
                 from run_stream_chunks where run_id = $1",
            )
            .bind(run.0)
            .bind(stream_name(stream))
            .bind(chunk)
            .execute(&self.pool)
            .await;
            match result {
                Ok(_) => return Ok(()),
                Err(sqlx::Error::Database(e)) if e.is_unique_violation() && attempt < 4 => {
                    attempt += 1;
                    tracing::debug!(%run, attempt, "raw stream seq clash, retrying");
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    async fn read(&self, run: RunId, stream: StreamKind) -> Result<Vec<u8>, StoreError> {
        let rows = sqlx::query(
            "select body from run_stream_chunks where run_id = $1 and stream = $2 order by seq",
        )
        .bind(run.0)
        .bind(stream_name(stream))
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in rows {
            let body: Vec<u8> = row.try_get("body")?;
            out.extend_from_slice(&body);
        }
        Ok(out)
    }
}

/// Deletes the chunks written more than `older_than` ago and returns how many rows went.
///
/// # Errors
/// Fails when the database call fails.
pub async fn prune_raw_streams(pool: &PgPool, older_than: Duration) -> Result<u64, StoreError> {
    let result = sqlx::query(
        "delete from run_stream_chunks where written_at < now() - make_interval(secs => $1)",
    )
    .bind(older_than.as_secs_f64())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
