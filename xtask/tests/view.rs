//! The workspace view over fixture workspaces (`tests/fixtures/view/<case>/`).
//!
//! Every test copies a case to a temporary directory and builds the view there offline: cargo
//! checks the copy, and nothing is written into the repository.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::view::{Artifact, Shared, TargetKind, View, ViewOptions};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("view")
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

/// A copy of the case in a temporary directory.
fn copy_case(case: &str) -> TestResult<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    copy_dir(&fixtures().join(case), dir.path())?;
    Ok(dir)
}

fn files(artifact: &Artifact) -> BTreeSet<&str> {
    artifact.files.iter().map(String::as_str).collect()
}

fn find<'a>(
    view: &'a View,
    package: &str,
    kind: TargetKind,
    test: bool,
) -> TestResult<&'a Artifact> {
    view.artifacts
        .iter()
        .find(|a| a.package == package && a.kind == kind && a.test == test)
        .ok_or_else(|| format!("no artifact {package} {kind:?} test={test}").into())
}

#[test]
fn the_view_lists_the_artifacts_of_the_members() -> TestResult {
    let dir = copy_case("plain")?;
    let view = View::build(dir.path(), ViewOptions::FIXTURE)?;

    let lib = find(&view, "a", TargetKind::Lib, false)?;
    assert_eq!(lib.manifest_dir, "crates/a");
    assert_eq!(lib.src_path, "crates/a/src/lib.rs");
    assert_eq!(
        files(lib),
        BTreeSet::from(["crates/a/src/lib.rs", "crates/a/src/sub.rs"])
    );
    find(&view, "a", TargetKind::Lib, true)?;

    let bin = find(&view, "b", TargetKind::Bin, false)?;
    assert_eq!(bin.manifest_dir, "crates/b");
    assert_eq!(files(bin), BTreeSet::from(["crates/b/src/main.rs"]));

    let test = find(&view, "a", TargetKind::Test, true)?;
    assert_eq!(test.name, "it");
    assert_eq!(files(test), BTreeSet::from(["crates/a/tests/it.rs"]));

    let build = find(&view, "a", TargetKind::BuildScript, false)?;
    assert_eq!(files(build), BTreeSet::from(["crates/a/build.rs"]));
    assert!(view.findings.is_empty(), "{:?}", view.findings);
    Ok(())
}

// AC: CE-08.8
#[cfg(unix)]
#[test]
fn a_root_reached_through_a_symlink_is_repo_relative() -> TestResult {
    // Cargo names its paths from the kernel's spelling of the working directory, which is the
    // link's target, so the view must take that spelling and not the one it was given.
    let dir = copy_case("plain")?;
    let link = tempfile::tempdir()?;
    let linked = link.path().join("clone");
    std::os::unix::fs::symlink(dir.path(), &linked)?;
    let view = View::build(&linked, ViewOptions::FIXTURE)?;
    let lib = find(&view, "a", TargetKind::Lib, false)?;
    assert_eq!(lib.manifest_dir, "crates/a");
    assert_eq!(lib.src_path, "crates/a/src/lib.rs");
    assert_eq!(
        files(lib),
        BTreeSet::from(["crates/a/src/lib.rs", "crates/a/src/sub.rs"])
    );
    assert_eq!(view.root, fs::canonicalize(dir.path())?);
    Ok(())
}

// AC: CE-08.8
#[test]
fn dep_info_lists_path_modules_includes_and_data() -> TestResult {
    let dir = copy_case("path-and-include")?;
    let view = View::build(dir.path(), ViewOptions::FIXTURE)?;
    let lib = find(&view, "a", TargetKind::Lib, false)?;
    assert_eq!(
        files(lib),
        BTreeSet::from([
            "crates/a/src/lib.rs",
            "crates/a/extra/m.rs",
            "crates/a/extra/inc.rs",
            "crates/a/data.txt",
        ])
    );
    assert!(view.source("crates/a/extra/m.rs").is_some());
    assert!(view.source("crates/a/data.txt").is_none());
    Ok(())
}

#[test]
fn test_files_are_what_test_builds_compile() -> TestResult {
    let dir = copy_case("plain")?;
    let view = View::build(dir.path(), ViewOptions::FIXTURE)?;
    let test_files = view.test_files();
    assert!(test_files.contains("crates/a/tests/it.rs"));
    assert!(test_files.contains("crates/a/src/lib.rs"));
    assert!(!test_files.contains("crates/a/build.rs"));
    let sources: Vec<&str> = view.sources().map(|s| s.path.as_str()).collect();
    assert_eq!(
        sources,
        [
            "crates/a/build.rs",
            "crates/a/src/lib.rs",
            "crates/a/src/sub.rs",
            "crates/a/tests/it.rs",
            "crates/b/src/main.rs",
        ]
    );
    Ok(())
}

#[test]
fn a_byte_order_mark_changes_nothing() -> TestResult {
    let dir = copy_case("bom")?;
    let view = View::build(dir.path(), ViewOptions::FIXTURE)?;
    let source = view
        .source("crates/a/src/lib.rs")
        .ok_or("not in the view")?;
    assert!(source.file.is_some());
    assert!(source.tokens.is_some());
    assert!(view.findings.is_empty(), "{:?}", view.findings);
    Ok(())
}

#[test]
fn a_workspace_that_does_not_check_is_an_error() -> TestResult {
    let dir = copy_case("does-not-check")?;
    let result = View::build(dir.path(), ViewOptions::FIXTURE);
    match result {
        Err(error @ xtask::Error::Command { .. }) => {
            let text = error.to_string();
            assert!(!text.contains("fn {"), "{text}");
            assert!(text.contains("exited with status"), "{text}");
            assert!(text.contains("the check failed"), "{text}");
            assert!(
                text.contains("cargo check --workspace --all-targets --locked"),
                "{text}"
            );
            Ok(())
        }
        other => Err(format!("expected a command error, got {:?}", other.map(|_| ())).into()),
    }
}

