//! The process seam (R-p): how the runner starts the CLI. A container launcher (Podman, Linux)
//! and a local-process launcher implement it; the runner only sees a [`Session`] that runs
//! commands and can be stopped.

mod local;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use loom_core::run::RunId;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

pub use local::LocalLauncher;

/// A boxed, sendable future, for the object-safe [`Session`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A byte stream an [`Exec`] writes.
pub type Pipe = Box<dyn AsyncRead + Send + Unpin>;

/// Starting or driving a session failed.
#[derive(Debug, Error)]
pub enum LaunchError {
    /// A process could not be started.
    #[error("could not start {program}: {source}")]
    Spawn {
        /// The program.
        program: String,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// Waiting for a process failed.
    #[error("waiting for the process failed: {0}")]
    Wait(#[source] std::io::Error),
    /// The session was stopped; it runs nothing more.
    #[error("the session is stopped")]
    Stopped,
    /// Any other failure of the launcher.
    #[error("{0}")]
    Other(String),
}

/// What a launcher needs to start the session of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The run.
    pub run: RunId,
    /// The run's clone, the CLI's working directory.
    pub workspace: PathBuf,
    /// The kit's checkout.
    pub kit: PathBuf,
    /// Names of environment variables to pass on from Loom's environment; never their values.
    pub env: Vec<String>,
}

/// Starts sessions.
pub trait Launcher: Send + Sync {
    /// Starts the session of one run.
    fn start(
        &self,
        ctx: &LaunchContext,
    ) -> impl Future<Output = Result<Box<dyn Session>, LaunchError>> + Send;

    /// Removes everything this launcher may have created for `run` (containers, networks, ...).
    /// The runner calls it when a start failed or was cancelled before it returned a session,
    /// so nothing half-started is left. It must succeed when nothing of `run` exists.
    fn abort(&self, run: RunId) -> impl Future<Output = Result<(), LaunchError>> + Send;
}

/// The place a run's commands run in (a container, or the local machine), alive until stopped.
pub trait Session: Send {
    /// Runs the CLI with `args`; `stdin` is written to its standard input, which is closed
    /// after it (no `stdin`: an empty standard input).
    fn exec<'a>(
        &'a mut self,
        args: &'a [String],
        stdin: Option<String>,
    ) -> BoxFuture<'a, Result<Exec, LaunchError>>;

    /// Ends every command of the session and the session itself; waits until they are gone.
    fn stop(&mut self) -> BoxFuture<'_, Result<(), LaunchError>>;

    /// The image the session runs, when it has one.
    fn image_id(&self) -> Option<String>;

    /// The kit's directory as the CLI sees it (for `--plugin-dir`).
    fn kit_dir(&self) -> String;
}

/// One running command: its two output streams and its end.
pub struct Exec {
    /// Standard output.
    pub stdout: Pipe,
    /// Standard error.
    pub stderr: Pipe,
    /// Resolves to the exit code when the command ends (`None` when it had none, such as when
    /// it was killed).
    pub exit: BoxFuture<'static, Result<Option<i32>, LaunchError>>,
}

impl std::fmt::Debug for Exec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Exec").finish_non_exhaustive()
    }
}

/// What a command printed, and its exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    /// The exit code, when the command had one.
    pub code: Option<i32>,
    /// Everything it wrote to standard output.
    pub stdout: Vec<u8>,
    /// Everything it wrote to standard error.
    pub stderr: Vec<u8>,
}

impl Exec {
    /// Waits for the command to end and collects both its outputs.
    ///
    /// # Errors
    /// [`LaunchError::Wait`] when an output cannot be read; the exit's own error otherwise.
    pub async fn collect(mut self) -> Result<Collected, LaunchError> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let (out, err, code) = tokio::join!(
            self.stdout.read_to_end(&mut stdout),
            self.stderr.read_to_end(&mut stderr),
            self.exit,
        );
        out.map_err(LaunchError::Wait)?;
        err.map_err(LaunchError::Wait)?;
        Ok(Collected {
            code: code?,
            stdout,
            stderr,
        })
    }
}
