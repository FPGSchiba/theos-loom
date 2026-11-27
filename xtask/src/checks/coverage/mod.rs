//! The `coverage` check: line coverage per crate against floors and against `main`
//! (`docs/gate.md` §1 and §3, CE-08.8).
//!
//! The check [`check`] runs `cargo llvm-cov nextest --workspace --locked --json` and reads the
//! line coverage of every crate from its export ([`llvm_json`]). Nextest's profile `default`
//! leaves out the slow tests (`gate/nextest.toml`), which take many times as long instrumented,
//! so the coverage is that of the tests of the `test` check. A crate fails when
//!
//! - it is below the floor of its class in `gate/thresholds.toml`,
//! - it has no class there (every workspace member needs one, so a new crate cannot escape the
//!   floors), or
//! - it has dropped by more than `max_drop` percentage points below the baseline.
//!
//! A crate without instrumented lines passes and is reported as such: the crates of P00 are
//! empty, and the floors apply as soon as code arrives. The numbers of every crate are in the
//! notes of the verdict.
//!
//! The baseline is the coverage of the merge base of `HEAD` and the base revision (the base of
//! the pull request, else `origin/main`), measured by [`baseline`] in a temporary work tree and
//! cached by commit, measuring toolchain and cargo-llvm-cov version. When the merge base is
//! `HEAD` itself the baseline is the head result, and when it has no `xtask/Cargo.toml` (the pull
//! request that adds the gate) there is no baseline and only the floors are checked. A push to
//! `main` stores its head result in the cache, so that the next pull requests find it.
//!
//! The pure core is [`verdict`]. The steps around it are in one function that only reaches
//! `cargo llvm-cov` through a trait, so that its tests do not run it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub mod baseline;
pub mod llvm_json;
mod thresholds;

pub use llvm_json::{CrateCoverage, parse_llvm_cov};
use thresholds::THRESHOLDS_FILE;
pub use thresholds::{CrateClass, Thresholds};

use crate::ci::CiEvent;
use crate::git::Git;
use crate::process::{
    CargoCommand, capture_cargo, capture_rustc_version, cargo_metadata, run_cargo, workspace_root,
};
use crate::tools::parse_version;
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The revision a run compares with when nothing else says.
const DEFAULT_BASE: &str = "origin/main";

/// The folder of the check below `target/`, with `head.json` and `base.json` (the exports),
/// `wt` (the work tree of the baseline), `target` (its build directory) and `cache`.
const WORK_DIR: [&str; 3] = ["target", "xtask", "coverage"];

/// Percentages are ratios of line counts and carry rounding noise, so a comparison with a limit
/// allows this much. A real difference is at least one line of tens of thousands.
const EPSILON: f64 = 1e-9;

/// Judges the coverage `head` of the crates against the floors in `thresholds` and, when there is
/// a `baseline`, against its drops. `members` are the names of the workspace members: each must
/// have a class, and one without lines in `head` has no instrumented lines.
///
/// A crate with a class and no instrumented lines passes with a note. A crate that is not in
/// `baseline` (new in the pull request) has no drop check, and without a baseline no crate has
/// one.
#[must_use]
pub fn verdict(
    head: &BTreeMap<String, CrateCoverage>,
    baseline: Option<&BTreeMap<String, CrateCoverage>>,
    thresholds: &Thresholds,
    members: &[String],
) -> Verdict {
    let mut findings = Vec::new();
    let mut notes = Vec::new();
    let names: BTreeSet<&str> = members
        .iter()
        .map(String::as_str)
        .chain(head.keys().map(String::as_str))
        .collect();
    for name in names {
        let Some(&class) = thresholds.crates.get(name) else {
            findings.push(Finding::new(format!(
                "`{name}` has no class in `{THRESHOLDS_FILE}`; list it under `[crates]` as \
                 \"library\" or \"binary\", so that it has a coverage floor"
            )));
            continue;
        };
        let now = head.get(name).copied().unwrap_or_default();
        let Some(percent) = now.percent() else {
            notes.push(format!("{name}: no instrumented lines"));
            continue;
        };
        let floor = thresholds.floor(class);
        if percent + EPSILON < floor {
            findings.push(Finding::new(format!(
                "`{name}` has {percent:.1} % line coverage, below the {floor:.1} % floor of \
                 {} crates in `{THRESHOLDS_FILE}`; add tests",
                class.name()
            )));
        }
        let before = baseline
            .and_then(|crates| crates.get(name))
            .and_then(CrateCoverage::percent);
        if let Some(before) = before {
            let drop = before - percent;
            if drop > thresholds.max_drop + EPSILON {
                findings.push(Finding::new(format!(
                    "`{name}` dropped from {before:.1} % on the baseline to {percent:.1} %, \
                     {drop:.1} points, and at most {:.1} are allowed; add tests",
                    thresholds.max_drop
                )));
            }
        }
        let against = before.map_or_else(String::new, |before| format!(", baseline {before:.1} %"));
        notes.push(format!(
            "{name}: {percent:.1} % of {} lines ({} floor {floor:.1} %{against})",
            now.count,
            class.name()
        ));
    }
    if baseline.is_none() {
        notes.push(
            "no baseline: the merge base has no xtask to measure with, so drops were not checked"
                .to_owned(),
        );
    }
    Verdict {
        check: Check::Coverage,
        findings,
        notes,
    }
}

