//! Runs the `git` command line (T5) through `tokio::process`.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;

use thiserror::Error;
use tokio::process::Command;

/// A git command that did not run or did not succeed.
#[derive(Debug, Error)]
pub enum GitError {
    /// The `git` binary could not be started.
    #[error("could not start `{command}`: {source}")]
    Spawn {
        /// The command line, for the message.
        command: String,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// Git ran and exited with an error.
    #[error("`{command}` failed ({status}): {stderr}")]
    Failed {
        /// The command line, for the message.
        command: String,
        /// The exit status.
        status: std::process::ExitStatus,
        /// What git wrote to stderr, trimmed.
        stderr: String,
    },
}

/// Runs `git <args>` (in `cwd` when given) and returns trimmed stdout.
///
/// Prompts for credentials are off, so a missing credential fails instead of waiting.
pub(crate) async fn run(cwd: Option<&Path>, args: &[&OsStr]) -> Result<String, GitError> {
    let mut command = Command::new("git");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let line = || {
        let rendered: Vec<_> = args.iter().map(|a| a.to_string_lossy()).collect();
        format!("git {}", rendered.join(" "))
    };
    let output = command.output().await.map_err(|source| GitError::Spawn {
        command: line(),
        source,
    })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(GitError::Failed {
            command: line(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Runs git with `&str` arguments, for the common case.
pub(crate) async fn run_str(cwd: Option<&Path>, args: &[&str]) -> Result<String, GitError> {
    let os: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run(cwd, &os).await
}
