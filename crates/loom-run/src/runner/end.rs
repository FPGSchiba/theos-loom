//! The one path every run ends through (spec §5.3 steps 7–8), whatever its cause: stop the
//! session, store the rest of the output, cross-check the spend, remove the run directory and
//! send `RunEnded` as the last event.

use std::io::ErrorKind;
use std::time::Duration;

use loom_core::run::{RawStreamStore, RunEvent, StopCause};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout_at};
use tokio_util::sync::CancellationToken;

use super::run::Run;
use crate::launcher::Launcher;
use crate::workspace::run_dir;

/// How long the output may take to end by itself after the session stopped.
const PUMP_GRACE: Duration = Duration::from_secs(2);
/// Computed and reported spend further apart than this share are a mismatch (D7).
const SPEND_TOLERANCE: f64 = 0.05;
/// Tries to remove the run directory: a process that was just killed may still hold a file
/// on Windows for a moment.
const REMOVE_TRIES: u32 = 5;
const REMOVE_PAUSE: Duration = Duration::from_millis(200);

impl<L: Launcher + 'static, S: RawStreamStore + 'static> Run<L, S> {
    /// Ends the run with `cause`.
    pub(super) async fn end(&mut self, cause: &StopCause) {
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.stop().await
        {
            tracing::warn!(%error, run = %self.id, "could not stop the session");
        }
        if self.session.is_none()
            && self.launching
            && let Err(error) = self.launcher.abort(self.id).await
        {
            tracing::warn!(%error, run = %self.id, "could not abort the half-made start");
        }
        join_pumps(std::mem::take(&mut self.pumps), &self.pump_stop).await;
        if let Some(event) = self.spend_mismatch() {
            self.emit(event).await;
        }
        if !self.config.keep_run_dirs {
            self.remove_run_dir().await;
        }
        tracing::info!(run = %self.id, ?cause, "the run ended");
        self.emit(RunEvent::RunEnded {
            cause: cause.clone(),
        })
        .await;
    }

    /// A `SpendMismatch` when the CLI reported a cost and Loom can price the usage (D7).
    fn spend_mismatch(&self) -> Option<RunEvent> {
        let reported = self.reported_usd?;
        let computed = self.prices.as_ref()?.spend(&self.ledger.by_model()).ok()?;
        ((computed - reported).abs() > SPEND_TOLERANCE * reported.abs()).then_some(
            RunEvent::SpendMismatch {
                computed_usd: computed,
                reported_usd: reported,
            },
        )
    }

    async fn remove_run_dir(&mut self) {
        // By its path, not through `Workspace`: a cancel during the clone leaves no `Workspace`
        // but may leave a half-made directory.
        let dir = run_dir(&self.config, self.id);
        for attempt in 1..=REMOVE_TRIES {
            match tokio::fs::remove_dir_all(&dir).await {
                Ok(()) => return,
                Err(error) if error.kind() == ErrorKind::NotFound => return,
                Err(error) if attempt == REMOVE_TRIES => {
                    tracing::warn!(%error, run = %self.id, dir = %dir.display(), "could not remove the run directory");
                }
                Err(_) => sleep(REMOVE_PAUSE).await,
            }
        }
    }
}

/// Waits for the pumps to read their output to the end; stops them after [`PUMP_GRACE`].
async fn join_pumps(pumps: Vec<JoinHandle<()>>, stop: &CancellationToken) {
    let deadline = Instant::now() + PUMP_GRACE;
    for mut pump in pumps {
        let result = if let Ok(result) = timeout_at(deadline, &mut pump).await {
            result
        } else {
            stop.cancel();
            pump.await
        };
        if let Err(error) = result {
            tracing::warn!(%error, "a stream reader failed");
        }
    }
}
