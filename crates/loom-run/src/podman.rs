//! The Podman command line (R-a), run through `tokio::process`: one method per call Loom makes,
//! so every call can be copied into a shell to debug. Linux only (D2).

use std::process::Stdio;
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

/// A Podman call that did not run or did not succeed.
#[derive(Debug, Error)]
pub enum PodmanError {
    /// The Podman binary could not be started.
    #[error("could not start `{command}`: {source}")]
    Spawn {
        /// The command line, for the message.
        command: String,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// Podman ran and exited with an error.
    #[error("`{command}` failed ({code:?}): {stderr}")]
    Failed {
        /// The command line, for the message.
        command: String,
        /// The exit code, when it had one.
        code: Option<i32>,
        /// What Podman wrote to stderr, trimmed.
        stderr: String,
    },
    /// Podman answered something this module cannot read.
    #[error("`{command}` printed {output:?}, which is not what was expected")]
    Unexpected {
        /// The command line, for the message.
        command: String,
        /// What it printed.
        output: String,
    },
}

/// The Podman CLI at one path, with extra environment variables for every call.
///
/// The variables are set on the Podman process, never on its command line: `--env NAME`
/// then copies them into a container without their values showing in `ps`.
#[derive(Debug, Clone)]
pub struct Podman {
    program: String,
    env: Vec<(String, String)>,
}

impl Podman {
    /// The Podman at `program` (a path, or a name looked up in `PATH`).
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            env: Vec::new(),
        }
    }

    /// Sets an environment variable on every Podman process.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// A command for `podman <args>`, with the environment set, no stdin, and killed when it is
    /// dropped. Callers that stream its output set the pipes themselves.
    #[must_use]
    pub fn command<S: AsRef<str>>(&self, args: &[S]) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(args.iter().map(AsRef::as_ref))
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    fn line<S: AsRef<str>>(&self, args: &[S]) -> String {
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        format!("{} {}", self.program, args.join(" "))
    }

    /// Runs `podman <args>` and returns its trimmed stdout.
    ///
    /// # Errors
    /// [`PodmanError::Spawn`] or [`PodmanError::Failed`].
    pub async fn output<S: AsRef<str>>(&self, args: &[S]) -> Result<String, PodmanError> {
        let line = self.line(args);
        tracing::debug!(command = %line, "podman");
        let output = self
            .command(args)
            .output()
            .await
            .map_err(|source| PodmanError::Spawn {
                command: line.clone(),
                source,
            })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            Err(PodmanError::Failed {
                command: line,
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    /// Whether Podman runs rootless.
    ///
    /// # Errors
    /// The call's error, or [`PodmanError::Unexpected`] for an answer other than `true`/`false`.
    pub async fn is_rootless(&self) -> Result<bool, PodmanError> {
        let args = ["info", "--format", "{{.Host.Security.Rootless}}"];
        match self.output(&args).await?.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(PodmanError::Unexpected {
                command: self.line(&args),
                output: other.to_owned(),
            }),
        }
    }

    /// `podman run -d <args>`; returns the new container's id.
    ///
    /// # Errors
    /// The call's error.
    pub async fn run_detached(&self, args: &[String]) -> Result<String, PodmanError> {
        let mut all = vec!["run".to_owned(), "-d".to_owned()];
        all.extend(args.iter().cloned());
        self.output(&all).await
    }

    /// A command for `podman exec [-i] <id> <args>`; `-i` when `stdin` is to be attached.
    #[must_use]
    pub fn exec(&self, id: &str, args: &[String], stdin: bool) -> Command {
        let mut all = vec!["exec".to_owned()];
        if stdin {
            all.push("-i".to_owned());
        }
        all.push(id.to_owned());
        all.extend(args.iter().cloned());
        tracing::debug!(command = %self.line(&all), "podman");
        self.command(&all)
    }

    /// `podman stop --time <time> <id>`: SIGTERM, then SIGKILL after `time`.
    ///
    /// # Errors
    /// The call's error.
    pub async fn stop(&self, id: &str, time: Duration) -> Result<(), PodmanError> {
        let secs = time.as_secs().to_string();
        self.output(&["stop", "--ignore", "--time", &secs, id])
            .await
            .map(drop)
    }

    /// `podman rm --force` of `id` without waiting; a container that is gone is no error.
    ///
    /// # Errors
    /// The call's error.
    pub async fn rm_force(&self, id: &str) -> Result<(), PodmanError> {
        self.output(&["rm", "--force", "--ignore", "--time", "0", id])
            .await
            .map(drop)
    }

    /// Podman's id of `image` (`podman image inspect`).
    ///
    /// # Errors
    /// The call's error.
    pub async fn image_id(&self, image: &str) -> Result<String, PodmanError> {
        self.output(&["image", "inspect", "--format", "{{.Id}}", image])
            .await
    }

    /// Creates an internal network (no route out) named `name`, with `label` (`key=value`).
    ///
    /// # Errors
    /// The call's error.
    pub async fn network_create_internal(
        &self,
        name: &str,
        label: &str,
    ) -> Result<(), PodmanError> {
        self.output(&["network", "create", "--internal", "--label", label, name])
            .await
            .map(drop)
    }

    /// Removes the network `name`, and any container still on it.
    ///
    /// # Errors
    /// The call's error.
    pub async fn network_rm(&self, name: &str) -> Result<(), PodmanError> {
        self.output(&["network", "rm", "--force", name])
            .await
            .map(drop)
    }

    /// A command for `podman logs --follow <id>`; the caller pipes its output.
    #[must_use]
    pub fn logs_follow(&self, id: &str) -> Command {
        let args = ["logs", "--follow", id];
        tracing::debug!(command = %self.line(&args), "podman");
        self.command(&args)
    }

    /// The ids of every container, running or not, with `label` (`key=value`).
    ///
    /// # Errors
    /// The call's error.
    pub async fn ls_by_label(&self, label: &str) -> Result<Vec<String>, PodmanError> {
        let filter = format!("label={label}");
        let out = self
            .output(&["ps", "--all", "--quiet", "--no-trunc", "--filter", &filter])
            .await?;
        Ok(lines(&out))
    }

    /// The names of every network with `label` (`key=value`).
    ///
    /// # Errors
    /// The call's error.
    pub async fn networks_by_label(&self, label: &str) -> Result<Vec<String>, PodmanError> {
        let filter = format!("label={label}");
        let out = self
            .output(&["network", "ls", "--quiet", "--filter", &filter])
            .await?;
        Ok(lines(&out))
    }
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_skips_blank_ones() {
        assert_eq!(lines("a\n\n b \n"), vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(lines(""), Vec::<String>::new());
    }

    #[test]
    fn exec_attaches_stdin_only_when_asked() {
        let podman = Podman::new("podman");
        let args = vec!["--version".to_owned()];
        let with = podman.exec("c1", &args, true);
        let without = podman.exec("c1", &args, false);
        let rendered = |c: &Command| {
            c.as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(rendered(&with), ["exec", "-i", "c1", "--version"]);
        assert_eq!(rendered(&without), ["exec", "c1", "--version"]);
    }
}
