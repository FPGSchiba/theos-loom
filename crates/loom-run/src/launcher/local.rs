//! The local-process launcher: runs the CLI as a child process on this machine (R-p). Used by
//! the tests on every platform, and for debugging without containers.

use std::future::Future;
use std::path::PathBuf;

use loom_core::run::RunId;
use tokio::process::Command;

use super::child::Children;
use super::{BoxFuture, Exec, LaunchContext, LaunchError, Launcher, Session};

/// Runs `program` as a local process, in the run's clone, with `KIT_DIR` set to the kit.
///
/// The child inherits Loom's environment (it runs on Loom's own machine), plus the variables
/// given with [`LocalLauncher::with_env`]. Its `Debug` shows their names, never their values.
#[derive(Clone)]
pub struct LocalLauncher {
    program: PathBuf,
    env: Vec<(String, String)>,
}

impl std::fmt::Debug for LocalLauncher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.env.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("LocalLauncher")
            .field("program", &self.program)
            .field("env", &names)
            .finish()
    }
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
    /// Nothing to remove: a local start creates nothing before it returns its session.
    fn abort(&self, _run: RunId) -> impl Future<Output = Result<(), LaunchError>> + Send {
        std::future::ready(Ok(()))
    }

    fn start(
        &self,
        ctx: &LaunchContext,
    ) -> impl Future<Output = Result<Box<dyn Session>, LaunchError>> + Send {
        let session: Box<dyn Session> = Box::new(LocalSession {
            program: self.program.clone(),
            env: self.env.clone(),
            ctx: ctx.clone(),
            children: Children::new(),
        });
        // Nothing to wait for: a local session is just its settings.
        std::future::ready(Ok(session))
    }
}

/// The local session: its settings, and the commands it started.
struct LocalSession {
    program: PathBuf,
    env: Vec<(String, String)>,
    ctx: LaunchContext,
    children: Children,
}

impl LocalSession {
    fn spawn(&mut self, args: &[String], stdin: Option<String>) -> Result<Exec, LaunchError> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(&self.ctx.workspace)
            .env("KIT_DIR", &self.ctx.kit)
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        let program = self.program.display().to_string();
        self.children.spawn(command, &program, stdin)
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
        Box::pin(self.children.stop())
    }

    fn image_id(&self) -> Option<String> {
        None
    }

    fn kit_dir(&self) -> String {
        self.ctx.kit.display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::LocalLauncher;

    #[test]
    fn debug_shows_env_names_never_values() {
        let launcher = LocalLauncher::new("claude").with_env("GH_TOKEN", "s3cr3t-value");
        let text = format!("{launcher:?}");
        assert!(text.contains("GH_TOKEN"), "{text}");
        assert!(!text.contains("s3cr3t"), "{text}");
    }
}
