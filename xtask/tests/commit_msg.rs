//! `commit-msg` over temporary git repositories, made with the `git` command line.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::Path;
use std::process::{Command, Output};

use xtask::branch::parse_branch;
use xtask::checks::commit_msg::{CommitRange, check};
use xtask::git::Git;
use xtask::verdict::{Check, Verdict};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Runs `git` in `dir`. The identity is given on every call because CI runners have none, and
/// `core.autocrlf` is off so a developer's setting does not change what a commit stores.
fn git(dir: &Path, args: &[&str]) -> TestResult<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
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

fn init() -> TestResult<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    git(dir.path(), &["init", "-q", "-b", "main"])?;
    Ok(dir)
}

/// An empty commit with exactly this message; returns its id.
fn commit(dir: &Path, message: &str) -> TestResult<String> {
    git(
        dir,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "--cleanup=verbatim",
            "-m",
            message,
        ],
    )?;
    git(dir, &["rev-parse", "HEAD"])
}

fn known() -> BTreeSet<String> {
    BTreeSet::from(["CE-08.1".to_owned()])
}

/// Checks `base..head` of the repository at `dir` as commits of `branch`.
fn run_check(dir: &Path, base: &str, head: &str, branch: &str) -> TestResult<Verdict> {
    let range = CommitRange::Between {
        base: base.to_owned(),
        head: head.to_owned(),
    };
    Ok(check(
        &Git::new(dir),
        &range,
        &parse_branch(branch),
        &known(),
    )?)
}

/// A repository with a base commit on `main` and the branch `phase/P00-x` switched to.
fn phase_branch() -> TestResult<(tempfile::TempDir, String)> {
    let dir = init()?;
    let base = commit(dir.path(), "chore: base")?;
    git(dir.path(), &["switch", "-q", "-c", "phase/P00-x"])?;
    Ok((dir, base))
}

