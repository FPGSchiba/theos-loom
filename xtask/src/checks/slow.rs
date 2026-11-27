//! The `slow-tests` check: the tests the `test` check leaves out because they take minutes, such
//! as the parity and golden runs of the map generator (`docs/gate.md` §1).
//!
//! A test is slow when a segment of its name, the path nextest shows, starts with `slow_`:
//! `maps::parity::slow_recipes_match` or `stages::slow_runs::full_pipeline`. The nextest profiles
//! of `gate/nextest.toml` split the tests by that name: `default` and `ci` leave the slow ones
//! out, `slow` runs only them. This check is [`cargo_cmds::slow_tests`].
//!
//! On a pull request the check runs the slow tests only when the pull request can change their
//! outcome ([`needed_by`]): when it changes a file of a workspace member with slow tests, of a
//! member such a member depends on, or one of [`ALWAYS`]. Otherwise it passes with a note and
//! runs nothing, so a pull request that cannot affect them does not pay for their build. The job
//! still runs and reports, so the required status check is never missing. A push to `main`, a
//! manual or scheduled run and `cargo xtask gate` always run them.
//!
//! A member has slow tests when one of its `.rs` files contains `slow_` ([`has_slow_tests`]).
//! That over-counts (a variable `slow_path` counts too), which only makes the tests run more
//! often. A pull request that removes the last slow test of a member leaves none there to run.
//!
//! The module is not called `slow_tests`: its own tests would be `checks::slow_tests::tests::…`,
//! which the profiles take for slow tests.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::checks::cargo_cmds;
use crate::ci::{CiEvent, PullRequestEvent};
use crate::files::{extension, repo_files};
use crate::git::Git;
use crate::process::cargo_metadata;
use crate::repo_path::repo_relative;
use crate::verdict::{Check, Verdict};
use crate::{Error, Result};

/// The text that marks a member as one with slow tests: the start of a slow test's name segment.
pub const MARKER: &str = "slow_";

/// The paths outside the members whose change can change the outcome of any test: the
/// workspace manifest and lock file, the toolchain, the cargo and nextest configuration, and the
/// gate itself (its files, xtask and the workflows). An entry that ends with `/` is a folder and
/// covers everything in it; any other entry is one file.
pub const ALWAYS: [&str; 8] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".cargo/",
    ".config/",
    ".github/",
    "gate/",
    "xtask/",
];

/// The note of a pull request whose changes cannot change the outcome of a slow test.
pub const NOT_NEEDED: &str = "the slow tests did not run: the pull request changes no workspace \
                              member with slow tests, no member they depend on, and none of the \
                              workspace manifest, the lock file, the toolchain, the cargo and \
                              nextest configuration and the gate";

/// A member of the workspace, as far as the slow tests are concerned.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Member {
    /// Its folder, relative to the root of the workspace with `/`; empty for a package at the
    /// root.
    pub dir: String,
    /// The folders of the members it depends on directly, of every kind: normal, dev and build.
    pub depends_on: Vec<String>,
    /// Whether it has slow tests ([`has_slow_tests`]).
    pub has_slow_tests: bool,
}

/// Runs the slow tests in `root`, or, on a pull request that cannot change their outcome, passes
/// with the note [`NOT_NEEDED`] without running them.
///
/// `_out` is not written to: cargo's own output goes straight to the terminal and the caller
/// writes the verdict.
///
/// # Errors
///
/// Fails when git cannot read the commits of the pull request, cargo cannot give the metadata of
/// the workspace, a source file cannot be read, or cargo cannot be started. Failing tests are a
/// finding, not an error.
pub fn run(root: &Path, ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    if let Some(CiEvent::PullRequest(pull_request)) = ci
        && !needed_by(root, pull_request)?
    {
        let mut verdict = Verdict::pass(Check::SlowTests);
        verdict.notes.push(NOT_NEEDED.to_owned());
        return Ok(verdict);
    }
    cargo_cmds::run_cargo_check(root, Check::SlowTests, &cargo_cmds::slow_tests())
}