/// What the check needs from `cargo llvm-cov`.
pub(crate) trait Llvm {
    /// The version of `cargo llvm-cov`, such as `0.9.1`.
    fn version(&self, root: &Path) -> Result<String>;

    /// Names the toolchain that measures: the lines `release: ` and `commit-hash: ` of
    /// `rustc -vV` of the rustc that belongs to the cargo that runs the gate.
    fn toolchain(&self, root: &Path) -> Result<String>;

    /// Runs the tests of the workspace in `dir` with instrumentation and writes the export to
    /// `output`. The build directory is `target_dir` when there is one. Says whether the tests
    /// passed, which they must for there to be an export.
    fn run(&self, dir: &Path, output: &Path, target_dir: Option<&Path>) -> Result<bool>;

    /// The root of the workspace in `dir` as cargo names it, the spelling the paths of the
    /// export are compared against ([`crate::process::workspace_root`]). The default is `dir`
    /// made absolute, for a fake that never runs cargo.
    fn cargo_root(&self, dir: &Path) -> Result<PathBuf> {
        std::path::absolute(dir).map_err(io_error(dir))
    }
}

/// `cargo llvm-cov` as it is installed.
struct CargoLlvm;

impl Llvm for CargoLlvm {
    fn version(&self, root: &Path) -> Result<String> {
        let command = CargoCommand {
            args: vec!["llvm-cov".to_owned(), "--version".to_owned()],
            env: Vec::new(),
            env_remove: Vec::new(),
        };
        let output = capture_cargo(root, &command)?.unwrap_or_default();
        parse_version("cargo-llvm-cov", &output).ok_or_else(|| Error::Missing {
            what: "cargo-llvm-cov; run `cargo xtask tools --install`".to_owned(),
        })
    }

    fn toolchain(&self, root: &Path) -> Result<String> {
        let output = capture_rustc_version(root)?.unwrap_or_default();
        rustc_identity(&output).ok_or_else(|| Error::Missing {
            what: "the toolchain: `rustc -vV` gave no release".to_owned(),
        })
    }

    fn run(&self, dir: &Path, output: &Path, target_dir: Option<&Path>) -> Result<bool> {
        run_cargo(dir, &llvm_cov_command(output, target_dir))
    }

    fn cargo_root(&self, dir: &Path) -> Result<PathBuf> {
        Ok(workspace_root(&cargo_metadata(dir, true, true, false)?))
    }
}

/// The lines of the output of `rustc -vV` that name a toolchain, `release: <version>` and
/// `commit-hash: <hash>` in this order, joined with a line feed. `None` when there is no release.
/// A build without a commit has only the release line.
fn rustc_identity(output: &str) -> Option<String> {
    let line = |key: &str| {
        output.lines().find_map(|line| {
            let value = line.trim().strip_prefix(key)?.trim();
            (!value.is_empty()).then(|| format!("{key} {value}"))
        })
    };
    let release = line("release:")?;
    Some(match line("commit-hash:") {
        Some(commit) => format!("{release}\n{commit}"),
        None => release,
    })
}

/// `cargo llvm-cov nextest --workspace --locked --json --output-path <output>`, building into
/// `target_dir` when there is one.
///
/// The folder goes in `CARGO_LLVM_COV_TARGET_DIR`, not `CARGO_TARGET_DIR`: cargo-llvm-cov builds
/// into the former, but the latter would also reach the tests it runs, and every `cargo` those
/// tests start would build into it. With a folder of its own, the command also drops
/// `CARGO_TARGET_DIR` and `CARGO_BUILD_TARGET_DIR` that xtask inherited, so they do not reach the
/// tests either. Without one, cargo's usual behaviour stays.
pub(crate) fn llvm_cov_command(output: &Path, target_dir: Option<&Path>) -> CargoCommand {
    let mut args: Vec<String> = [
        "llvm-cov",
        "nextest",
        "--workspace",
        "--locked",
        "--json",
        "--output-path",
    ]
    .map(str::to_owned)
    .into();
    args.push(output.to_string_lossy().into_owned());
    let env = target_dir
        .map(|dir| {
            (
                "CARGO_LLVM_COV_TARGET_DIR".to_owned(),
                dir.to_string_lossy().into_owned(),
            )
        })
        .into_iter()
        .collect();
    let env_remove = if target_dir.is_some() {
        ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
            .map(str::to_owned)
            .into()
    } else {
        Vec::new()
    };
    CargoCommand {
        args,
        env,
        env_remove,
    }
}