fn messages(verdict: &Verdict) -> String {
    verdict
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

// AC: CE-08.10
#[test]
fn good_range_passes() -> TestResult {
    let (dir, base) = phase_branch()?;
    let path = dir.path();
    commit(
        path,
        "feat(xtask): parse commit messages\n\nThe parser is shared with pr-title.\n\n\
         Refs: CE-08.1\nPhase: P00\nCo-Authored-By: Claude <noreply@anthropic.com>",
    )?;
    let head = commit(path, "fix(gate): keep the range open\n\nPhase: P00")?;

    let verdict = run_check(path, &base, &head, "phase/P00-x")?;
    assert_eq!(verdict.check, Check::CommitMsg);
    assert!(verdict.passed(), "{:?}", verdict.findings);
    Ok(())
}

// AC: CE-08.10
#[test]
fn bad_header_fails() -> TestResult {
    let (dir, base) = phase_branch()?;
    let bad = commit(dir.path(), "Added stuff")?;

    let verdict = run_check(dir.path(), &base, &bad, "phase/P00-x")?;
    assert!(!verdict.passed());
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    let finding = &verdict.findings[0];
    // The finding names the commit, not a place in a file.
    assert_eq!(finding.file.as_deref(), Some(&bad[..7]));
    assert_eq!(finding.line, None);
    assert!(
        finding.message.contains("type(scope)!: summary"),
        "{finding:?}"
    );
    Ok(())
}

// AC: CE-08.10
#[test]
fn unknown_type_or_scope_fails() -> TestResult {
    let (dir, base) = phase_branch()?;
    let path = dir.path();
    commit(path, "feature(xtask): add a thing\n\nPhase: P00")?;
    let head = commit(path, "feat(nope): add a thing\n\nPhase: P00")?;

    let verdict = run_check(path, &base, &head, "phase/P00-x")?;
    assert_eq!(verdict.findings.len(), 2, "{:?}", verdict.findings);
    let text = messages(&verdict);
    assert!(
        text.contains("`feature`") && text.contains("`nope`"),
        "{text}"
    );
    Ok(())
}

// AC: CE-08.10
#[test]
fn long_summary_fails() -> TestResult {
    let (dir, base) = phase_branch()?;
    let at_limit = commit(
        dir.path(),
        &format!("feat(xtask): {}\n\nPhase: P00", "a".repeat(72)),
    )?;
    assert!(run_check(dir.path(), &base, &at_limit, "phase/P00-x")?.passed());

    let over = commit(
        dir.path(),
        &format!("feat(xtask): {}\n\nPhase: P00", "a".repeat(73)),
    )?;
    let verdict = run_check(dir.path(), &at_limit, &over, "phase/P00-x")?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(messages(&verdict).contains("73"));
    Ok(())
}

// AC: CE-08.10
#[test]
fn bang_without_breaking_footer_fails() -> TestResult {
    let (dir, base) = phase_branch()?;
    let head = commit(dir.path(), "feat(cli)!: rename the flag\n\nPhase: P00")?;

    let verdict = run_check(dir.path(), &base, &head, "phase/P00-x")?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(messages(&verdict).contains("BREAKING CHANGE"));
    Ok(())
}

// AC: CE-08.10
#[test]
fn missing_phase_footer_fails_on_phase_branch() -> TestResult {
    let (dir, base) = phase_branch()?;
    let head = commit(dir.path(), "feat(xtask): parse commit messages")?;

    let verdict = run_check(dir.path(), &base, &head, "phase/P00-x")?;
    assert!(!verdict.passed());
    assert!(
        messages(&verdict).contains("Phase: P00"),
        "{:?}",
        verdict.findings
    );

    // Off a phase branch the footer is optional.
    assert!(run_check(dir.path(), &base, &head, "fix/x")?.passed());
    Ok(())
}

// AC: CE-08.10
#[test]
fn unknown_ref_fails() -> TestResult {
    let (dir, base) = phase_branch()?;
    let head = commit(
        dir.path(),
        "feat(xtask): parse commit messages\n\nRefs: CE-99.1\nPhase: P00",
    )?;

    let verdict = run_check(dir.path(), &base, &head, "phase/P00-x")?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(messages(&verdict).contains("CE-99.1"));

    let known_ref = commit(
        dir.path(),
        "feat(xtask): parse commit messages\n\nRefs: CE-08.1\nPhase: P00",
    )?;
    assert!(run_check(dir.path(), &head, &known_ref, "phase/P00-x")?.passed());
    Ok(())
}

// AC: CE-08.10
#[test]
fn merge_commit_is_skipped() -> TestResult {
    let dir = init()?;
    let path = dir.path();
    commit(path, "chore: initial")?;
    git(path, &["switch", "-q", "-c", "phase/P00-x"])?;
    commit(path, "feat(xtask): work on the branch\n\nPhase: P00")?;
    git(path, &["switch", "-q", "main"])?;
    let tip = commit(path, "chore: main moves on")?;
    // What "Update branch" does on GitHub: merge the target into the pull request branch.
    git(path, &["switch", "-q", "phase/P00-x"])?;
    git(
        path,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'main' into phase/P00-x",
            "main",
        ],
    )?;
    let head = git(path, &["rev-parse", "HEAD"])?;
    let parents = git(path, &["rev-list", "--parents", "-n", "1", &head])?;
    assert_eq!(
        parents.split(' ').count(),
        3,
        "{parents}: not a merge commit"
    );

    // The range holds the merge and the commit on the branch; only the second is judged.
    let verdict = run_check(path, &tip, &head, "phase/P00-x")?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    Ok(())
}

// AC: CE-08.10
#[test]
fn fixup_commit_fails() -> TestResult {
    let (dir, base) = phase_branch()?;
    let head = commit(dir.path(), "fixup! feat(xtask): x")?;

    let verdict = run_check(dir.path(), &base, &head, "phase/P00-x")?;
    assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    assert!(messages(&verdict).contains("fixup!"));

    let squash = commit(dir.path(), "squash! feat(xtask): x")?;
    assert!(!run_check(dir.path(), &head, &squash, "phase/P00-x")?.passed());
    Ok(())
}

