//! The commands a session started, as local child processes: spawned into an [`Exec`], each
//! waited for in a task of its own, and all killed by one token. Both launchers use it: the local
//! one runs the CLI itself, the Podman one runs `podman exec`.

use std::future::Future;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::{CancellationToken, DropGuard};

use super::{Exec, LaunchError};

/// The children of one session.
pub(super) struct Children {
    stop: CancellationToken,
    /// Cancels `stop` when the session is dropped without being stopped.
    _guard: DropGuard,
    /// One task per command, each waiting for its child.
    waiters: Vec<JoinHandle<()>>,
}

impl Children {
    pub(super) fn new() -> Self {
        let stop = CancellationToken::new();
        Self {
            _guard: stop.clone().drop_guard(),
            stop,
            waiters: Vec::new(),
        }
    }

    /// Spawns `command` (named `program` in errors) with piped output; `stdin` is written to
    /// its standard input, which is then closed (no `stdin`: an empty standard input).
    pub(super) fn spawn(
        &mut self,
        mut command: Command,
        program: &str,
        stdin: Option<String>,
    ) -> Result<Exec, LaunchError> {
        if self.stop.is_cancelled() {
            return Err(LaunchError::Stopped);
        }
        command
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|source| LaunchError::Spawn {
            program: program.to_owned(),
            source,
        })?;
        let missing = || LaunchError::Other("a pipe of the child is missing".to_owned());
        let stdout = child.stdout.take().ok_or_else(missing)?;
        let stderr = child.stderr.take().ok_or_else(missing)?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            tokio::spawn(async move {
                // A CLI that ends before reading its input is not an error of the launcher.
                if let Err(error) = pipe.write_all(text.as_bytes()).await {
                    tracing::debug!(%error, "could not write the CLI's stdin");
                }
            });
        }
        // The child is waited for in a task of its own, which outlives a dropped exit future,
        // so stopped children are always killed and reaped.
        let (tx, rx) = oneshot::channel();
        let stop = self.stop.clone();
        self.waiters.push(tokio::spawn(async move {
            // The receiver may be gone; the result is then of no interest.
            let _ = tx.send(wait_or_kill(child, &stop).await);
        }));
        Ok(Exec {
            stdout: Box::new(stdout),
            stderr: Box::new(stderr),
            exit: Box::pin(async move {
                rx.await
                    .unwrap_or_else(|_| Err(LaunchError::Other("the wait task ended".to_owned())))
            }),
        })
    }

    /// Kills every child; the future waits until they are reaped.
    pub(super) fn stop(
        &mut self,
    ) -> impl Future<Output = Result<(), LaunchError>> + Send + 'static {
        self.stop.cancel();
        let waiters = std::mem::take(&mut self.waiters);
        async move {
            for waiter in waiters {
                waiter
                    .await
                    .map_err(|e| LaunchError::Other(format!("a wait task failed: {e}")))?;
            }
            Ok(())
        }
    }
}

/// The child's exit code; when `stop` fires first, the child is killed and `None` returned.
async fn wait_or_kill(
    mut child: Child,
    stop: &CancellationToken,
) -> Result<Option<i32>, LaunchError> {
    tokio::select! {
        status = child.wait() => status.map(|s| s.code()).map_err(LaunchError::Wait),
        () = stop.cancelled() => {
            // `kill` also waits for the child; one that already ended is no error.
            if let Err(error) = child.kill().await {
                tracing::debug!(%error, "could not kill the CLI");
            }
            Ok(None)
        }
    }
}
