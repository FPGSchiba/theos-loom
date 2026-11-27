//! The split of the tests into `test` and `slow-tests`: the nextest profiles that make it, and
//! what `slow-tests` reads from git and cargo to decide whether the slow tests run on a pull
//! request, over temporary repositories made with the `git` command line.
//!
//! No test here has a name segment that starts with `slow_`: the profiles would take it for a
//! slow test and leave it out of `test`.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::checks::slow::{self, Member};
use xtask::ci::PullRequestEvent;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The nextest filter of the slow tests: a segment of the test's name starts with `slow_`.
const SLOW: &str = "test(/(^|::)slow_/)";

fn root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?
        .to_path_buf())
}

/// The default filter of each profile of `gate/nextest.toml`, in the order of `profiles`.
fn filters(profiles: &[&str]) -> TestResult<Vec<Option<String>>> {
    let text = fs::read_to_string(root()?.join("gate").join("nextest.toml"))?;
    let config: toml::Table = text.parse()?;
    let all = config
        .get("profile")
        .and_then(toml::Value::as_table)
        .ok_or("gate/nextest.toml has no profiles")?;
    profiles
        .iter()
        .map(|profile| {
            let table = all
                .get(*profile)
                .and_then(toml::Value::as_table)
                .ok_or_else(|| format!("gate/nextest.toml has no profile {profile}"))?;
            Ok(table
                .get("default-filter")
                .and_then(toml::Value::as_str)
                .map(str::to_owned))
        })
        .collect()
}

// AC: CE-08.1
#[test]
fn every_test_runs_in_exactly_one_of_test_and_slow_tests() -> TestResult {
    // `default` is the `test` check locally and the coverage run, `ci` the `test` check in CI,
    // `slow` the `slow-tests` check everywhere. One filter and its negation: no test is in both,
    // and none is in neither.
    let not_slow = format!("not {SLOW}");
    assert_eq!(
        filters(&["default", "ci", "slow"])?,
        [
            Some(not_slow.clone()),
            Some(not_slow),
            Some(SLOW.to_owned())
        ]
    );
    Ok(())
}

/// A repository on branch `main` with a small workspace, with a helper for each thing the tests
/// do to it.
struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    /// The workspace has four members under `crates/`: `maps` has a slow test and depends on
    /// `format`; `cli` depends on `maps`; `vcs` stands alone. Plus a document outside them.
    fn new() -> TestResult<Self> {
        let repo = Self {
            dir: tempfile::tempdir()?,
        };
        repo.git(&["init", "-q", "-b", "main"])?;
        repo.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n",
        )?;
        repo.member("maps", &["format"])?;
        repo.write(
            "crates/maps/tests/runs.rs",
            "//! Map-scale runs.\n\n#[test]\nfn slow_full_run() {}\n",
        )?;
        repo.member("format", &[])?;
        repo.member("cli", &["maps"])?;
        repo.member("vcs", &[])?;
        repo.write("docs/notes.md", "# Notes\n")?;
        repo.commit("the workspace")?;
        Ok(repo)
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Line endings are left as written and every call carries an identity, because CI runners
    /// have none.
    fn git(&self, args: &[&str]) -> TestResult<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args([
                "-c",
                "core.autocrlf=false",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?.trim_end().to_owned())
    }

    fn write(&self, path: &str, text: &str) -> TestResult {
        let path = self.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
        Ok(())
    }

    /// A library member `crates/<name>` with path dependencies on the members `depends_on`.
    fn member(&self, name: &str, depends_on: &[&str]) -> TestResult {
        let dependencies: Vec<String> = depends_on
            .iter()
            .map(|dependency| format!("{dependency} = {{ path = \"../{dependency}\" }}"))
            .collect();
        self.write(
            &format!("crates/{name}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 [dependencies]\n{}\n",
                dependencies.join("\n")
            ),
        )?;
        self.write(&format!("crates/{name}/src/lib.rs"), "//! A member.\n")
    }

    /// Commits everything in the work tree and returns the new commit.
    fn commit(&self, message: &str) -> TestResult<String> {
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-q", "--allow-empty", "-m", message])?;
        self.git(&["rev-parse", "HEAD"])
    }

    /// A pull request from a new branch off `main` that writes `path`, checked out as CI has it.
    /// The text is a comment in TOML; nothing compiles the Rust files.
    fn pull_request(&self, branch: &str, path: &str) -> TestResult<PullRequestEvent> {
        self.git(&["switch", "-q", "-c", branch, "main"])?;
        self.write(path, "# changed\n")?;
        let head_sha = self.commit("the change")?;
        Ok(event(&self.git(&["rev-parse", "main"])?, &head_sha, branch))
    }
}

