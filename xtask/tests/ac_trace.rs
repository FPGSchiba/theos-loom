//! `ac-trace` over fixture repositories (`tests/fixtures/ac-trace/<case>/`), each with an excerpt
//! of the umbrella spec in the format of the real one: P90 is built in `theos-editor` and has the
//! criteria TU-90.1 and TU-90.2, P91 is built in `FPGSchiba/bmad-method`.
//!
//! Each fixture is a one-crate workspace, copied to a temporary directory first, where `cargo
//! check` builds the view; in this repository `ac-trace` leaves everything under
//! `tests/fixtures/` alone.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use xtask::branch::Phase;
use xtask::checks::ac_trace;
use xtask::verdict::{Check, Verdict};
use xtask::view::{Shared, ViewOptions};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Copies the directory `from` into the existing directory `to`.
fn copy_dir(from: &Path, to: &Path) -> TestResult {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target)?;
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A temporary copy of the fixture `name`.
fn copy_fixture(name: &str) -> TestResult<tempfile::TempDir> {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ac-trace")
        .join(name);
    let dir = tempfile::tempdir()?;
    copy_dir(&from, dir.path())?;
    Ok(dir)
}

/// The verdict and the table of `ac-trace` for `phase` on a copy of the fixture `name`.
fn trace(name: &str, phase: Option<&str>) -> TestResult<(Verdict, String)> {
    let dir = copy_fixture(name)?;
    let phase = phase.map(str::parse::<Phase>).transpose()?;
    let mut out = Vec::new();
    let shared = Shared::with(ViewOptions::FIXTURE);
    let verdict = ac_trace::run_phase(dir.path(), phase, false, &shared, &mut out)?;
    Ok((verdict, String::from_utf8(out)?))
}

// AC: TU-00.4
#[test]
fn phase_ac_without_tag_fails() -> TestResult {
    let (verdict, table) = trace("phase-ac-without-tag", Some("P90"))?;
    assert_eq!(verdict.check, Check::AcTrace);
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(verdict.findings[0].message.contains("TU-90.2"));
    assert!(!verdict.findings[0].message.contains("TU-90.1"));
    assert!(
        table.contains("TU-90.1   src/lib.rs:3 proves_the_first\n"),
        "{table}"
    );
    assert!(table.contains("TU-90.2   no counting test\n"), "{table}");

    // On a branch without a phase nothing is demanded.
    let (verdict, _) = trace("phase-ac-without-tag", None)?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    Ok(())
}