#[test]
fn shared_builds_the_view_once() -> TestResult {
    let dir = copy_case("plain")?;
    let shared = Shared::with(ViewOptions::FIXTURE);
    let first: *const View = shared.view(dir.path())?;
    fs::write(dir.path().join("crates/a/src/extra.rs"), "//! Added.\n")?;
    fs::write(
        dir.path().join("crates/a/src/lib.rs"),
        "//! Crate a.\n\npub mod sub;\npub mod extra;\n",
    )?;
    let second = shared.view(dir.path())?;
    assert!(std::ptr::eq(first, second));
    assert!(second.source("crates/a/src/extra.rs").is_none());
    Ok(())
}

#[test]
fn a_failed_build_is_not_tried_again() -> TestResult {
    let dir = copy_case("does-not-check")?;
    let shared = Shared::with(ViewOptions::FIXTURE);
    let first = shared.view(dir.path()).err().ok_or("the build passed")?;
    assert!(matches!(first, xtask::Error::Command { .. }), "{first}");
    // Were cargo run again, this valid file would make the second call succeed.
    fs::write(dir.path().join("crates/a/src/lib.rs"), "//! Fixed.\n")?;
    let second = shared.view(dir.path()).err().ok_or("the build ran again")?;
    let text = second.to_string();
    assert!(text.contains("failed earlier in this run"), "{text}");
    assert!(text.contains(&first.to_string()), "{text}");
    Ok(())
}

#[test]
fn artifacts_come_in_a_fixed_order() -> TestResult {
    let dir = copy_case("plain")?;
    let view = View::build(dir.path(), ViewOptions::FIXTURE)?;
    let order: Vec<(&str, TargetKind, &str, bool)> = view
        .artifacts
        .iter()
        .map(|a| (a.package.as_str(), a.kind, a.name.as_str(), a.test))
        .collect();
    assert!(order.windows(2).all(|pair| pair[0] <= pair[1]), "{order:?}");
    Ok(())
}

#[test]
fn a_relative_root_gives_repo_relative_paths() -> TestResult {
    // The copy goes next to the repository's target folder, so a path relative to the current
    // directory (the package's folder when tests run) reaches it on every drive.
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target");
    fs::create_dir_all(&base)?;
    let dir = tempfile::tempdir_in(&base)?;
    copy_dir(&fixtures().join("plain"), dir.path())?;
    let name = dir.path().file_name().ok_or("no name")?;
    let relative = Path::new("..").join("target").join(name);
    assert!(relative.is_relative());
    let view = View::build(&relative, ViewOptions::FIXTURE)?;
    let lib = find(&view, "a", TargetKind::Lib, false)?;
    assert_eq!(lib.manifest_dir, "crates/a");
    assert_eq!(lib.src_path, "crates/a/src/lib.rs");
    assert_eq!(
        files(lib),
        BTreeSet::from(["crates/a/src/lib.rs", "crates/a/src/sub.rs"])
    );
    Ok(())
}

/// Options for a fixture whose cargo child sees `CARGO_TARGET_DIR=<shared>`, as it does when the
/// coverage baseline runs these tests with one target directory.
fn with_shared_target_dir(shared: &Path) -> TestResult<ViewOptions> {
    let shared: &'static str =
        Box::leak(shared.to_str().ok_or("path")?.to_owned().into_boxed_str());
    let env: &'static [(&str, &str)] = Box::leak(Box::new([("CARGO_TARGET_DIR", shared)]));
    Ok(ViewOptions {
        extra_env: env,
        ..ViewOptions::FIXTURE
    })
}

// AC: CE-08.8
#[test]
fn fixtures_do_not_share_builds_through_an_inherited_target_dir() -> TestResult {
    let shared = tempfile::tempdir()?;
    let options = with_shared_target_dir(&shared.path().join("shared-target"))?;

    let plain = copy_case("plain")?;
    let view = View::build(plain.path(), options)?;
    let lib = find(&view, "a", TargetKind::Lib, false)?;
    assert_eq!(
        files(lib),
        BTreeSet::from(["crates/a/src/lib.rs", "crates/a/src/sub.rs"])
    );

    let linked = copy_case("path-and-include")?;
    let other = View::build(linked.path(), options)?;
    let other_lib = find(&other, "a", TargetKind::Lib, false)?;
    assert_ne!(files(other_lib), files(lib));
    assert!(
        other_lib.files.iter().all(|file| !file.ends_with("sub.rs")),
        "{:?}",
        other_lib.files
    );

    // Each fixture built into its own `target` folder; the shared directory was never used.
    assert!(plain.path().join("target").is_dir());
    assert!(linked.path().join("target").is_dir());
    assert!(!shared.path().join("shared-target").exists());
    Ok(())
}

// AC: CE-08.8
#[test]
fn a_failing_fixture_still_fails_with_an_inherited_target_dir() -> TestResult {
    let shared = tempfile::tempdir()?;
    let options = with_shared_target_dir(&shared.path().join("shared-target"))?;

    let good = copy_case("plain")?;
    View::build(good.path(), options)?;
    let bad = copy_case("does-not-check")?;
    let result = View::build(bad.path(), options);
    assert!(
        matches!(result, Err(xtask::Error::Command { .. })),
        "{:?}",
        result.map(|_| ())
    );
    Ok(())
}
