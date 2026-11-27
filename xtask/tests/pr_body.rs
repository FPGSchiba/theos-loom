//! `cargo xtask pr-body` as the command line and the pull request event drive it.

use std::error::Error;
use std::path::Path;
use std::process::{Command, Output};

use xtask::checks::pr_body::template_sections;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The template of this repository, which the check reads from `--root`.
const TEMPLATE: &str = include_str!("../../.github/pull_request_template.md");

/// The sections of the template.
const HEADINGS: [&str; 7] = [
    "Phase",
    "Acceptance criteria",
    "Final review",
    "Rulings",
    "Gate",
    "Protected paths",
    "Trial",
];

#[test]
fn the_template_has_every_section() {
    assert_eq!(template_sections(TEMPLATE), HEADINGS);
}

/// A description with every section but `left_out`, each with some text under it.
fn body_without(left_out: Option<&str>) -> String {
    HEADINGS
        .iter()
        .filter(|heading| Some(**heading) != left_out)
        .map(|heading| format!("## {heading}\n\nDone.\n\n"))
        .collect::<Vec<_>>()
        .concat()
}

/// A directory that has the template where the check looks for it.
fn root_with_template() -> TestResult<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join(".github"))?;
    std::fs::write(
        dir.path().join(".github/pull_request_template.md"),
        TEMPLATE,
    )?;
    Ok(dir)
}

/// `cargo xtask pr-body` in `root` with `extra` arguments, outside GitHub Actions unless `env`
/// says otherwise.
fn xtask_pr_body(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> TestResult<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("--root")
        .arg(root)
        .arg("pr-body")
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

#[test]
fn a_body_file_gives_the_description() -> TestResult {
    let root = root_with_template()?;
    let body_file = root.path().join("body.md");
    let body_file = body_file.to_str().ok_or("temp path is not UTF-8")?;

    std::fs::write(body_file, body_without(None))?;
    let pass = xtask_pr_body(root.path(), &["--body-file", body_file], &[])?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");
    assert!(stdout(&pass).contains("pass  pr-body"), "{pass:?}");

    std::fs::write(body_file, body_without(Some("Gate")))?;
    let fail = xtask_pr_body(root.path(), &["--body-file", body_file], &[])?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    let table = stdout(&fail);
    assert!(table.contains("FAIL  pr-body"), "{table}");
    assert!(table.contains("## Gate"), "{table}");
    Ok(())
}

#[test]
fn a_description_only_exists_on_a_pull_request() -> TestResult {
    let root = root_with_template()?;
    let output = xtask_pr_body(root.path(), &[], &[])?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(stderr(&output).contains("--body-file"), "{output:?}");

    let missing = root.path().join("nowhere.md");
    let missing = missing.to_str().ok_or("temp path is not UTF-8")?;
    let unreadable = xtask_pr_body(root.path(), &["--body-file", missing], &[])?;
    assert_eq!(unreadable.status.code(), Some(2), "{unreadable:?}");
    Ok(())
}

/// Runs the check as a `pull_request` event of GitHub Actions would, for a pull request with
/// `body` (`null` for an empty one).
fn run_on_event(body: Option<&str>) -> TestResult<Output> {
    let root = root_with_template()?;
    let event = serde_json::json!({
        "action": "edited",
        "number": 7,
        "pull_request": {
            "number": 7,
            "title": "feat(xtask): x (P00)",
            "body": body,
            "base": { "ref": "main", "sha": "1" },
            "head": { "ref": "phase/P00-x", "sha": "2" },
        },
    });
    let path = root.path().join("event.json");
    std::fs::write(&path, event.to_string())?;
    let path = path.to_str().ok_or("temp path is not UTF-8")?;
    xtask_pr_body(
        root.path(),
        &[],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_EVENT_PATH", path),
        ],
    )
}

#[test]
fn under_github_actions_the_event_gives_the_description() -> TestResult {
    let pass = run_on_event(Some(&body_without(None)))?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");

    let fail = run_on_event(Some(&body_without(Some("Gate"))))?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    // The finding reaches the pull request as an annotation.
    let table = stdout(&fail);
    assert!(table.contains("::error title=pr-body::"), "{table}");

    // An empty description is every section missing.
    let empty = run_on_event(None)?;
    assert_eq!(empty.status.code(), Some(1), "{empty:?}");
    assert_eq!(
        stdout(&empty).matches("::error title=pr-body::").count(),
        HEADINGS.len()
    );
    Ok(())
}
