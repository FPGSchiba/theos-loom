//! How `cargo xtask` tells a person that a check could not run: one line on stderr, exit code 2.

use std::error::Error;
use std::process::{Command, Output};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Runs xtask outside GitHub Actions with `args`, unless `configure` says otherwise.
fn xtask(args: &[&str], configure: impl FnOnce(&mut Command)) -> TestResult<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .args(args)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_EVENT_PATH");
    configure(&mut command);
    Ok(command.output()?)
}

#[test]
fn an_io_error_says_what_the_system_said_once() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("no-such-root");
    let root = root.to_str().ok_or("temporary path is not UTF-8")?;
    let output = xtask(&["--root", root, "tools", "--print-install-spec"], |_| {})?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains("tools.toml"), "{stderr}");
    // The message of `Error::Io` has the text of the operating system in it, and the source
    // of the error is that same text, so a printout of the whole chain says it twice.
    assert_eq!(stderr.matches("(os error").count(), 1, "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    Ok(())
}

#[test]
fn an_error_with_a_context_keeps_the_context_and_the_cause() -> TestResult {
    let output = xtask(&["lints"], |command| {
        command.env("GITHUB_ACTIONS", "true");
    })?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(
        stderr.trim_end(),
        "error: cannot read the GitHub Actions event: missing environment variable \
         GITHUB_EVENT_NAME under GitHub Actions"
    );
    Ok(())
}

// AC: CE-08.4
#[test]
fn no_line_of_an_error_can_start_a_workflow_command() -> TestResult {
    // A revision name is text the caller controls, and git's complaint repeats it. The branch is
    // named so that the run reaches git whatever is checked out: CI checks out a detached HEAD.
    let output = xtask(
        &[
            "commit-msg",
            "--branch",
            "phase/P00-foundation-and-gate",
            "--base",
            "no-such\n::error::forged",
        ],
        |_| {},
    )?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("forged"), "{stderr}");
    assert!(stderr.starts_with("error: "), "{stderr}");
    for line in stderr.lines() {
        assert!(!line.starts_with("::"), "{stderr}");
    }
    Ok(())
}