/// The folder of the check below `root`.
pub(crate) fn work_dir(root: &Path) -> PathBuf {
    WORK_DIR
        .iter()
        .fold(root.to_path_buf(), |dir, part| dir.join(part))
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The names of the workspace members at `root`.
fn workspace_members(root: &Path) -> Result<Vec<String>> {
    let metadata = cargo_metadata(root, true, true, false)?;
    Ok(metadata
        .workspace_packages()
        .iter()
        .map(|package| package.name.to_string())
        .collect())
}

/// Measures the head in `root` with `llvm`, the baseline at the merge base of `HEAD` and
/// `base_rev`, and judges both. With `store_head` the head result goes to the cache under the
/// commit of `HEAD`.
pub(crate) fn evaluate(
    llvm: &dyn Llvm,
    root: &Path,
    base_rev: &str,
    store_head: bool,
    members: &[String],
) -> Result<Verdict> {
    // Git and the work directory are found from an absolute root; the paths in the export are
    // absolute too, in cargo's spelling of the root.
    let root = std::path::absolute(root).map_err(io_error(root))?;
    let thresholds = Thresholds::load(&root)?;
    let git = Git::new(&root);
    let head_sha = git.rev_parse("HEAD")?;
    let base_sha = git.merge_base("HEAD", base_rev)?;

    let work = work_dir(&root);
    std::fs::create_dir_all(&work).map_err(io_error(&work))?;
    let export = work.join("head.json");
    // An export of an earlier run must not be taken for this one.
    if export.exists() {
        std::fs::remove_file(&export).map_err(io_error(&export))?;
    }
    if !llvm.run(&root, &export, None)? {
        return Ok(Verdict::with(
            Check::Coverage,
            vec![Finding::new(
                "the tests failed under instrumentation, so coverage could not be measured; \
                 `cargo xtask test` shows the failing tests and cargo's output is above",
            )],
        ));
    }
    let json = std::fs::read_to_string(&export).map_err(io_error(&export))?;
    let head = parse_llvm_cov(&json, &llvm.cargo_root(&root)?)?;

    let cache_dir = work.join("cache");
    if store_head {
        baseline::store(llvm, &root, &head_sha, &cache_dir, &head)?;
    }
    let same_commit = base_sha == head_sha;
    let baseline = if same_commit {
        Some(head.clone())
    } else {
        baseline::measure_with(llvm, &root, &base_sha, &cache_dir)?
    };
    let mut result = verdict(&head, baseline.as_ref(), &thresholds, members);
    if same_commit {
        result
            .notes
            .push("the baseline is this commit, so no drop was measured".to_owned());
    }
    Ok(result)
}

/// Checks the coverage of the workspace at `root` against the merge base of `HEAD` and
/// `base_rev`.
///
/// # Errors
///
/// Fails when the thresholds or the workspace cannot be read, git or cargo cannot run,
/// `base_rev` is unknown, the baseline's tests fail, or an export is not what cargo-llvm-cov
/// writes. Failing tests of the head are a failing verdict.
pub fn check(root: &Path, base_rev: &str) -> Result<Verdict> {
    evaluate(&CargoLlvm, root, base_rev, false, &workspace_members(root)?)
}

/// The revision to compare with: `base` when given, else the base commit of a pull request,
/// else `origin/main`.
fn base_rev(ci: Option<&CiEvent>, base: Option<&str>) -> String {
    match (base, ci) {
        (Some(base), _) => base.to_owned(),
        (None, Some(CiEvent::PullRequest(event))) => event.base_sha.clone(),
        _ => DEFAULT_BASE.to_owned(),
    }
}

/// Whether the run is for a push to `main`, whose head result later pull requests compare with.
/// `github_ref` is `$GITHUB_REF`.
fn stores_head(ci: Option<&CiEvent>, github_ref: Option<&str>) -> bool {
    matches!(ci, Some(CiEvent::Push(_))) && github_ref == Some("refs/heads/main")
}

/// The `coverage` check as `cargo xtask coverage` runs it: the base revision is the base of the
/// pull request under GitHub Actions and `origin/main` elsewhere, and a push to `main` also
/// stores its result for the pull requests to come. `_out` is not used.
///
/// # Errors
///
/// As [`check`].
pub fn run(root: &Path, ci: Option<&CiEvent>, out: &mut dyn Write) -> Result<Verdict> {
    run_with_base(root, ci, None, out)
}

/// [`run`], comparing with `base` instead when it is given.
///
/// # Errors
///
/// As [`check`].
pub fn run_with_base(
    root: &Path,
    ci: Option<&CiEvent>,
    base: Option<&str>,
    _out: &mut dyn Write,
) -> Result<Verdict> {
    let store = stores_head(ci, std::env::var("GITHUB_REF").ok().as_deref());
    evaluate(
        &CargoLlvm,
        root,
        &base_rev(ci, base),
        store,
        &workspace_members(root)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::{PullRequestEvent, PushEvent};
    use crate::git::testing::{TestResult, commit, git, init_repo};
    use serde_json::json;
    use std::cell::RefCell;
    use std::fs;

    fn cov(covered: u64, count: u64) -> CrateCoverage {
        CrateCoverage { covered, count }
    }

    /// A crate with `tenths` covered of 1000 lines, so `799` is 79.9 %.
    fn tenths(tenths: u64) -> CrateCoverage {
        cov(tenths, 1000)
    }

    fn gate() -> Thresholds {
        let class = |name: &str, class| (name.to_owned(), class);
        Thresholds {
            max_drop: 1.0,
            library: 80.0,
            binary: 60.0,
            crates: BTreeMap::from([
                class("theos-content", CrateClass::Library),
                class("theos-vcs", CrateClass::Library),
                class("theos-studio", CrateClass::Library),
                class("xtask", CrateClass::Library),
                class("theos", CrateClass::Binary),
                class("theos-mcp", CrateClass::Binary),
                class("theos-editor", CrateClass::Binary),
            ]),
        }
    }

    fn members() -> Vec<String> {
        gate().crates.keys().cloned().collect()
    }

    fn map(crates: &[(&str, CrateCoverage)]) -> BTreeMap<String, CrateCoverage> {
        crates
            .iter()
            .map(|(name, coverage)| ((*name).to_owned(), *coverage))
            .collect()
    }

    fn messages(verdict: &Verdict) -> Vec<&str> {
        verdict
            .findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect()
    }

    fn has_note(verdict: &Verdict, parts: &[&str]) -> bool {
        verdict
            .notes
            .iter()
            .any(|note| parts.iter().all(|part| note.contains(part)))
    }

    // AC: CE-08.8
    #[test]
    fn crate_below_floor_fails() {
        let head = map(&[("theos-content", tenths(799)), ("xtask", tenths(800))]);
        let result = verdict(&head, None, &gate(), &members());
        assert!(!result.passed(), "{result:?}");
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        for part in ["theos-content", "79.9", "80.0", "library"] {
            assert!(
                messages[0].contains(part),
                "{part} is not in {}",
                messages[0]
            );
        }
    }

    // AC: CE-08.8
    #[test]
    fn binary_floor_is_60() {
        let passes = verdict(&map(&[("theos", tenths(600))]), None, &gate(), &members());
        assert!(passes.passed(), "{passes:?}");
        let fails = verdict(&map(&[("theos", tenths(599))]), None, &gate(), &members());
        assert!(!fails.passed(), "{fails:?}");
        let messages = messages(&fails);
        assert_eq!(messages.len(), 1, "{messages:?}");
        for part in ["theos", "59.9", "60.0", "binary"] {
            assert!(
                messages[0].contains(part),
                "{part} is not in {}",
                messages[0]
            );
        }
    }

    // AC: CE-08.8
    #[test]
    fn drop_over_max_fails() {
        let head = map(&[("theos-content", tenths(809))]);
        let base = map(&[("theos-content", tenths(820))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(!result.passed(), "{result:?}");
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        for part in ["theos-content", "82.0", "80.9", "1.1", "1.0"] {
            assert!(
                messages[0].contains(part),
                "{part} is not in {}",
                messages[0]
            );
        }
    }

    // AC: CE-08.8
    #[test]
    fn drop_within_max_passes() {
        let head = map(&[("theos-content", tenths(810))]);
        let base = map(&[("theos-content", tenths(820))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(result.passed(), "{result:?}");
    }

    // AC: CE-08.8
    #[test]
    fn unlisted_member_fails() {
        let mut members = members();
        members.push("theos-new".to_owned());
        let result = verdict(&map(&[]), None, &gate(), &members);
        assert!(!result.passed(), "{result:?}");
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("theos-new"), "{}", messages[0]);
        assert!(
            messages[0].contains("gate/thresholds.toml"),
            "{}",
            messages[0]
        );
    }

    // AC: CE-08.8
    #[test]
    fn empty_crate_passes() {
        let head = map(&[("theos-vcs", cov(0, 0))]);
        let result = verdict(&head, None, &gate(), &members());
        assert!(result.passed(), "{result:?}");
        assert!(
            has_note(&result, &["theos-vcs", "no instrumented lines"]),
            "{result:?}"
        );
        // A crate that the export does not list at all has no instrumented lines either.
        assert!(
            has_note(&result, &["theos-studio", "no instrumented lines"]),
            "{result:?}"
        );
    }

    // AC: CE-08.8
    #[test]
    fn crate_new_in_pr_has_no_drop_check() {
        let head = map(&[("theos-vcs", tenths(805)), ("theos-content", tenths(900))]);
        let base = map(&[("theos-content", tenths(900))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(result.passed(), "{result:?}");
    }

    // AC: CE-08.8
    #[test]
    fn a_new_crate_still_has_its_floor() {
        let head = map(&[("theos-vcs", tenths(700))]);
        let base = map(&[("theos-content", tenths(900))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(!result.passed(), "{result:?}");
    }

    #[test]
    fn a_crate_that_lost_all_its_lines_passes_as_empty() {
        let head = map(&[("theos-content", cov(0, 0))]);
        let base = map(&[("theos-content", tenths(900))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(result.passed(), "{result:?}");
        assert!(has_note(
            &result,
            &["theos-content", "no instrumented lines"]
        ));
    }

    #[test]
    fn a_crate_that_is_empty_on_the_baseline_has_no_drop_check() {
        let head = map(&[("theos-content", tenths(900))]);
        let base = map(&[("theos-content", cov(0, 0))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(result.passed(), "{result:?}");
    }

    #[test]
    fn rounding_noise_does_not_decide_a_limit() {
        // 64.4 - 63.4 is 1.000000000000007 in floating point, and the drop is exactly 1.0.
        let head = map(&[("theos", tenths(634))]);
        let base = map(&[("theos", tenths(644))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(result.passed(), "{result:?}");
    }

    #[test]
    fn a_crate_that_has_lines_but_is_not_a_member_needs_a_class_too() {
        let head = map(&[("stray", tenths(900))]);
        let result = verdict(&head, None, &gate(), &members());
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("stray"), "{}", messages[0]);
    }

    #[test]
    fn the_notes_have_the_numbers_of_every_crate() {
        let head = map(&[("theos-content", tenths(851)), ("theos", cov(3, 4))]);
        let base = map(&[("theos-content", tenths(860))]);
        let result = verdict(&head, Some(&base), &gate(), &members());
        assert!(result.passed(), "{result:?}");
        assert!(has_note(
            &result,
            &[
                "theos-content",
                "85.1 %",
                "1000 lines",
                "library floor 80.0 %",
                "baseline 86.0 %"
            ]
        ));
        assert!(has_note(
            &result,
            &["theos:", "75.0 %", "4 lines", "binary floor 60.0 %"]
        ));
        assert!(!has_note(&result, &["no baseline"]), "{result:?}");
        let without = verdict(&head, None, &gate(), &members());
        assert!(has_note(&without, &["no baseline"]), "{without:?}");
    }

    const RUSTC_VV: &str = "rustc 1.99.1 (5f94df478 2026-08-27)\nbinary: rustc\n\
        commit-hash: 5f94df4780000000000000000000000000000000\ncommit-date: 2026-08-27\n\
        host: x86_64-pc-windows-msvc\nrelease: 1.99.1\nLLVM version: 21.1.0\n";

    #[test]
    fn the_toolchain_is_named_by_its_release_and_commit() {
        assert_eq!(
            rustc_identity(RUSTC_VV).as_deref(),
            Some("release: 1.99.1\ncommit-hash: 5f94df4780000000000000000000000000000000")
        );
        // The order of the lines and CRLF do not matter, and nothing else is in the name.
        let windows = "release: 1.99.1\r\nhost: x\r\ncommit-hash: abc\r\n";
        assert_eq!(
            rustc_identity(windows).as_deref(),
            Some("release: 1.99.1\ncommit-hash: abc")
        );
        // A build without a commit is named by its release.
        assert_eq!(
            rustc_identity("release: 1.99.1\ncommit-hash: unknown\n").as_deref(),
            Some("release: 1.99.1\ncommit-hash: unknown")
        );
        assert_eq!(
            rustc_identity("release: 1.99.1\n").as_deref(),
            Some("release: 1.99.1")
        );
    }

    #[test]
    fn output_without_a_release_names_no_toolchain() {
        for output in [
            "",
            "rustc 1.99.1\n",
            "commit-hash: abc\n",
            "release:\n",
            "release: \n",
        ] {
            assert_eq!(rustc_identity(output), None, "{output:?}");
        }
    }

    #[test]
    fn the_command_is_the_documented_one() {
        let output = Path::new("some").join("head.json");
        let command = llvm_cov_command(&output, None);
        assert_eq!(
            command.args,
            [
                "llvm-cov",
                "nextest",
                "--workspace",
                "--locked",
                "--json",
                "--output-path",
                &output.to_string_lossy(),
            ]
        );
        assert_eq!(command.env, Vec::<(String, String)>::new());
        assert_eq!(command.env_remove, Vec::<String>::new());
        let target = Path::new("some").join("target");
        let command = llvm_cov_command(&output, Some(&target));
        // cargo-llvm-cov builds into this one; the tests it runs never see `CARGO_TARGET_DIR`.
        assert_eq!(
            command.env,
            [(
                "CARGO_LLVM_COV_TARGET_DIR".to_owned(),
                target.to_string_lossy().into_owned()
            )]
        );
        assert_eq!(
            command.env_remove,
            ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
        );
    }

    fn pull_request(base_sha: &str) -> CiEvent {
        CiEvent::PullRequest(PullRequestEvent {
            action: "synchronize".to_owned(),
            number: 7,
            title: "t".to_owned(),
            body: None,
            base_sha: base_sha.to_owned(),
            head_sha: "head".to_owned(),
            head_ref: "phase/P00-x".to_owned(),
            label: None,
        })
    }

    fn push() -> CiEvent {
        CiEvent::Push(PushEvent {
            before: "a".to_owned(),
            after: "b".to_owned(),
        })
    }

    #[test]
    fn the_base_comes_from_the_flag_then_the_pull_request_then_origin_main() {
        let pr = pull_request("abc123");
        assert_eq!(base_rev(Some(&pr), Some("v1")), "v1");
        assert_eq!(base_rev(None, Some("v1")), "v1");
        assert_eq!(base_rev(Some(&pr), None), "abc123");
        assert_eq!(base_rev(Some(&push()), None), "origin/main");
        assert_eq!(base_rev(Some(&CiEvent::Other), None), "origin/main");
        assert_eq!(base_rev(None, None), "origin/main");
    }

    #[test]
    fn only_a_push_to_main_stores_its_result() {
        let main = Some("refs/heads/main");
        assert!(stores_head(Some(&push()), main));
        assert!(!stores_head(Some(&push()), Some("refs/heads/phase/P00-x")));
        assert!(!stores_head(Some(&push()), Some("refs/tags/main")));
        assert!(!stores_head(Some(&push()), None));
        assert!(!stores_head(Some(&pull_request("a")), main));
        assert!(!stores_head(Some(&CiEvent::Other), main));
        assert!(!stores_head(None, main));
    }

    /// A `cargo llvm-cov` that writes an export for `crates/theos-content` from the file
    /// `cov.txt` of the directory it runs in (`<covered> <count>`), and notes where it ran.
    #[derive(Default)]
    struct FakeLlvm {
        runs: RefCell<Vec<PathBuf>>,
        head_fails: bool,
        baseline_fails: bool,
        /// Writes the paths of a repository somewhere else than where it ran.
        elsewhere: bool,
        /// Spells every root as cargo does, which is not how it was given (a symlinked clone, or
        /// macOS's `/private/var` for `/var`): the export has its paths, `cargo_root` names it.
        cargo_spelling: Option<&'static str>,
    }

    fn is_worktree(dir: &Path) -> bool {
        dir.file_name().is_some_and(|name| name == "wt")
    }

    impl Llvm for FakeLlvm {
        fn version(&self, _root: &Path) -> Result<String> {
            Ok("0.9.1".to_owned())
        }

        fn toolchain(&self, _root: &Path) -> Result<String> {
            Ok("release: 1.99.1".to_owned())
        }

        fn run(&self, dir: &Path, output: &Path, target_dir: Option<&Path>) -> Result<bool> {
            self.runs.borrow_mut().push(dir.to_path_buf());
            let baseline = is_worktree(dir);
            // The baseline is built apart from the head, so that they do not share a directory.
            assert_eq!(target_dir.is_some(), baseline, "{dir:?} {target_dir:?}");
            if (baseline && self.baseline_fails) || (!baseline && self.head_fails) {
                return Ok(false);
            }
            let path = dir.join("cov.txt");
            let text = fs::read_to_string(&path).map_err(io_error(&path))?;
            let mut numbers = text.split_whitespace().map(str::parse::<u64>);
            let (Some(Ok(covered)), Some(Ok(count))) = (numbers.next(), numbers.next()) else {
                panic!("cov.txt is `<covered> <count>`");
            };
            let place = if self.elsewhere {
                "/elsewhere".to_owned()
            } else if let Some(spelling) = self.cargo_spelling {
                spelling.to_owned()
            } else {
                dir.display().to_string()
            };
            let export = json!({
                "type": "llvm.coverage.json.export",
                "data": [{ "files": [{
                    "filename": format!("{place}/crates/theos-content/src/lib.rs"),
                    "summary": { "lines": { "count": count, "covered": covered } },
                }]}],
            });
            fs::write(output, export.to_string()).map_err(io_error(output))?;
            Ok(true)
        }

        fn cargo_root(&self, dir: &Path) -> Result<PathBuf> {
            match self.cargo_spelling {
                Some(spelling) => Ok(PathBuf::from(spelling)),
                None => std::path::absolute(dir).map_err(io_error(dir)),
            }
        }
    }

    impl FakeLlvm {
        fn ran_in(&self) -> Vec<&'static str> {
            self.runs
                .borrow()
                .iter()
                .map(|dir| if is_worktree(dir) { "baseline" } else { "head" })
                .collect()
        }
    }

    /// A repository on `main` whose first commit has the thresholds and `cov.txt` with `cov`,
    /// and `xtask/Cargo.toml` when `with_xtask`.
    fn repo(cov: &str, with_xtask: bool) -> TestResult<tempfile::TempDir> {
        let dir = init_repo()?;
        let path = dir.path();
        let thresholds = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("gate")
            .join("thresholds.toml");
        fs::create_dir(path.join("gate"))?;
        fs::copy(thresholds, path.join("gate").join("thresholds.toml"))?;
        fs::write(path.join(".gitignore"), "target\n")?;
        fs::write(path.join("cov.txt"), cov)?;
        if with_xtask {
            fs::create_dir(path.join("xtask"))?;
            fs::write(path.join("xtask").join("Cargo.toml"), "[package]\n")?;
        }
        commit(path, "feat: first")?;
        Ok(dir)
    }

    fn set_cov(dir: &Path, cov: &str) -> TestResult {
        fs::write(dir.join("cov.txt"), cov)?;
        commit(dir, "feat: change coverage")?;
        Ok(())
    }

    fn the_member() -> Vec<String> {
        vec!["theos-content".to_owned()]
    }

    fn worktrees(dir: &Path) -> TestResult<usize> {
        Ok(git(dir, &["worktree", "list"])?.lines().count())
    }

    fn cache_files(dir: &Path) -> TestResult<Vec<String>> {
        let cache = work_dir(dir).join("cache");
        let mut names = Vec::new();
        if cache.exists() {
            for entry in fs::read_dir(cache)? {
                names.push(entry?.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    // AC: CE-08.8
    #[test]
    fn the_first_pull_request_with_xtask_is_checked_against_the_floors_only() -> TestResult {
        let dir = repo("90 100", false)?;
        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        fs::create_dir(dir.path().join("xtask"))?;
        fs::write(dir.path().join("xtask").join("Cargo.toml"), "[package]\n")?;
        set_cov(dir.path(), "50 100")?;
        let llvm = FakeLlvm::default();
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert_eq!(llvm.ran_in(), ["head"]);
        assert_eq!(worktrees(dir.path())?, 1);
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("below the 80.0 % floor"),
            "{}",
            messages[0]
        );
        assert!(has_note(&result, &["no baseline"]), "{result:?}");
        Ok(())
    }

    // AC: CE-08.8
    #[test]
    fn the_export_is_read_against_the_root_as_cargo_names_it() -> TestResult {
        let dir = repo("90 100", true)?;
        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        set_cov(dir.path(), "80 100")?;
        let llvm = FakeLlvm {
            cargo_spelling: Some("/cargo-spelling"),
            ..FakeLlvm::default()
        };
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert_eq!(llvm.ran_in(), ["head", "baseline"]);
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("10.0"), "{}", messages[0]);
        Ok(())
    }

    // AC: CE-08.8
    #[test]
    fn a_drop_below_the_merge_base_fails_and_leaves_no_work_tree() -> TestResult {
        let dir = repo("90 100", true)?;
        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        set_cov(dir.path(), "80 100")?;
        let llvm = FakeLlvm::default();
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert_eq!(llvm.ran_in(), ["head", "baseline"]);
        let messages = messages(&result);
        assert_eq!(messages.len(), 1, "{messages:?}");
        for part in ["theos-content", "90.0", "80.0", "10.0"] {
            assert!(
                messages[0].contains(part),
                "{part} is not in {}",
                messages[0]
            );
        }
        assert_eq!(worktrees(dir.path())?, 1);
        assert!(!work_dir(dir.path()).join("wt").exists());
        assert_eq!(cache_files(dir.path())?.len(), 1);
        Ok(())
    }

    #[test]
    fn the_baseline_is_measured_once_per_commit() -> TestResult {
        let dir = repo("90 100", true)?;
        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        set_cov(dir.path(), "89 100")?;
        let llvm = FakeLlvm::default();
        let first = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert!(first.passed(), "{first:?}");
        set_cov(dir.path(), "89 100")?;
        let second = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert!(second.passed(), "{second:?}");
        set_cov(dir.path(), "88 100")?;
        let third = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert!(!third.passed(), "{third:?}");
        assert_eq!(llvm.ran_in(), ["head", "baseline", "head", "head"]);
        Ok(())
    }

    // AC: CE-08.8
    #[test]
    fn a_branch_behind_main_is_compared_with_the_merge_base() -> TestResult {
        let dir = repo("85 100", true)?;
        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        set_cov(dir.path(), "85 100")?;
        git(dir.path(), &["switch", "-q", "main"])?;
        set_cov(dir.path(), "99 100")?;
        git(dir.path(), &["switch", "-q", "phase/P00-x"])?;
        let llvm = FakeLlvm::default();
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        // Against the tip of `main` (99 %) this would be a drop of 14 points.
        assert!(result.passed(), "{result:?}");
        assert_eq!(llvm.ran_in(), ["head", "baseline"]);
        Ok(())
    }

    #[test]
    fn the_head_is_its_own_baseline_on_the_commit_of_main() -> TestResult {
        let dir = repo("90 100", true)?;
        let llvm = FakeLlvm::default();
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert!(result.passed(), "{result:?}");
        assert_eq!(llvm.ran_in(), ["head"]);
        assert!(
            has_note(&result, &["the baseline is this commit"]),
            "{result:?}"
        );
        Ok(())
    }

    // AC: CE-08.8
    #[test]
    fn failing_tests_are_a_failing_verdict() -> TestResult {
        let dir = repo("90 100", true)?;
        let llvm = FakeLlvm {
            head_fails: true,
            ..FakeLlvm::default()
        };
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert!(!result.passed(), "{result:?}");
        assert!(messages(&result)[0].contains("failed under instrumentation"));
        Ok(())
    }

    #[test]
    fn failing_tests_on_the_baseline_are_an_error_and_the_work_tree_goes() -> TestResult {
        let dir = repo("90 100", true)?;
        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        set_cov(dir.path(), "90 100")?;
        let llvm = FakeLlvm {
            baseline_fails: true,
            ..FakeLlvm::default()
        };
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member());
        assert!(matches!(result, Err(Error::Command { .. })), "{result:?}");
        assert_eq!(worktrees(dir.path())?, 1);
        assert!(!work_dir(dir.path()).join("wt").exists());
        assert_eq!(cache_files(dir.path())?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_push_to_main_stores_its_result_for_the_next_pull_request() -> TestResult {
        let dir = repo("85 100", true)?;
        let llvm = FakeLlvm::default();
        let pushed = evaluate(&llvm, dir.path(), "origin/main", true, &the_member());
        // There is no `origin/main` in this repository, which is an error, not a verdict.
        assert!(pushed.is_err(), "{pushed:?}");
        assert_eq!(cache_files(dir.path())?, Vec::<String>::new());

        let pushed = evaluate(&llvm, dir.path(), "main", true, &the_member())?;
        assert!(pushed.passed(), "{pushed:?}");
        assert_eq!(cache_files(dir.path())?.len(), 1);

        git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
        set_cov(dir.path(), "80 100")?;
        let pull_request = evaluate(&llvm, dir.path(), "main", false, &the_member())?;
        assert!(!pull_request.passed(), "{pull_request:?}");
        // The baseline came from the cache that the push filled.
        assert_eq!(llvm.ran_in(), ["head", "head"]);
        Ok(())
    }

    #[test]
    fn an_unknown_base_is_an_error_before_anything_runs() -> TestResult {
        let dir = repo("90 100", true)?;
        let llvm = FakeLlvm::default();
        let result = evaluate(&llvm, dir.path(), "no-such-branch", false, &the_member());
        assert!(matches!(result, Err(Error::Command { .. })), "{result:?}");
        assert_eq!(llvm.ran_in(), Vec::<&str>::new());
        let flag = evaluate(&llvm, dir.path(), "--output=x", false, &the_member());
        assert!(matches!(flag, Err(Error::Parse { .. })), "{flag:?}");
        Ok(())
    }

    #[test]
    fn a_missing_thresholds_file_is_an_error_before_anything_runs() -> TestResult {
        let dir = repo("90 100", true)?;
        fs::remove_file(dir.path().join("gate").join("thresholds.toml"))?;
        let llvm = FakeLlvm::default();
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member());
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
        assert_eq!(llvm.ran_in(), Vec::<&str>::new());
        Ok(())
    }

    // AC: CE-08.8
    #[test]
    fn an_export_that_does_not_fit_the_root_is_an_error_not_a_pass() -> TestResult {
        // If the paths of the export and the root disagree (a root given through a link, a short
        // name or a substituted drive) every crate would have no instrumented lines and the
        // check would pass without having looked at anything.
        let dir = repo("90 100", true)?;
        let llvm = FakeLlvm {
            elsewhere: true,
            ..FakeLlvm::default()
        };
        let result = evaluate(&llvm, dir.path(), "main", false, &the_member());
        assert!(matches!(result, Err(Error::Parse { .. })), "{result:?}");
        assert_eq!(llvm.ran_in(), ["head"]);
        Ok(())
    }

    #[test]
    fn an_export_that_is_not_one_is_an_error() -> TestResult {
        struct Garbage;
        impl Llvm for Garbage {
            fn version(&self, _root: &Path) -> Result<String> {
                Ok("0.9.1".to_owned())
            }
            fn toolchain(&self, _root: &Path) -> Result<String> {
                Ok("release: 1.99.1".to_owned())
            }
            fn run(&self, _dir: &Path, output: &Path, _target: Option<&Path>) -> Result<bool> {
                fs::write(output, "not json").map_err(io_error(output))?;
                Ok(true)
            }
        }
        let dir = repo("90 100", true)?;
        let result = evaluate(&Garbage, dir.path(), "main", false, &the_member());
        assert!(matches!(result, Err(Error::Parse { .. })), "{result:?}");
        Ok(())
    }

    #[test]
    fn a_run_that_writes_no_export_is_an_error() -> TestResult {
        struct Silent;
        impl Llvm for Silent {
            fn version(&self, _root: &Path) -> Result<String> {
                Ok("0.9.1".to_owned())
            }
            fn toolchain(&self, _root: &Path) -> Result<String> {
                Ok("release: 1.99.1".to_owned())
            }
            fn run(&self, _dir: &Path, _output: &Path, _target: Option<&Path>) -> Result<bool> {
                Ok(true)
            }
        }
        let dir = repo("90 100", true)?;
        // An export left by an earlier run is not taken for this one.
        fs::create_dir_all(work_dir(dir.path()))?;
        fs::write(work_dir(dir.path()).join("head.json"), "{\"data\": []}")?;
        let result = evaluate(&Silent, dir.path(), "main", false, &the_member());
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
        Ok(())
    }
}
