//! `cargo xtask pr-title` as the command line and the pull request event drive it.

use std::error::Error;
use std::path::Path;
use std::process::{Command, Output};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// `cargo xtask pr-title` in `root` with `extra` arguments, outside GitHub Actions unless `env`
/// says otherwise.
fn xtask_pr_title(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> TestResult<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("--root")
        .arg(root)
        .arg("pr-title")
        .args(extra)
        // A real CI run must not make this one write annotations or a step summary.
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .envs(env.iter().copied());
    Ok(command.output()?)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// AC: TU-00.4
#[test]
fn flags_give_the_title_and_the_branch() -> TestResult {
    let dir = tempfile::tempdir()?;
    let good = [
        "--title",
        "feat(core): records and grammars (P02)",
        "--branch",
        "phase/P02-records-and-grammars",
    ];
    let pass = xtask_pr_title(dir.path(), &good, &[])?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");
    assert!(stdout(&pass).contains("pass  pr-title"), "{pass:?}");

    let wrong_phase = [
        "--title",
        "feat(core): records and grammars (P03)",
        "--branch",
        "phase/P02-records-and-grammars",
    ];
    let fail = xtask_pr_title(dir.path(), &wrong_phase, &[])?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    let table = stdout(&fail);
    assert!(table.contains("FAIL  pr-title"), "{table}");
    assert!(table.contains("(P02)"), "{table}");
    Ok(())
}

// AC: TU-00.4
#[test]
fn without_a_branch_flag_the_checked_out_branch_is_used() -> TestResult {
    let dir = tempfile::tempdir()?;
    let init = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q", "-b", "gate/x"])
        .output()?;
    assert!(init.status.success(), "{init:?}");

    let pass = xtask_pr_title(dir.path(), &["--title", "ci(gate): x (gate)"], &[])?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");
    let fail = xtask_pr_title(dir.path(), &["--title", "ci(gate): x (docs)"], &[])?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    Ok(())
}

// AC: TU-00.4
#[test]
fn a_title_only_exists_on_a_pull_request() -> TestResult {
    let dir = tempfile::tempdir()?;
    let output = xtask_pr_title(dir.path(), &[], &[])?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(stderr(&output).contains("--title"), "{output:?}");

    // `--branch` alone would be ignored, so it is refused.
    let alone = xtask_pr_title(dir.path(), &["--branch", "fix/x"], &[])?;
    assert_eq!(alone.status.code(), Some(2), "{alone:?}");
    Ok(())
}

/// Runs the check as a `pull_request` event of GitHub Actions would, for a pull request with
/// `title` from `head_ref`.
fn run_on_event(title: &str, head_ref: &str) -> TestResult<Output> {
    let dir = tempfile::tempdir()?;
    let event = serde_json::json!({
        "action": "edited",
        "number": 7,
        "pull_request": {
            "number": 7,
            "title": title,
            "body": null,
            "base": { "ref": "main", "sha": "1" },
            "head": { "ref": head_ref, "sha": "2" },
        },
    });
    let path = dir.path().join("event.json");
    std::fs::write(&path, event.to_string())?;
    let path = path.to_str().ok_or("temp path is not UTF-8")?;
    xtask_pr_title(
        dir.path(),
        &[],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_EVENT_PATH", path),
        ],
    )
}

// AC: TU-00.4
#[test]
fn under_github_actions_the_event_gives_the_title_and_the_branch() -> TestResult {
    let pass = run_on_event("feat(core): records and grammars (P02)", "phase/P02-x")?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");

    // A release branch has no rule in Loom, so its title cannot pass.
    let release = run_on_event("chore(release): v0.1.0", "release-plz-2026-11-01")?;
    assert_eq!(release.status.code(), Some(1), "{release:?}");

    let fail = run_on_event("records and grammars", "phase/P02-x")?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    // The finding reaches the pull request as an annotation.
    let table = stdout(&fail);
    assert!(table.contains("::error title=pr-title::"), "{table}");
    Ok(())
}