// AC: CE-08.10
#[test]
fn crlf_message_from_windows_passes() -> TestResult {
    let (dir, base) = phase_branch()?;
    let head = commit(
        dir.path(),
        "feat(xtask): parse commit messages\r\n\r\nThe parser is shared.\r\n\r\n\
         Refs: CE-08.1\r\nPhase: P00",
    )?;

    let verdict = run_check(dir.path(), &base, &head, "phase/P00-x")?;
    assert!(verdict.passed(), "{:?}", verdict.findings);
    Ok(())
}

// AC: CE-08.10
#[test]
fn every_bad_commit_is_reported() -> TestResult {
    let (dir, base) = phase_branch()?;
    let path = dir.path();
    let first = commit(path, "Added stuff")?;
    commit(path, "feat(xtask): fine\n\nPhase: P00")?;
    let last = commit(path, "oops")?;

    let verdict = run_check(path, &base, &last, "phase/P00-x")?;
    let files: Vec<Option<&str>> = verdict
        .findings
        .iter()
        .map(|finding| finding.file.as_deref())
        .collect();
    assert_eq!(files, [Some(&first[..7]), Some(&last[..7])]);
    Ok(())
}

/// `cargo xtask commit-msg` with the flags that replace the event and git's defaults.
fn xtask_commit_msg(dir: &Path, extra: &[&str]) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--root")
        .arg(dir)
        .arg("commit-msg")
        .args(extra)
        // A real CI run must not make this one write annotations or a step summary.
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()?)
}

/// An umbrella spec with the criterion CE-08.1 in `dir`: the list `Refs:` footers are checked
/// against comes from there.
fn add_umbrella(dir: &Path) -> TestResult {
    let specs = dir.join("docs").join("specs");
    std::fs::create_dir_all(&specs)?;
    std::fs::write(
        specs.join("00-umbrella.md"),
        "| Phase | Name | Repo |\n|---|---|---|\n| P00 | Foundation | theos-editor |\n\n\
         ### P00 · Foundation\n- CE-08.1 First.\n",
    )?;
    Ok(())
}

// AC: CE-08.10
#[test]
fn refs_are_checked_against_the_umbrella_spec() -> TestResult {
    let (dir, base) = phase_branch()?;
    let path = dir.path();
    add_umbrella(path)?;
    let known = commit(path, "feat(xtask): fine\n\nRefs: CE-08.1\nPhase: P00")?;
    // It looks like an id, but the umbrella does not have it.
    let unknown = commit(
        path,
        "feat(xtask): fine\n\nRefs: CE-08.1, CE-99.9\nPhase: P00",
    )?;

    let pass = xtask_commit_msg(path, &["--base", &base, "--head", &known])?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");

    let fail = xtask_commit_msg(path, &["--base", &known, "--head", &unknown])?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    assert!(String::from_utf8_lossy(&fail.stdout).contains("CE-99.9"));

    // Without an umbrella spec there is no list to check against: an error, not a pass.
    std::fs::remove_dir_all(path.join("docs"))?;
    let broken = xtask_commit_msg(path, &["--base", &base, "--head", &known])?;
    assert_eq!(broken.status.code(), Some(2), "{broken:?}");
    Ok(())
}