/// Whether `pull_request` can change the outcome of a slow test: whether a file it changes since
/// the merge base of its base and head commits is [`watched`] in the workspace checked out at
/// `root`.
///
/// # Errors
///
/// Fails when either commit is not in the repository, cargo cannot give the metadata, or a source
/// file cannot be read.
pub fn needed_by(root: &Path, pull_request: &PullRequestEvent) -> Result<bool> {
    let git = Git::new(root);
    let merge_base = git.merge_base(&pull_request.base_sha, &pull_request.head_sha)?;
    let changed = git.diff_name_only_no_renames(&merge_base, &pull_request.head_sha)?;
    let dirs = watched_dirs(&members(root)?);
    Ok(changed.iter().any(|path| watched(path, &dirs)))
}

/// The members of the workspace at `root`, from `cargo metadata --no-deps` and the `.rs` files of
/// each member.
///
/// # Errors
///
/// Fails when cargo cannot give the metadata, the files cannot be listed or a `.rs` file cannot
/// be read.
pub fn members(root: &Path) -> Result<Vec<Member>> {
    let metadata = cargo_metadata(root, true, true, false)?;
    let workspace = metadata.workspace_root.as_std_path();
    let packages = metadata.workspace_packages();
    let dir_of = |manifest: &Path| {
        let dir = manifest.parent().unwrap_or(manifest);
        repo_relative(workspace, &dir.to_string_lossy())
    };
    let dirs: BTreeSet<String> = packages
        .iter()
        .map(|package| dir_of(package.manifest_path.as_std_path()))
        .collect();
    let sources: Vec<String> = repo_files(workspace)?
        .into_iter()
        .filter(|path| extension(path) == Some("rs"))
        .collect();
    packages
        .iter()
        .map(|package| {
            let dir = dir_of(package.manifest_path.as_std_path());
            let depends_on = package
                .dependencies
                .iter()
                .filter_map(|dependency| dependency.path.as_ref())
                .map(|path| repo_relative(workspace, path.as_str()))
                .filter(|path| dirs.contains(path))
                .collect();
            let has_slow_tests = has_slow_tests(workspace, &dir, &sources)?;
            Ok(Member {
                dir,
                depends_on,
                has_slow_tests,
            })
        })
        .collect()
}

