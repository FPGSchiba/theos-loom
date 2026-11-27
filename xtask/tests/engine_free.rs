//! `engine-free` over fixture workspaces (`tests/fixtures/engine-free/<case>/`).
//!
//! Every case is a workspace of its own. Its crate `a` depends on the fake engine crate
//! `theos-core` of `engine/`, which is outside every workspace, or on a member called
//! `theos-editor`. A test copies the case and `engine/` to a temporary directory, with the layout
//! they have here, and checks the copy offline: cargo writes a `Cargo.lock` there, and nothing is
//! written into the repository.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::checks::engine_free::{EngineCrates, check};
use xtask::verdict::{Check, Verdict};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("engine-free")
}

/// Copies the directory `from` to `to`, which is created.
fn copy_dir(from: &Path, to: &Path) -> TestResult {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The engine crates of the gate, as `gate/engine-crates.toml` lists them.
fn gate_engine_crates() -> TestResult<EngineCrates> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?;
    Ok(EngineCrates::load(repo)?)
}

/// The result of `engine-free` on a temporary copy of the case, offline. With `locked` the copy
/// cannot be resolved, for it has no `Cargo.lock` and cargo is not to make one.
fn check_copy(
    case: &str,
    engine: &EngineCrates,
    locked: bool,
) -> TestResult<xtask::Result<Verdict>> {
    let dir = tempfile::tempdir()?;
    copy_dir(&fixtures().join("engine"), &dir.path().join("engine"))?;
    let root = dir.path().join(case);
    copy_dir(&fixtures().join(case), &root)?;
    Ok(check(&root, engine, locked, true))
}

/// The verdict of `engine-free` on a temporary copy of the case, offline and without `--locked`.
fn check_case_with(case: &str, engine: &EngineCrates) -> TestResult<Verdict> {
    let verdict = check_copy(case, engine, false)??;
    assert_eq!(verdict.check, Check::EngineFree);
    Ok(verdict)
}

fn check_case(case: &str) -> TestResult<Verdict> {
    check_case_with(case, &gate_engine_crates()?)
}

/// The messages of the findings of a failing verdict that have all of `parts`.
fn messages_with<'a>(verdict: &'a Verdict, parts: &[&str]) -> Vec<&'a str> {
    verdict
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .filter(|message| parts.iter().all(|part| message.contains(part)))
        .collect()
}

/// A verdict that fails with a finding that has all of `parts`.
fn assert_fails_with(verdict: &Verdict, parts: &[&str]) {
    assert!(!verdict.passed(), "passed: {:?}", verdict.notes);
    assert!(
        !messages_with(verdict, parts).is_empty(),
        "no finding with {parts:?} in {:?}",
        verdict.findings
    );
}

fn assert_passes(verdict: &Verdict) {
    assert!(verdict.passed(), "{:?}", verdict.findings);
}