// AC: CE-08.10
#[test]
fn command_line_flags_choose_the_range_and_the_branch() -> TestResult {
    let (dir, base) = phase_branch()?;
    let path = dir.path();
    add_umbrella(path)?;
    let good = commit(path, "feat(xtask): fine\n\nPhase: P00")?;
    let bad = commit(path, "Added stuff")?;

    let pass = xtask_commit_msg(
        path,
        &["--base", &base, "--head", &good, "--branch", "phase/P00-x"],
    )?;
    assert_eq!(pass.status.code(), Some(0), "{pass:?}");
    assert!(String::from_utf8_lossy(&pass.stdout).contains("pass  commit-msg"));

    let fail = xtask_commit_msg(path, &["--base", &base, "--head", &bad])?;
    assert_eq!(fail.status.code(), Some(1), "{fail:?}");
    let stdout = String::from_utf8_lossy(&fail.stdout);
    assert!(stdout.contains("FAIL  commit-msg"), "{stdout}");
    assert!(stdout.contains(&bad[..7]), "{stdout}");

    // The branch decides whether `Phase:` is needed, and this commit has none. Without
    // `--branch` it is the branch checked out here, `phase/P00-x`.
    let no_footer = commit(path, "feat(xtask): no footer")?;
    let range = ["--base", bad.as_str(), "--head", no_footer.as_str()];
    let default_branch = xtask_commit_msg(path, &range)?;
    assert_eq!(default_branch.status.code(), Some(1), "{default_branch:?}");
    for (branch, code) in [("phase/P00-x", 1), ("fix/x", 0), ("main", 0)] {
        let output = xtask_commit_msg(path, &[&range[..], &["--branch", branch]].concat())?;
        assert_eq!(output.status.code(), Some(code), "{branch}: {output:?}");
    }
    Ok(())
}

// AC: CE-08.10
#[test]
fn an_unreadable_range_is_exit_code_2() -> TestResult {
    let (dir, _base) = phase_branch()?;
    let output = xtask_commit_msg(
        dir.path(),
        &["--base", "no-such-revision", "--branch", "main"],
    )?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    Ok(())
}

/// What `CONTRIBUTING.md` §2 lists and `code` lacks, and the other way round, as sentences.
fn list_differences(what: &str, document: &BTreeSet<&str>, code: &[&str]) -> Vec<String> {
    let code: BTreeSet<&str> = code.iter().copied().collect();
    let mut differences = Vec::new();
    differences.extend(
        document
            .difference(&code)
            .map(|item| format!("{what} `{item}` is in CONTRIBUTING.md but not in the code")),
    );
    differences.extend(
        code.difference(document)
            .map(|item| format!("{what} `{item}` is in the code but not in CONTRIBUTING.md")),
    );
    differences
}

// AC: CE-08.10
#[test]
fn types_and_scopes_equal_contributing_md_section_2() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?;
    let text = std::fs::read_to_string(root.join("CONTRIBUTING.md"))?.replace("\r\n", "\n");
    let section = text
        .split("\n## 2. Commits")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .ok_or("CONTRIBUTING.md has no section 2")?;

    // The first column of the types table, in backticks.
    let table = section
        .split("**Types**")
        .nth(1)
        .and_then(|rest| rest.split("\n\n**").next())
        .ok_or("section 2 has no types table")?;
    let types: BTreeSet<&str> = table
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split('`').next())
        .collect();

    // Everything in backticks in the paragraph that starts with `**Scopes:**`.
    let scopes_paragraph = section
        .split("**Scopes:**")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .ok_or("section 2 has no scopes line")?;
    let scopes: BTreeSet<&str> = scopes_paragraph.split('`').skip(1).step_by(2).collect();

    assert!(
        types.contains("feat") && scopes.contains("content"),
        "not read: {section}"
    );
    assert_eq!(
        list_differences("type", &types, &xtask::conventional::TYPES),
        Vec::<String>::new()
    );
    assert_eq!(
        list_differences("scope", &scopes, &xtask::conventional::SCOPES),
        Vec::<String>::new()
    );
    Ok(())
}

// AC: CE-08.10
#[test]
fn a_type_or_scope_on_one_side_only_is_a_difference() {
    let document: BTreeSet<&str> = ["feat", "fix"].into();
    assert_eq!(
        list_differences("type", &document, &["feat", "fix"]),
        Vec::<String>::new()
    );
    let extra_in_code = list_differences("type", &document, &["feat", "fix", "style"]);
    assert_eq!(extra_in_code.len(), 1, "{extra_in_code:?}");
    assert!(extra_in_code[0].contains("`style`") && extra_in_code[0].contains("in the code"));
    let extra_in_document = list_differences("scope", &document, &["feat"]);
    assert_eq!(extra_in_document.len(), 1, "{extra_in_document:?}");
    assert!(
        extra_in_document[0].contains("`fix`") && extra_in_document[0].contains("CONTRIBUTING")
    );
}
