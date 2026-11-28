//! `coverage` on a synthetic llvm-cov export (`tests/fixtures/coverage/`), on the thresholds of
//! the repository and on the workspace, and the command line in front of it.
//!
//! None of these runs `cargo llvm-cov`: that is what `cargo xtask coverage` does, and it runs the
//! tests, these included.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cargo_metadata::MetadataCommand;
use xtask::checks::coverage::{CrateClass, CrateCoverage, Thresholds, parse_llvm_cov, verdict};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The synthetic export, whose paths are for a repository at `/work/repo`.
fn fixture() -> TestResult<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("coverage")
        .join("llvm-cov.json");
    Ok(fs::read_to_string(path)?)
}

fn repo_root() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?
        .to_path_buf())
}

fn cov(covered: u64, count: u64) -> CrateCoverage {
    CrateCoverage { covered, count }
}

#[test]
fn parses_llvm_cov_fixture() -> TestResult {
    let crates = parse_llvm_cov(&fixture()?, Path::new("/work/repo"))?;
    // Lines are summed over the files of a crate, `\` counts like `/`, and what is not a source
    // of the repository (tests, fixtures, build scripts, other repositories, dependencies) is
    // left out: `loom-store` and its `build.rs` are not there at all.
    assert_eq!(
        crates,
        BTreeMap::from([
            ("loom".to_owned(), cov(3, 5)),
            ("loom-core".to_owned(), cov(76, 100)),
            ("loom-plan".to_owned(), cov(0, 0)),
            ("xtask".to_owned(), cov(91, 100)),
        ])
    );
    assert_eq!(crates["loom-core"].percent(), Some(76.0));
    assert_eq!(crates["loom"].percent(), Some(60.0));
    assert_eq!(crates["loom-plan"].percent(), None);
    Ok(())
}

#[test]
fn another_root_does_not_fit_the_fixture_and_is_an_error() -> TestResult {
    // Not an empty map: that would let every crate pass as having no lines.
    for root in [
        "/work/other",
        "/work",
        "/work/repo/crates",
        "/work/repo-other/x",
    ] {
        let result = parse_llvm_cov(&fixture()?, Path::new(root));
        assert!(
            matches!(&result, Err(xtask::Error::Parse { .. })),
            "{root}: {result:?}"
        );
    }
    Ok(())
}

#[test]
fn the_thresholds_of_the_gate_are_the_decided_ones() -> TestResult {
    let thresholds = Thresholds::load(&repo_root()?)?;
    assert_eq!(thresholds.max_drop, 1.0);
    assert_eq!(thresholds.library, 80.0);
    assert_eq!(thresholds.binary, 60.0);
    let classes: Vec<(&str, CrateClass)> = thresholds
        .crates
        .iter()
        .map(|(name, class)| (name.as_str(), *class))
        .collect();
    assert_eq!(
        classes,
        [
            ("loom", CrateClass::Binary),
            ("loom-api", CrateClass::Library),
            ("loom-context", CrateClass::Library),
            ("loom-core", CrateClass::Library),
            ("loom-gate", CrateClass::Library),
            ("loom-plan", CrateClass::Library),
            ("loom-run", CrateClass::Library),
            ("loom-store", CrateClass::Library),
            ("xtask", CrateClass::Library),
        ]
    );
    Ok(())
}

#[test]
fn every_workspace_member_has_a_class() -> TestResult {
    let metadata = MetadataCommand::new().no_deps().exec()?;
    let mut members: Vec<String> = metadata
        .workspace_packages()
        .iter()
        .map(|package| package.name.to_string())
        .collect();
    members.sort();
    let thresholds = Thresholds::load(&repo_root()?)?;
    let classed: Vec<String> = thresholds.crates.keys().cloned().collect();
    assert_eq!(members, classed);
    Ok(())
}

#[test]
fn the_fixture_is_judged_against_the_thresholds_of_the_gate() -> TestResult {
    let thresholds = Thresholds::load(&repo_root()?)?;
    let members: Vec<String> = thresholds.crates.keys().cloned().collect();
    let head = parse_llvm_cov(&fixture()?, Path::new("/work/repo"))?;

    let result = verdict(&head, None, &thresholds, &members);
    // `loom-core` has 76 % of a library; `loom` has exactly its floor of 60 %.
    assert!(!result.passed(), "{result:?}");
    assert_eq!(result.findings.len(), 1, "{result:?}");
    assert!(result.findings[0].message.contains("loom-core"));
    assert!(result.findings[0].message.contains("76.0"));
    // `loom-plan` has a file without lines and `loom-store` and `loom-run` none at all.
    for name in ["loom-plan", "loom-store", "loom-run"] {
        let note = format!("{name}: no instrumented lines");
        assert!(
            result.notes.contains(&note),
            "{note} is not in {:?}",
            result.notes
        );
    }

    // Against a baseline `loom` is within the allowed drop, `xtask` is not.
    let baseline = BTreeMap::from([
        ("loom".to_owned(), cov(61, 100)),
        ("xtask".to_owned(), cov(93, 100)),
        ("loom-core".to_owned(), cov(76, 100)),
    ]);
    let result = verdict(&head, Some(&baseline), &thresholds, &members);
    let messages: Vec<&str> = result
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("loom-core"), "{}", messages[0]);
    assert!(messages[1].contains("xtask"), "{}", messages[1]);
    assert!(messages[1].contains("93.0"), "{}", messages[1]);
    assert!(messages[1].contains("91.0"), "{}", messages[1]);
    Ok(())
}

/// `cargo xtask coverage <extra>` on the repository, without a GitHub Actions environment.
fn xtask_coverage(extra: &[&str]) -> TestResult<std::process::Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--root")
        .arg(repo_root()?)
        .arg("coverage")
        .args(extra)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()?)
}

#[test]
fn the_command_takes_a_base_and_stops_at_one_that_does_not_exist() -> TestResult {
    let output = xtask_coverage(&["--base", "no-such-revision"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Exit code 2: the check could not run. It fails before it runs a single test.
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("no-such-revision"), "{stderr}");
    assert!(!stderr.contains("not built yet"), "{stderr}");
    Ok(())
}

#[test]
fn a_base_cannot_be_read_as_a_flag_by_git() -> TestResult {
    let output = xtask_coverage(&["--base=--output=planted.txt"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("starts with `-`"), "{stderr}");
    assert!(!repo_root()?.join("planted.txt").exists());
    Ok(())
}
