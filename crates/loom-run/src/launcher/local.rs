//! The local-process launcher: runs the CLI as a child process on this machine (R-p). Used by
//! the tests on every platform, and for debugging without containers.

use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::{CancellationToken, DropGuard};

use super::{BoxFuture, Exec, LaunchContext, LaunchError, Launcher, Session};

/// Runs `program` as a local process, in the run's clone, with `KIT_DIR` set to the kit.
///
/// The child inherits Loom's environment (it runs on Loom's own machine), plus the variables
/// given with [`LocalLauncher::with_env`].
#[derive(Debug, Clone)]
pub struct LocalLauncher {
    program: PathBuf,
    env: Vec<(String, String)>,
}

impl LocalLauncher {
    /// A launcher for `program`.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            env: Vec::new(),
        }
    }

    /// Sets an environment variable for every command the launcher's sessions run.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }
}

impl Launcher for LocalLauncher {
    fn start(
        &self,
        ctx: &LaunchContext,
    ) -> impl Future<Output = Result<Box<dyn Session>, LaunchError>> + Send {
        let stop = CancellationToken::new();
        let session: Box<dyn Session> = Box::new(LocalSession {
            program: self.program.clone(),
            env: self.env.clone(),
            ctx: ctx.clone(),
            _guard: stop.clone().drop_guard(),
            stop,
            waiters: Vec::new(),
        });
        // Nothing to wait for: a local session is just its settings.
        std::future::ready(Ok(session))
    }
}

/// The local session: the commands it started, and a token that kills them all.
struct LocalSession {
    program: PathBuf,
    env: Vec<(String, String)>,
    ctx: LaunchContext,
    stop: CancellationToken,
    /// Cancels `stop` when the session is dropped without being stopped.
    _guard: DropGuard,
    /// One task per command, each waiting for its child.
    waiters: Vec<JoinHandle<()>>,
}

impl LocalSession {
    fn spawn(&mut self, args: &[String], stdin: Option<String>) -> Result<Exec, LaunchError> {
        if self.stop.is_cancelled() {
            return Err(LaunchError::Stopped);
        }
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(&self.ctx.workspace)
            .env("KIT_DIR", &self.ctx.kit)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|source| LaunchError::Spawn {
            program: self.program.display().to_string(),
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
        // so a stopped session always kills and reaps its children.
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

impl Session for LocalSession {
    fn exec<'a>(
        &'a mut self,
        args: &'a [String],
        stdin: Option<String>,
    ) -> BoxFuture<'a, Result<Exec, LaunchError>> {
        let exec = self.spawn(args, stdin);
        Box::pin(async move { exec })
    }

    fn stop(&mut self) -> BoxFuture<'_, Result<(), LaunchError>> {
        self.stop.cancel();
        let waiters = std::mem::take(&mut self.waiters);
        Box::pin(async move {
            for waiter in waiters {
                waiter
                    .await
                    .map_err(|e| LaunchError::Other(format!("a wait task failed: {e}")))?;
            }
            Ok(())
        })
    }

    fn image_id(&self) -> Option<String> {
        None
    }

    fn kit_dir(&self) -> String {
        self.ctx.kit.display().to_string()
    }
}