/// Whether one of the `.rs` files of `sources` (paths relative to `root`) that lies in `dir`
/// contains [`MARKER`].
///
/// # Errors
///
/// Fails when one of those files cannot be read.
pub fn has_slow_tests(root: &Path, dir: &str, sources: &[String]) -> Result<bool> {
    for path in sources.iter().filter(|path| inside(path, dir)) {
        let full = root.join(path);
        let bytes = fs::read(&full).map_err(|source| Error::Io { path: full, source })?;
        if String::from_utf8_lossy(&bytes).contains(MARKER) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The folders a change in which can change the outcome of a slow test: every member with slow
/// tests, and every member such a member depends on, directly or through others.
#[must_use]
pub fn watched_dirs(members: &[Member]) -> BTreeSet<String> {
    let mut watched = BTreeSet::new();
    let mut pending: Vec<&str> = members
        .iter()
        .filter(|member| member.has_slow_tests)
        .map(|member| member.dir.as_str())
        .collect();
    while let Some(dir) = pending.pop() {
        if !watched.insert(dir.to_owned()) {
            continue;
        }
        let dependencies = members
            .iter()
            .filter(|member| member.dir == dir)
            .flat_map(|member| member.depends_on.iter().map(String::as_str));
        pending.extend(dependencies);
    }
    watched
}

/// Whether changing `path` (relative to the root, with `/`) can change the outcome of a slow
/// test: it is one of [`ALWAYS`] or lies in one of `dirs`.
#[must_use]
pub fn watched(path: &str, dirs: &BTreeSet<String>) -> bool {
    let always = ALWAYS.iter().any(|entry| {
        if entry.ends_with('/') {
            path.starts_with(entry)
        } else {
            path == *entry
        }
    });
    always || dirs.iter().any(|dir| inside(path, dir))
}

/// Whether `path` lies in the folder `dir`; every path lies in the root, which is empty.
fn inside(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(dir: &str, depends_on: &[&str], has_slow_tests: bool) -> Member {
        Member {
            dir: dir.to_owned(),
            depends_on: depends_on.iter().map(|dep| (*dep).to_owned()).collect(),
            has_slow_tests,
        }
    }

    fn set(dirs: &[&str]) -> BTreeSet<String> {
        dirs.iter().map(|dir| (*dir).to_owned()).collect()
    }

    #[test]
    fn members_with_slow_tests_and_their_dependencies_are_watched() {
        let members = [
            member("crates/maps", &["crates/format"], true),
            member("crates/format", &["crates/base"], false),
            member("crates/base", &[], false),
            member("crates/cli", &["crates/maps", "crates/base"], false),
            member("crates/vcs", &[], false),
        ];
        assert_eq!(
            watched_dirs(&members),
            set(&["crates/maps", "crates/format", "crates/base"]),
            "a member that depends on one with slow tests cannot change their outcome"
        );
    }

    #[test]
    fn no_member_with_slow_tests_watches_no_folder() {
        let members = [
            member("crates/a", &["crates/b"], false),
            member("crates/b", &[], false),
        ];
        assert_eq!(watched_dirs(&members), BTreeSet::new());
    }

    #[test]
    fn a_dependency_cycle_ends() {
        let members = [
            member("crates/a", &["crates/b"], true),
            member("crates/b", &["crates/a"], false),
        ];
        assert_eq!(watched_dirs(&members), set(&["crates/a", "crates/b"]));
    }

    #[test]
    fn a_path_is_watched_in_a_watched_folder_and_nowhere_else() {
        let dirs = set(&["crates/maps"]);
        assert!(watched("crates/maps/src/lib.rs", &dirs));
        assert!(watched("crates/maps/Cargo.toml", &dirs));
        assert!(!watched("crates/maps-cli/src/lib.rs", &dirs), "a sibling");
        assert!(!watched("crates/vcs/src/lib.rs", &dirs));
        assert!(!watched("docs/gate.md", &dirs));
        assert!(
            !watched("crates/maps", &dirs),
            "a file named like the folder"
        );
    }

    #[test]
    fn the_build_inputs_and_the_gate_are_always_watched() {
        let none = BTreeSet::new();
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            ".config/nextest.toml",
            ".github/workflows/ci.yml",
            "gate/nextest.toml",
            "xtask/src/main.rs",
        ] {
            assert!(watched(path, &none), "{path}");
        }
        for path in [
            "crates/vcs/Cargo.toml",
            "docs/Cargo.toml",
            "Cargo.toml.orig",
            "gate.md",
            "README.md",
            "editor-ui/package.json",
        ] {
            assert!(!watched(path, &none), "{path}");
        }
    }

    #[test]
    fn a_package_at_the_root_watches_everything() {
        let dirs = set(&[""]);
        assert!(watched("docs/gate.md", &dirs));
    }

    #[test]
    fn the_marker_in_the_folder_of_a_member_marks_its_slow_tests() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|source| Error::Io {
            path: "tempdir".into(),
            source,
        })?;
        let write = |path: &str, text: &str| -> Result<()> {
            let full = dir.path().join(path);
            let parent = full.parent().ok_or_else(|| Error::Missing {
                what: "a parent folder".to_owned(),
            })?;
            fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
            fs::write(&full, text).map_err(|source| Error::Io { path: full, source })
        };
        write(
            "crates/maps/tests/runs.rs",
            "#[test]\nfn slow_full_run() {}\n",
        )?;
        write("crates/maps-cli/src/lib.rs", "//! fast\n")?;
        write("crates/vcs/src/lib.rs", "//! fast\n")?;
        let sources = [
            "crates/maps/tests/runs.rs",
            "crates/maps-cli/src/lib.rs",
            "crates/vcs/src/lib.rs",
        ]
        .map(str::to_owned);
        assert!(has_slow_tests(dir.path(), "crates/maps", &sources)?);
        assert!(!has_slow_tests(dir.path(), "crates/vcs", &sources)?);
        assert!(
            !has_slow_tests(dir.path(), "crates/maps-cli", &sources)?,
            "the folder of a sibling is not the member's"
        );
        assert!(
            has_slow_tests(dir.path(), "", &sources)?,
            "the root has all"
        );
        Ok(())
    }

    #[test]
    fn an_unreadable_source_is_an_error() {
        let sources = ["crates/gone/src/lib.rs".to_owned()];
        let result = has_slow_tests(Path::new("no-such-root"), "crates/gone", &sources);
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
    }
}
