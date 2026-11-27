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

// AC: TU-00.3
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

// AC: TU-00.3
#[test]
fn only_with_skip_is_refused_by_the_command_line() -> TestResult {
    let output = xtask(&["gate", "--only", "fmt", "--skip", "test"], |_| {})?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

// AC: TU-00.3
#[test]
fn a_name_that_is_not_a_gate_part_exits_with_2_and_names_it() -> TestResult {
    let output = xtask(&["gate", "--only", "tset"], |_| {})?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stdout.clone())? + &String::from_utf8(output.stderr)?;
    assert!(stderr.contains("tset"), "{stderr}");
    assert!(!stderr.contains("==> "), "no part ran: {stderr}");
    Ok(())
}

/// The stdout of a run that must have passed.
fn stdout_of(args: &[&str]) -> TestResult<String> {
    let output = xtask(args, |_| {})?;
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    Ok(String::from_utf8(output.stdout)?)
}

// AC: TU-00.3
#[test]
fn the_install_spec_can_be_asked_for_the_tools_of_some_parts() -> TestResult {
    let pins = xtask::tools::load_pins(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("no root")?,
    )?;
    let version = |name: &str| -> TestResult<String> {
        Ok(pins
            .iter()
            .find(|pin| pin.name == name)
            .ok_or("no such pin")?
            .version
            .clone())
    };
    // A part that needs a cargo tool installs that one; gitleaks, Node and pnpm never go in.
    let expected = format!("cargo-deny@{}", version("cargo-deny")?);
    let spec = stdout_of(&["tools", "--print-install-spec", "--for", "deny,secrets,ui"])?;
    assert_eq!(spec.trim(), expected);
    let spec = stdout_of(&["tools", "--print-install-spec", "--for", "fmt,clippy"])?;
    assert_eq!(spec.trim(), "");
    let output = xtask(&["tools", "--print-install-spec", "--for", "tset"], |_| {})?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

// AC: TU-00.3
#[test]
fn one_pin_can_be_printed_for_the_workflows() -> TestResult {
    let pin = stdout_of(&["tools", "--print-pin", "gitleaks"])?;
    let pins = xtask::tools::load_pins(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("no root")?,
    )?;
    let expected = pins
        .iter()
        .find(|pin| pin.name == "gitleaks")
        .ok_or("no gitleaks pin")?;
    assert_eq!(pin.trim(), expected.version);
    let output = xtask(&["tools", "--print-pin", "nothing"], |_| {})?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}
