//! One run's state, and its steps from the clone to the started CLI (spec §5.3 steps 2–5).

use std::future::Future;
use std::sync::Arc;

use loom_core::run::{CapKind, RawStreamStore, RunEvent, RunId, StopCause, StreamKind};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;

use crate::chunks::ChunkWriter;
use crate::config::{RunConfig, RunSpec};
use crate::deadline::sleep_until_some;
use crate::kit::{Kit, KitError};
use crate::launcher::{LaunchContext, LaunchError, Launcher, Session};
use crate::prices::PriceBook;
use crate::pump;
use crate::usage::UsageLedger;

use super::{PreludeLimits, Runner};
use crate::workspace::{Workspace, WorkspaceError};

/// Parsed stdout events buffered between the stdout pump and the read loop.
const LINE_BUFFER: usize = 256;
/// Longest part of a failed update's output kept in its event.
const UPDATE_MESSAGE_CHARS: usize = 500;

/// A step of the run failed; the run ends as `Failed` and the error is logged.
#[derive(Debug, Error)]
pub(super) enum StepError {
    /// The clone could not be made.
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    /// The kit could not be prepared.
    #[error(transparent)]
    Kit(#[from] KitError),
    /// The launcher or a command failed.
    #[error(transparent)]
    Launch(#[from] LaunchError),
}

/// Everything one run owns. The steps fill it as they go, so the end path can clean up after
/// any of them, also when a cancel drops a step half-way.
pub(super) struct Run<L, S: RawStreamStore> {
    pub(super) id: RunId,
    pub(super) spec: RunSpec,
    pub(super) config: Arc<RunConfig>,
    pub(super) launcher: Arc<L>,
    store: Arc<S>,
    pub(super) prices: Option<Arc<PriceBook>>,
    events: mpsc::Sender<RunEvent>,
    pub(super) session: Option<Box<dyn Session>>,
    /// Set when the launcher was asked to start: from then on the end path has a session to
    /// stop or a half-made start to abort.
    pub(super) launching: bool,
    limits: PreludeLimits,
    /// The tasks reading the CLI's stdout and stderr.
    pub(super) pumps: Vec<JoinHandle<()>>,
    /// Stops the pumps when the CLI's output does not end by itself.
    pub(super) pump_stop: CancellationToken,
    pub(super) ledger: UsageLedger,
    /// The `total_cost_usd` of the last `result` event.
    pub(super) reported_usd: Option<f64>,
}

impl<L: Launcher + 'static, S: RawStreamStore + 'static> Run<L, S> {
    pub(super) fn new(
        id: RunId,
        spec: RunSpec,
        runner: &Runner<L, S>,
        events: mpsc::Sender<RunEvent>,
    ) -> Self {
        Self {
            id,
            spec,
            config: Arc::clone(&runner.config),
            launcher: Arc::clone(&runner.launcher),
            store: Arc::clone(&runner.store),
            prices: runner.prices.clone(),
            events,
            session: None,
            launching: false,
            limits: runner.limits,
            pumps: Vec::new(),
            pump_stop: CancellationToken::new(),
            ledger: UsageLedger::default(),
            reported_usd: None,
        }
    }

    /// Runs to the end: the steps until a stop cause, or a cancel; then the end path.
    pub(super) async fn drive(mut self, cancel: CancellationToken) -> StopCause {
        let cause = tokio::select! {
            biased;
            () = cancel.cancelled() => StopCause::Cancelled,
            cause = self.steps() => cause,
        };
        self.end(&cause).await;
        cause
    }

    /// Sends an event to the caller; a caller that stopped listening is not an error. The
    /// future owns its sender, so no borrow of the run (whose session is not `Sync`) is held
    /// while it waits.
    pub(super) fn emit(&self, event: RunEvent) -> impl Future<Output = ()> + Send + 'static {
        let events = self.events.clone();
        async move {
            let _ = events.send(event).await;
        }
    }

    async fn steps(&mut self) -> StopCause {
        match self.try_steps().await {
            Ok(cause) => cause,
            Err(error) => {
                tracing::error!(%error, run = %self.id, "a step of the run failed");
                StopCause::Failed { exit_code: None }
            }
        }
    }

    async fn try_steps(&mut self) -> Result<StopCause, StepError> {
        let workspace = Workspace::prepare(&self.config, self.id, &self.spec).await?;
        let work = workspace.path().to_owned();
        let kit = Kit::prepare(&self.config, &self.spec.kit).await?;
        self.emit(RunEvent::Kit {
            repository: self.spec.kit.repository.clone(),
            version: self.spec.kit.version.clone(),
        })
        .await;
        let ctx = LaunchContext {
            run: self.id,
            workspace: work,
            kit: kit.path().to_owned(),
            env: self.config.credentials.clone(),
        };
        self.launching = true;
        let mut session = self.launcher.start(&ctx).await?;
        let started = Instant::now();
        let image = session.image_id();
        if let Some(extra) = session.take_events() {
            let events = self.events.clone();
            self.pumps
                .push(forward(extra, events, self.pump_stop.clone()));
        }
        self.session = Some(session);
        if let Some(id) = image {
            self.emit(RunEvent::Image { id }).await;
        }
        // The read loop checks the time cap itself; the prelude is guarded here.
        let deadline = self.spec.caps.wall_time.map(|cap| started + cap);
        let prelude = tokio::select! {
            prelude = self.prelude() => prelude?,
            () = sleep_until_some(deadline) => Some(StopCause::Cap(CapKind::WallTime)),
        };
        if let Some(cause) = prelude {
            return Ok(cause);
        }
        let kit_dir = self.session()?.kit_dir();
        let args = cli_args(&self.spec, &kit_dir);
        let prompt = self.spec.prompt.clone();
        let exec = self.session()?.exec(&args, Some(prompt)).await?;
        let (lines, rx) = mpsc::channel(LINE_BUFFER);
        let stdout = self.writer(StreamKind::Stdout);
        let stderr = self.writer(StreamKind::Stderr);
        self.pumps.push(pump::spawn(
            exec.stdout,
            stdout,
            Some(lines),
            self.pump_stop.clone(),
        ));
        self.pumps.push(pump::spawn(
            exec.stderr,
            stderr,
            None,
            self.pump_stop.clone(),
        ));
        self.watch(rx, exec.exit, started).await
    }

    /// `claude update` (a failure is recorded and the run goes on, R-f), then
    /// `claude --version` (T14). Returns a stop cause when the version cannot be read.
    async fn prelude(&mut self) -> Result<Option<StopCause>, StepError> {
        let update = timeout(self.limits.update, self.collect(&["update"])).await;
        let failure = match update {
            Err(_) => Some(format!(
                "`claude update` did not end within {:?}",
                self.limits.update
            )),
            Ok(Ok(out)) if out.code == Some(0) => None,
            Ok(Ok(out)) => Some(format!(
                "`claude update` exited with {}: {}",
                out.code
                    .map_or_else(|| "no code".to_owned(), |c| c.to_string()),
                truncated(&String::from_utf8_lossy(&out.stderr))
            )),
            Ok(Err(error)) => Some(error.to_string()),
        };
        if let Some(message) = failure {
            tracing::warn!(run = %self.id, %message, "the CLI update failed; the run goes on");
            self.emit(RunEvent::CliUpdateFailed { message }).await;
        }
        let limit = self.limits.version;
        let Ok(version) = timeout(limit, self.collect(&["--version"])).await else {
            tracing::error!(run = %self.id, ?limit, "`claude --version` did not end in time");
            return Ok(Some(StopCause::Failed { exit_code: None }));
        };
        let version = version?;
        if version.code != Some(0) {
            tracing::error!(run = %self.id, code = ?version.code, "`claude --version` failed");
            return Ok(Some(StopCause::Failed {
                exit_code: version.code,
            }));
        }
        let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
        self.emit(RunEvent::CliVersion { version }).await;
        Ok(None)
    }

    async fn collect(&mut self, args: &[&str]) -> Result<crate::launcher::Collected, LaunchError> {
        let args: Vec<String> = args.iter().map(|&a| a.to_owned()).collect();
        self.session()?.exec(&args, None).await?.collect().await
    }

    /// The started session; the steps only ask for it after the launcher started it.
    fn session(&mut self) -> Result<&mut Box<dyn Session>, LaunchError> {
        self.session
            .as_mut()
            .ok_or_else(|| LaunchError::Other("no session".to_owned()))
    }

    fn writer(&self, stream: StreamKind) -> ChunkWriter<S> {
        ChunkWriter::new(Arc::clone(&self.store), self.id, stream)
    }
}

/// Passes the session's own events (the proxy's `Egress` events) on to the caller until their
/// channel closes, which it does once the session stopped, or until `stop` fires. Its handle
/// joins the pumps', so the end path waits for it before `RunEnded`.
fn forward(
    mut extra: mpsc::Receiver<RunEvent>,
    events: mpsc::Sender<RunEvent>,
    stop: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                event = extra.recv() => match event {
                    // The caller may have stopped listening; that is no error.
                    Some(event) => { let _ = events.send(event).await; }
                    None => break,
                },
                () = stop.cancelled() => break,
            }
        }
    })
}

/// The arguments of the CLI's run (spec §5.3 step 5); the prompt goes on stdin (R-k).
fn cli_args(spec: &RunSpec, kit_dir: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--dangerously-skip-permissions",
        "--plugin-dir",
        kit_dir,
    ]
    .iter()
    .map(|&a| a.to_owned())
    .collect();
    if let Some(model) = &spec.model {
        args.push("--model".to_owned());
        args.push(model.clone());
    }
    args.extend(spec.extra_args.iter().cloned());
    args
}

fn truncated(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(UPDATE_MESSAGE_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_update_message_is_cut() {
        let long = "x".repeat(UPDATE_MESSAGE_CHARS + 10);
        let cut = truncated(&long);
        assert_eq!(cut.chars().count(), UPDATE_MESSAGE_CHARS + 1);
        assert_eq!(truncated(" short \n"), "short");
    }
}
