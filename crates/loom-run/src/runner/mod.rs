//! The runner (spec §5.3): prepares a run, starts the CLI through a [`Launcher`], reads its
//! stream, enforces the caps and stop causes, and cleans up. Events go out on a channel; the
//! caller gets a [`RunHandle`] to cancel the run or wait for its end.
//!
//! The run is split by step: [`run`] holds the run's state and its start, `watch` the read loop
//! with the caps, and `end` the one path every run ends through.

mod end;
mod run;
mod watch;

use std::sync::Arc;

use loom_core::run::{RawStreamStore, RunEvent, RunId, StopCause};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{RunConfig, RunSpec};
use crate::launcher::Launcher;
use crate::prices::PriceBook;

use run::Run;

/// Events buffered for the caller before the run waits for it to read.
const EVENT_BUFFER: usize = 1024;

/// A run could not be started.
#[derive(Debug, Error)]
pub enum RunError {
    /// `start` was called outside a Tokio runtime.
    #[error("a run needs a Tokio runtime")]
    NoRuntime,
}

/// Starts runs with one configuration, launcher, raw stream store and price book.
#[derive(Debug)]
pub struct Runner<L, S> {
    config: Arc<RunConfig>,
    launcher: Arc<L>,
    store: Arc<S>,
    prices: Option<Arc<PriceBook>>,
}

impl<L, S> Runner<L, S>
where
    L: Launcher + 'static,
    S: RawStreamStore + 'static,
{
    /// A runner. Without `prices`, a run with a spend cap stops with `NoPrice` (R-g) and no
    /// spend is cross-checked.
    #[must_use]
    pub fn new(config: RunConfig, launcher: L, store: S, prices: Option<PriceBook>) -> Self {
        Self {
            config: Arc::new(config),
            launcher: Arc::new(launcher),
            store: Arc::new(store),
            prices: prices.map(Arc::new),
        }
    }

    /// Starts a run of `spec` in the background. Its events arrive on the receiver, the last
    /// one always `RunEnded`; the receiver must be read, or the run waits for it.
    ///
    /// # Errors
    /// [`RunError::NoRuntime`] outside a Tokio runtime.
    pub fn start(&self, spec: RunSpec) -> Result<(RunHandle, mpsc::Receiver<RunEvent>), RunError> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| RunError::NoRuntime)?;
        let id = RunId::new();
        let cancel = CancellationToken::new();
        let (events, rx) = mpsc::channel(EVENT_BUFFER);
        let run = Run::new(
            id,
            spec,
            Arc::clone(&self.config),
            Arc::clone(&self.launcher),
            Arc::clone(&self.store),
            self.prices.clone(),
            events,
        );
        let task = runtime.spawn(run.drive(cancel.clone()));
        Ok((RunHandle { id, cancel, task }, rx))
    }
}

/// The handle of a started run.
#[derive(Debug)]
pub struct RunHandle {
    /// The run's id.
    pub id: RunId,
    cancel: CancellationToken,
    task: JoinHandle<StopCause>,
}

impl RunHandle {
    /// Cancels the run; it ends with `Cancelled` unless it has already ended.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Waits for the run to end and returns its cause.
    pub async fn join(self) -> StopCause {
        match self.task.await {
            Ok(cause) => cause,
            Err(error) => {
                tracing::error!(%error, run = %self.id, "the run's task failed");
                StopCause::Failed { exit_code: None }
            }
        }
    }
}