fn event(base_sha: &str, head_sha: &str, branch: &str) -> PullRequestEvent {
    PullRequestEvent {
        action: "synchronize".to_owned(),
        number: 1,
        title: "ci(gate): a change (gate)".to_owned(),
        body: None,
        base_sha: base_sha.to_owned(),
        head_sha: head_sha.to_owned(),
        head_ref: branch.to_owned(),
        label: None,
    }
}

#[test]
fn members_are_read_from_cargo_and_their_sources() -> TestResult {
    let repo = Repo::new()?;
    let mut members = slow::members(repo.path())?;
    members.sort_by(|a, b| a.dir.cmp(&b.dir));
    let member = |dir: &str, depends_on: &[&str], has_slow_tests| Member {
        dir: dir.to_owned(),
        depends_on: depends_on.iter().map(|dep| (*dep).to_owned()).collect(),
        has_slow_tests,
    };
    assert_eq!(
        members,
        [
            member("crates/cli", &["crates/maps"], false),
            member("crates/format", &[], false),
            member("crates/maps", &["crates/format"], true),
            member("crates/vcs", &[], false),
        ]
    );
    Ok(())
}

#[test]
fn a_pull_request_needs_the_slow_tests_when_it_can_change_them() -> TestResult {
    let repo = Repo::new()?;
    for (branch, path, needed) in [
        ("maps", "crates/maps/src/lib.rs", true),
        (
            "maps-fixture",
            "crates/maps/tests/fixtures/recipe.toml",
            true,
        ),
        ("format", "crates/format/src/lib.rs", true),
        ("config", ".config/nextest.toml", true),
        ("gate", "gate/nextest.toml", true),
        ("cli", "crates/cli/src/lib.rs", false),
        ("vcs", "crates/vcs/src/lib.rs", false),
        ("docs", "docs/notes.md", false),
        // The pull request replaces the last slow test, so it leaves none to run.
        ("maps-last", "crates/maps/tests/runs.rs", false),
    ] {
        let pull_request = repo.pull_request(branch, path)?;
        assert_eq!(
            slow::needed_by(repo.path(), &pull_request)?,
            needed,
            "a pull request that changes {path}"
        );
    }
    Ok(())
}

#[test]
fn only_the_changes_since_the_merge_base_count() -> TestResult {
    let repo = Repo::new()?;
    let pull_request = repo.pull_request("docs", "docs/notes.md")?;
    // `main` moves on with a change to `maps` after the branch left it: that change is not the
    // pull request's.
    repo.git(&["switch", "-q", "main"])?;
    repo.write("crates/maps/src/lib.rs", "//! Moved on.\n")?;
    let base_sha = repo.commit("main moves on")?;
    repo.git(&["switch", "-q", "docs"])?;
    let pull_request = event(&base_sha, &pull_request.head_sha, "docs");
    assert!(!slow::needed_by(repo.path(), &pull_request)?);
    Ok(())
}

#[test]
fn a_pull_request_without_its_head_commit_cannot_be_decided() -> TestResult {
    let repo = Repo::new()?;
    let base_sha = repo.git(&["rev-parse", "main"])?;
    let missing = "0123456789abcdef0123456789abcdef01234567";
    let result = slow::needed_by(repo.path(), &event(&base_sha, missing, "gone"));
    assert!(result.is_err(), "{result:?}");
    Ok(())
}