// AC: CE-08.2
#[test]
fn non_optional_engine_dep_fails() -> TestResult {
    let verdict = check_case("non-optional-engine-dep")?;
    assert_fails_with(&verdict, &["`theos-core`", "not optional"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn optional_dep_behind_other_feature_fails() -> TestResult {
    let verdict = check_case("optional-dep-behind-other-feature")?;
    assert_fails_with(&verdict, &["feature `render`", "`theos-core`"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn engine_reached_from_default_fails() -> TestResult {
    let verdict = check_case("engine-reached-from-default")?;
    // Both levels see it: the declared one in the features, the resolved one in the graph.
    assert_fails_with(&verdict, &["feature `default`", "feature `engine`"]);
    assert_fails_with(&verdict, &["dependency graph", "`theos-core`"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn renamed_engine_dep_fails() -> TestResult {
    let verdict = check_case("renamed-engine-dep")?;
    assert_fails_with(&verdict, &["`theos-core`", "`core`", "not optional"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn dev_dependency_on_engine_fails() -> TestResult {
    let verdict = check_case("dev-dependency-on-engine")?;
    assert_fails_with(&verdict, &["dev-dependency", "`theos-core`"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn build_dependency_on_engine_fails() -> TestResult {
    let verdict = check_case("build-dependency-on-engine")?;
    assert_fails_with(&verdict, &["build-dependency", "`theos-core`"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn target_specific_dependency_on_engine_fails() -> TestResult {
    // The condition is for Windows; the check does not run on one platform only.
    let verdict = check_case("target-dependency-on-engine")?;
    assert_fails_with(&verdict, &["`theos-core`", "not optional"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn implicit_feature_fails() -> TestResult {
    // Without a `dep:` anywhere cargo makes a feature of the dependency's name.
    let verdict = check_case("implicit-feature")?;
    assert_fails_with(&verdict, &["feature `theos-core`", "`theos-core`"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn weak_feature_on_engine_dep_fails() -> TestResult {
    let verdict = check_case("weak-feature-on-engine-dep")?;
    assert_fails_with(&verdict, &["feature `render`", "`theos-core`"]);
    // The feature `engine` itself is allowed to name it.
    assert!(
        !verdict
            .findings
            .iter()
            .any(|finding| finding.message.starts_with("the feature `engine` of")),
        "{:?}",
        verdict.findings
    );
    Ok(())
}

// AC: CE-08.2
#[test]
fn dep_behind_engine_only_passes() -> TestResult {
    assert_passes(&check_case("dep-behind-engine-only")?);
    Ok(())
}

// AC: CE-08.2
#[test]
fn own_member_named_theos_editor_passes() -> TestResult {
    // `theos-editor` is in the name list, because the engine has a crate of that name. Ours is
    // a member of the workspace and depended on by path.
    let engine = gate_engine_crates()?;
    assert!(engine.names.iter().any(|name| name == "theos-editor"));
    assert_passes(&check_case_with("own-member-named-theos-editor", &engine)?);
    Ok(())
}

// AC: CE-08.2
#[test]
fn member_with_its_engine_feature_on_must_be_optional() -> TestResult {
    let verdict = check_case("member-engine-feature-not-optional")?;
    assert_fails_with(&verdict, &["`theos-editor`", "not optional"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn member_with_its_engine_feature_on_behind_engine_passes() -> TestResult {
    assert_passes(&check_case("member-engine-feature-behind-engine")?);
    Ok(())
}

// AC: CE-08.2
#[test]
fn member_engine_feature_behind_other_feature_fails() -> TestResult {
    let verdict = check_case("member-engine-feature-behind-other-feature")?;
    assert_fails_with(&verdict, &["feature `render`", "`theos-editor`"]);
    Ok(())
}

// AC: CE-08.2
#[test]
fn path_outside_the_workspace_is_engine_without_the_name_list() -> TestResult {
    let engine = EngineCrates {
        sources: vec!["github.com/fpgschiba/theos-engine".to_owned()],
        names: Vec::new(),
    };
    let verdict = check_case_with("non-optional-engine-dep", &engine)?;
    assert_fails_with(&verdict, &["`theos-core`", "outside the workspace"]);
    // The members are ours whatever the list says.
    assert_passes(&check_case_with("own-member-named-theos-editor", &engine)?);
    Ok(())
}

// AC: CE-08.2
#[test]
fn git_dependency_on_the_engine_repo_fails_whatever_its_name() -> TestResult {
    // The URL of the dependency holds the source of the engine repository and names a repository
    // that does not exist, so the graph cannot be resolved offline, whatever cargo's git cache
    // holds on this machine. The source of the dependency is what makes it an engine crate, and
    // the name is in no list.
    let verdict = check_case("git-engine-dep")?;
    assert_fails_with(
        &verdict,
        &["`not-in-the-list`", "not optional", "from a source"],
    );
    assert_eq!(verdict.notes.len(), 1, "{:?}", verdict.notes);
    assert!(
        verdict.notes[0].contains("resolved level did not run"),
        "{:?}",
        verdict.notes
    );
    Ok(())
}

// AC: CE-08.2
#[test]
fn declared_findings_stand_when_the_workspace_cannot_be_resolved() -> TestResult {
    // A lock file that is not up to date, as in a pull request that forgot it: the manifests
    // can be read, the resolve cannot.
    let verdict = check_copy("non-optional-engine-dep", &gate_engine_crates()?, true)??;
    assert_fails_with(&verdict, &["`theos-core`", "not optional"]);
    assert!(
        verdict
            .notes
            .iter()
            .any(|note| note.contains("resolved level did not run")),
        "{:?}",
        verdict.notes
    );
    Ok(())
}

// AC: CE-08.2
#[test]
fn a_workspace_that_cannot_be_resolved_is_no_pass() -> TestResult {
    let result = check_copy("dep-behind-engine-only", &gate_engine_crates()?, true)?;
    assert!(
        matches!(result, Err(xtask::Error::Missing { .. })),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn gate_file_lists_the_engine_repo_and_its_crates() -> TestResult {
    let engine = gate_engine_crates()?;
    assert_eq!(engine.sources, ["github.com/fpgschiba/theos-engine"]);
    let mut names = engine.names.clone();
    names.sort();
    let mut expected = [
        "theos-shared",
        "theos-math",
        "theos-core",
        "theos-ecs",
        "theos-assets",
        "theos-render",
        "theos-physics",
        "theos-ai",
        "theos-magic",
        "theos-scripting",
        "theos-window",
        "theos-plugin-api",
        "theos-plugin-loader",
        "theos-launcher",
        "theos-editor",
        "theos-game",
        "theos-editor-api",
    ]
    .map(String::from);
    expected.sort();
    assert_eq!(names, expected);
    Ok(())
}