// AC: TU-00.4
#[test]
fn unknown_id_fails() -> TestResult {
    for phase in [Some("P90"), None] {
        let (verdict, _) = trace("unknown-id", phase)?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        let finding = &verdict.findings[0];
        assert!(finding.message.contains("TU-99.9"), "{finding:?}");
        assert_eq!(finding.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(finding.line, Some(11));
    }
    Ok(())
}

// AC: TU-00.4
#[test]
fn ignored_test_does_not_prove_an_ac() -> TestResult {
    let (verdict, table) = trace("ignored-test", Some("P90"))?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(verdict.findings[0].message.contains("TU-90.2"));
    assert!(
        table.contains("TU-90.2   src/lib.rs:7 would_prove_the_second (does not count)\n"),
        "{table}"
    );
    Ok(())
}

// AC: TU-00.4
#[test]
fn dangling_tag_fails() -> TestResult {
    for phase in [Some("P90"), None] {
        let (verdict, _) = trace("dangling-tag", phase)?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        let finding = &verdict.findings[0];
        assert_eq!(finding.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(finding.line, Some(11));
    }
    Ok(())
}

// AC: TU-00.4
#[test]
fn complete_phase_passes() -> TestResult {
    let (verdict, table) = trace("complete-phase", Some("P90"))?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    assert!(
        table.contains("TU-90.1   src/lib.rs:3 proves_the_first\n"),
        "{table}"
    );
    assert!(
        table.contains("TU-90.2   e2e/records.spec.ts:4 shows a record\n"),
        "{table}"
    );
    // The file inside `tests/fixtures/` has a dangling tag and an unknown id, and is not read.
    assert!(!table.contains("not-scanned"), "{table}");
    Ok(())
}

// AC: TU-00.4
#[test]
fn other_repo_phase_demands_nothing() -> TestResult {
    let (verdict, _) = trace("other-repo-phase", Some("P91"))?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    assert_eq!(verdict.notes.len(), 1, "{:?}", verdict.notes);
    assert!(verdict.notes[0].contains("FPGSchiba/bmad-method"));
    Ok(())
}

// AC: TU-00.4
#[test]
fn a_tag_in_a_file_no_target_compiles_does_not_count() -> TestResult {
    let (verdict, table) = trace("uncompiled-file", Some("P90"))?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(verdict.findings[0].message.contains("TU-90.1"));
    assert!(verdict.findings[0].message.contains("no test that counts"));
    assert!(
        table.contains(
            "TU-90.1   no counting test
"
        ),
        "{table}"
    );
    assert!(!table.contains("orphan.rs"), "{table}");
    Ok(())
}

// AC: TU-00.4
#[test]
fn a_tag_above_a_test_in_an_inline_module_counts() -> TestResult {
    let (verdict, table) = trace("tag-in-inline-module", Some("P90"))?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    assert!(
        table.contains(
            "TU-90.1   src/lib.rs:5 proves_the_first
"
        ),
        "{table}"
    );
    Ok(())
}

// AC: TU-00.4
#[test]
fn a_test_in_a_module_that_no_build_compiles_does_not_prove_an_ac() -> TestResult {
    let (verdict, table) = trace("disabled-test-module", Some("P90"))?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(verdict.findings[0].message.contains("TU-90.2"));
    assert!(
        table.contains("TU-90.2   src/lib.rs:12 would_prove_the_second (does not count)\n"),
        "{table}"
    );
    assert!(
        table.contains("TU-90.1   src/lib.rs:5 proves_the_first\n"),
        "{table}"
    );
    Ok(())
}

// AC: TU-00.4
#[test]
fn a_test_in_a_file_that_no_build_runs_does_not_prove_an_ac() -> TestResult {
    let (verdict, table) = trace("disabled-test-file", Some("P90"))?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(verdict.findings[0].message.contains("TU-90.2"));
    assert!(
        table.contains("TU-90.2   tests/never.rs:4 would_prove_the_second (does not count)\n"),
        "{table}"
    );
    assert!(
        table.contains("TU-90.1   src/lib.rs:5 proves_the_first\n"),
        "{table}"
    );
    Ok(())
}

// AC: TU-00.4
#[test]
fn a_tag_inside_a_string_is_not_a_tag() -> TestResult {
    let (verdict, table) = trace("string-looks-like-a-tag", Some("P90"))?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    assert!(!table.contains("TU-99.9"), "{table}");
    Ok(())
}

#[test]
fn a_file_of_the_view_that_does_not_parse_is_a_finding() -> TestResult {
    let (verdict, _) = trace("unparseable-file", None)?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    let finding = &verdict.findings[0];
    assert_eq!(finding.file.as_deref(), Some("src/data.rs"));
    assert_eq!(finding.line, Some(1));
    Ok(())
}

#[test]
fn research_folders_are_scanned() -> TestResult {
    // Without a phase the table lists every criterion that has a tag, wherever the test is.
    let (verdict, table) = trace("complete-phase", None)?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    assert!(
        table.contains("TU-91.1   research/prose/prototype.spec.ts:3 loads the corpus\n"),
        "{table}"
    );
    Ok(())
}

/// `cargo xtask ac-trace` in `root` with `extra` arguments, outside GitHub Actions.
fn xtask_ac_trace(root: &Path, extra: &[&str]) -> TestResult<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("--root")
        .arg(root)
        .arg("ac-trace")
        .args(extra)
        // A real CI run must not make this one write annotations or a step summary.
        .env_remove("GITHUB_ACTIONS")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("GITHUB_STEP_SUMMARY");
    Ok(command.output()?)
}

// AC: TU-00.4
#[test]
fn the_command_line_takes_the_phase() -> TestResult {
    let missing = copy_fixture("phase-ac-without-tag")?;
    let output = xtask_ac_trace(missing.path(), &["--phase", "P90"])?;
    let table = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(table.contains("FAIL  ac-trace"), "{table}");
    assert!(table.contains("TU-90.2"), "{table}");

    let complete = copy_fixture("complete-phase")?;
    let output = xtask_ac_trace(complete.path(), &["--phase", "P90"])?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("pass  ac-trace"));

    // A phase the umbrella does not have is not a pass.
    let output = xtask_ac_trace(complete.path(), &["--phase", "P77"])?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");

    // Not a phase at all: the command line is wrong.
    let output = xtask_ac_trace(complete.path(), &["--phase", "90"])?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--phase"));
    Ok(())
}

#[test]
fn the_command_without_a_phase_needs_a_branch() -> TestResult {
    // The copy is not a git repository, so there is no branch to take the phase from.
    let dir = copy_fixture("complete-phase")?;
    let output = xtask_ac_trace(dir.path(), &[])?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    Ok(())
}
