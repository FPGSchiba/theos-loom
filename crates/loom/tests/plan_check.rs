//! Behaviour tests of `loom plan check` and `loom plan schemas`, through the built binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use loom_plan::Rule;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The fixtures the library crate owns.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../loom-plan/tests/fixtures")
}

fn loom(args: &[&str], path: &Path) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .arg(path)
        .output()
}

fn subdirs(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// The rule a tree name asserts: the longest rule name the folder starts with, then `-` or the end.
fn expected_rule(name: &str) -> Option<Rule> {
    Rule::ALL
        .into_iter()
        .filter(|rule| {
            name.strip_prefix(rule.name())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        })
        .max_by_key(|rule| rule.name().len())
}

// AC: TU-01.2
#[test]
fn check_exits_0_on_the_valid_tree() -> TestResult {
    let out = loom(&["plan", "check"], &fixtures().join("valid"))?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let cases = subdirs(&fixtures().join("cases"))?;
    assert!(!cases.is_empty(), "no case trees found");
    for case in cases {
        // This case lives in `cases/` but has one error by design (a story ready by its own mark
        // in a draft epic); `check_completeness.rs` asserts it. It must exit 1, not 0.
        let want = i32::from(case.ends_with("ready-story-in-draft-epic"));
        let out = loom(&["plan", "check"], &case)?;
        assert_eq!(
            out.status.code(),
            Some(want),
            "{}: {}",
            case.display(),
            String::from_utf8_lossy(&out.stdout)
        );
    }
    Ok(())
}

// AC: TU-01.2
#[test]
fn check_exits_1_on_every_invalid_tree() -> TestResult {
    let trees = subdirs(&fixtures().join("invalid"))?;
    assert!(
        trees.len() >= 30,
        "only {} invalid trees found",
        trees.len()
    );
    for tree in trees {
        let name = tree
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let rule = expected_rule(name).ok_or_else(|| format!("{name} names no rule"))?;
        let out = loom(&["plan", "check"], &tree)?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.status.code(), Some(1), "{name}: {stdout}");
        assert!(
            stdout.contains(&format!("error[{rule}]")),
            "{name}: {stdout}"
        );
    }
    Ok(())
}

// AC: TU-01.2
#[test]
fn check_exits_2_on_a_path_that_is_not_a_directory() -> TestResult {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("file.txt");
    fs::write(&file, "x")?;
    for path in [file, dir.path().join("missing")] {
        let out = loom(&["plan", "check"], &path)?;
        assert_eq!(out.status.code(), Some(2), "{}", path.display());
        assert!(!out.stderr.is_empty(), "{}", path.display());
    }
    Ok(())
}

// AC: TU-01.4
#[test]
fn schemas_command_writes_every_schema() -> TestResult {
    let dir = tempfile::tempdir()?;
    let out = loom(&["plan", "schemas"], dir.path())?;
    assert_eq!(out.status.code(), Some(0));
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../loom-plan/schemas");
    let mut names = Vec::new();
    for entry in fs::read_dir(&shipped)? {
        let name = entry?.file_name();
        assert_eq!(
            fs::read_to_string(dir.path().join(&name))?,
            fs::read_to_string(shipped.join(&name))?,
            "{name:?}"
        );
        names.push(name);
    }
    assert!(names.len() >= 10, "only {} schemas shipped", names.len());
    assert_eq!(fs::read_dir(dir.path())?.count(), names.len());
    Ok(())
}
