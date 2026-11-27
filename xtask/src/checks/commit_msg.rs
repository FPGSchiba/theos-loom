//! The `commit-msg` check: every commit of a pull request is a Conventional Commit
//! (`CONTRIBUTING.md` §2, `docs/gate.md` §1).
//!
//! The core is [`check`]: a range of commits, the branch they are on and the known acceptance
//! criteria in, a verdict out. [`run`] gathers those from the GitHub event or from git.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::branch::{BranchKind, parse_branch};
use crate::ci::CiEvent;
use crate::conventional::{header_problems, message_problems, parse_message};
use crate::git::Git;
use crate::umbrella;
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The commits to check.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CommitRange {
    /// The commits reachable from `head` and not from `base` (`git rev-list base..head`).
    Between {
        /// The revision the range starts after; its own commit is not checked.
        base: String,
        /// The last commit checked.
        head: String,
    },
}

/// Checks every commit of `range` against the rules of `CONTRIBUTING.md` §2.
///
/// # Errors
///
/// Fails when git cannot list the range or read a commit.
pub fn check(
    git: &Git,
    range: &CommitRange,
    branch: &BranchKind,
    known_acs: &BTreeSet<String>,
) -> Result<Verdict> {
    let CommitRange::Between { base, head } = range;
    if base.is_empty() || head.is_empty() {
        // `..head` would silently mean `HEAD..head`.
        return Err(Error::Missing {
            what: "both ends of the commit range".to_owned(),
        });
    }
    let mut commits = git.rev_list(&format!("{base}..{head}"))?;
    // `rev_list` is newest first; people read a branch from its first commit.
    commits.reverse();
    let mut verdict = Verdict::pass(Check::CommitMsg);
    if commits.is_empty() {
        verdict
            .notes
            .push("there are no commits to check".to_owned());
    }
    for sha in &commits {
        // A merge commit is not written by a person: "Update branch" makes them, and squash
        // merging keeps them off `main`.
        if git.parents(sha)?.len() >= 2 {
            continue;
        }
        let file = sha.get(..SHORT_SHA).unwrap_or(sha).to_owned();
        for problem in problems(&git.message(sha)?, branch, known_acs) {
            verdict.findings.push(Finding {
                file: Some(file.clone()),
                line: None,
                message: problem,
            });
        }
    }
    Ok(verdict)
}

/// How many characters of a commit id name it in a finding.
const SHORT_SHA: usize = 7;

/// Everything wrong with the commit message `text` on `branch`; one sentence for each problem.
fn problems(text: &str, branch: &BranchKind, known_acs: &BTreeSet<String>) -> Vec<String> {
    match parse_message(text) {
        Err(problem) => vec![problem],
        Ok(message) => {
            let mut all = header_problems(&message.header);
            all.extend(message_problems(&message, branch, known_acs));
            all
        }
    }
}

/// Runs the check in `root`, taking the range and the branch from the GitHub event, or from git
/// when there is none.
///
/// A pull request is checked from the base to the head of the event, with the head's branch
/// name. A push is checked from the tip before to the tip after, and a push that creates a branch
/// passes with a note, because there is no earlier tip. Otherwise the range starts at the merge
/// base with `origin/main`.
///
/// # Errors
///
/// Fails when git cannot run, `origin/main` is unknown, or `HEAD` is detached outside a pull
/// request.
pub fn run(root: &Path, ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    let git = Git::new(root);
    let range = match ci {
        Some(CiEvent::PullRequest(pull_request)) => CommitRange::Between {
            base: pull_request.base_sha.clone(),
            head: pull_request.head_sha.clone(),
        },
        Some(CiEvent::Push(push)) if is_all_zeros(&push.before) => {
            let mut verdict = Verdict::pass(Check::CommitMsg);
            verdict.notes.push(
                "the push created a branch, so there is no earlier tip to start from and no \
                 commits were checked"
                    .to_owned(),
            );
            return Ok(verdict);
        }
        Some(CiEvent::Push(push)) => CommitRange::Between {
            base: push.before.clone(),
            head: push.after.clone(),
        },
        Some(CiEvent::Other) | None => local_range(&git, None, None)?,
    };
    let branch = match ci {
        Some(CiEvent::PullRequest(pull_request)) => parse_branch(&pull_request.head_ref),
        _ => branch_kind(&git, None)?,
    };
    check(&git, &range, &branch, &known_acs(root)?)
}

/// What git writes for "no commit" in the `before` of a push that creates a branch.
fn is_all_zeros(sha: &str) -> bool {
    !sha.is_empty() && sha.chars().all(|c| c == '0')
}

/// The range checked on a developer's machine: from `base` (default: the merge base of `head` and
/// `origin/main`) to `head` (default: `HEAD`).
///
/// # Errors
///
/// Fails when no `base` is given and git cannot find the merge base.
pub fn local_range(git: &Git, base: Option<&str>, head: Option<&str>) -> Result<CommitRange> {
    let head = head.unwrap_or("HEAD");
    let base = match base {
        Some(base) => base.to_owned(),
        None => git.merge_base(head, "origin/main")?,
    };
    Ok(CommitRange::Between {
        base,
        head: head.to_owned(),
    })
}

/// The kind of the branch `name`, or of the branch `HEAD` is on when there is no name.
///
/// # Errors
///
/// Fails when there is no name and `HEAD` is detached.
pub fn branch_kind(git: &Git, name: Option<&str>) -> Result<BranchKind> {
    if let Some(name) = name {
        return Ok(parse_branch(name));
    }
    git.current_branch()?
        .map(|name| parse_branch(&name))
        .ok_or_else(|| Error::Missing {
            what: "the branch name: HEAD is detached, so pass `--branch`".to_owned(),
        })
}

/// The acceptance criteria `Refs:` footers may name: every id of the umbrella spec of the
/// repository at `root`.
///
/// The list is never empty. A repository without criteria is an error, so a `Refs:` entry is
/// always checked against the real ids and never only for its shape.
///
/// # Errors
///
/// Fails when the umbrella spec cannot be read or parsed, or lists no criteria.
pub fn known_acs(root: &Path) -> Result<BTreeSet<String>> {
    Ok(umbrella::load(root)?.all_ids())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::{PullRequestEvent, PushEvent};
    use crate::git::testing::{TestResult, commit, git, init_repo};

    fn pull_request(base: &str, head: &str, head_ref: &str) -> CiEvent {
        CiEvent::PullRequest(PullRequestEvent {
            action: "synchronize".to_owned(),
            number: 1,
            title: "feat(xtask): a title (P00)".to_owned(),
            body: None,
            base_sha: base.to_owned(),
            head_sha: head.to_owned(),
            head_ref: head_ref.to_owned(),
            label: None,
        })
    }

    fn between(base: &str, head: &str) -> CommitRange {
        CommitRange::Between {
            base: base.to_owned(),
            head: head.to_owned(),
        }
    }

    fn short(sha: &str) -> String {
        sha[..7].to_owned()
    }

    fn run_in(root: &Path, ci: Option<&CiEvent>) -> Result<Verdict> {
        run(root, ci, &mut Vec::new())
    }

    /// A new repository with an umbrella spec, which `run` reads the known criteria from.
    fn repo_with_umbrella() -> TestResult<tempfile::TempDir> {
        let dir = init_repo()?;
        let specs = dir.path().join("docs").join("specs");
        std::fs::create_dir_all(&specs)?;
        std::fs::write(
            specs.join("00-umbrella.md"),
            "| Phase | Name | Repo |\n|---|---|---|\n| P00 | Foundation | theos-editor |\n\n\
             ### P00 · Foundation\n- CE-08.10 First.\n",
        )?;
        Ok(dir)
    }

    // AC: CE-08.10
    #[test]
    fn findings_name_the_commit_by_its_short_sha_and_come_oldest_first() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let base = commit(path, "Not even checked")?;
        let first = commit(path, "Added stuff")?;
        let second = commit(path, "feat(nope): Add x.")?;
        let good = commit(path, "fix(xtask): fine")?;

        let verdict = check(
            &Git::new(path),
            &between(&base, &good),
            &parse_branch("main"),
            &BTreeSet::new(),
        )?;
        assert_eq!(verdict.check, Check::CommitMsg);
        let places: Vec<(Option<String>, Option<u32>)> = verdict
            .findings
            .iter()
            .map(|finding| (finding.file.clone(), finding.line))
            .collect();
        // One problem in the first commit (its header does not parse), three in the second.
        let expected = [
            (Some(short(&first)), None),
            (Some(short(&second)), None),
            (Some(short(&second)), None),
            (Some(short(&second)), None),
        ];
        assert_eq!(places, expected, "{:?}", verdict.findings);
        Ok(())
    }

    #[test]
    fn an_empty_range_passes_with_a_note() -> TestResult {
        let dir = init_repo()?;
        let tip = commit(dir.path(), "chore: tip")?;
        let verdict = check(
            &Git::new(dir.path()),
            &between(&tip, &tip),
            &parse_branch("main"),
            &BTreeSet::new(),
        )?;
        assert!(verdict.passed());
        assert_eq!(verdict.notes.len(), 1, "{:?}", verdict.notes);
        Ok(())
    }

    #[test]
    fn a_range_with_a_missing_end_is_an_error_not_the_wrong_range() -> TestResult {
        let dir = init_repo()?;
        let tip = commit(dir.path(), "chore: tip")?;
        for (base, head) in [("", tip.as_str()), (tip.as_str(), "")] {
            let result = check(
                &Git::new(dir.path()),
                &between(base, head),
                &parse_branch("main"),
                &BTreeSet::new(),
            );
            assert!(matches!(result, Err(Error::Missing { .. })), "{result:?}");
        }
        Ok(())
    }

    // AC: CE-08.10
    #[test]
    fn a_pull_request_event_gives_the_range_and_the_branch() -> TestResult {
        let dir = repo_with_umbrella()?;
        let path = dir.path();
        let base = commit(path, "chore: base")?;
        let head = commit(path, "feat(xtask): no phase footer")?;

        // The branch is the head of the pull request, not the branch checked out here.
        let on_phase = pull_request(&base, &head, "phase/P00-x");
        let verdict = run_in(path, Some(&on_phase))?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        assert!(verdict.findings[0].message.contains("Phase: P00"));

        let on_fix = pull_request(&base, &head, "fix/x");
        assert!(run_in(path, Some(&on_fix))?.passed());
        Ok(())
    }

    // AC: CE-08.10
    #[test]
    fn a_push_event_gives_before_to_after() -> TestResult {
        let dir = repo_with_umbrella()?;
        let path = dir.path();
        let before = commit(path, "Not checked")?;
        let after = commit(path, "Added stuff")?;
        let push = CiEvent::Push(PushEvent {
            before: before.clone(),
            after,
        });
        let verdict = run_in(path, Some(&push))?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);

        let nothing_new = CiEvent::Push(PushEvent {
            before: before.clone(),
            after: before,
        });
        assert!(run_in(path, Some(&nothing_new))?.passed());
        Ok(())
    }

    // AC: CE-08.10
    #[test]
    fn a_push_that_creates_a_branch_passes_with_a_note() -> TestResult {
        // No repository is needed: there is nothing to compare with.
        let dir = tempfile::tempdir()?;
        let push = CiEvent::Push(PushEvent {
            before: "0".repeat(40),
            after: "a".repeat(40),
        });
        let verdict = run_in(dir.path(), Some(&push))?;
        assert!(verdict.passed());
        assert_eq!(verdict.notes.len(), 1, "{:?}", verdict.notes);
        Ok(())
    }

    // AC: CE-08.10
    #[test]
    fn locally_the_range_starts_at_the_merge_base_with_origin_main() -> TestResult {
        let dir = repo_with_umbrella()?;
        let path = dir.path();
        let initial = commit(path, "Not checked, it is on origin/main")?;
        git(path, &["update-ref", "refs/remotes/origin/main", &initial])?;
        git(path, &["switch", "-q", "-c", "phase/P00-x"])?;
        commit(path, "feat(xtask): good\n\nPhase: P00")?;
        assert!(run_in(path, None)?.passed());

        commit(path, "feat(xtask): forgot the footer")?;
        let verdict = run_in(path, None)?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        assert!(verdict.findings[0].message.contains("Phase: P00"));

        // Any other event is read like a local run.
        assert!(!run_in(path, Some(&CiEvent::Other))?.passed());
        Ok(())
    }

    #[test]
    fn locally_a_detached_head_or_a_missing_origin_main_is_an_error() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let initial = commit(path, "chore: initial")?;
        let result = run_in(path, None);
        assert!(matches!(result, Err(Error::Command { .. })), "{result:?}");

        git(path, &["update-ref", "refs/remotes/origin/main", &initial])?;
        git(path, &["switch", "-q", "--detach", &initial])?;
        let result = run_in(path, None);
        assert!(
            matches!(&result, Err(Error::Missing { what }) if what.contains("--branch")),
            "{result:?}"
        );
        Ok(())
    }

    #[test]
    fn flags_replace_the_defaults() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let initial = commit(path, "chore: initial")?;
        git(path, &["update-ref", "refs/remotes/origin/main", &initial])?;
        let repo = Git::new(path);

        assert_eq!(local_range(&repo, None, None)?, between(&initial, "HEAD"));
        assert_eq!(local_range(&repo, Some("a"), Some("b"))?, between("a", "b"));
        assert_eq!(local_range(&repo, Some("a"), None)?, between("a", "HEAD"));
        assert_eq!(
            local_range(&repo, None, Some("origin/main"))?,
            between(&initial, "origin/main")
        );

        assert_eq!(branch_kind(&repo, None)?, parse_branch("main"));
        assert_eq!(
            branch_kind(&repo, Some("phase/P00-x"))?,
            parse_branch("phase/P00-x")
        );
        Ok(())
    }

    // AC: CE-08.10
    #[test]
    fn known_acs_are_the_ids_of_the_umbrella_spec() -> TestResult {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let known = known_acs(&root)?;
        for id in ["CE-08.1", "CE-08.10", "CE-08R.1", "CE-06.1"] {
            assert!(known.contains(id), "{id}");
        }
        assert!(!known.contains("CE-99.9"));
        // The work outside the build has ids that are not criteria.
        assert!(!known.contains("CE-02"));
        Ok(())
    }

    #[test]
    fn known_acs_need_an_umbrella_spec_with_criteria() -> TestResult {
        let dir = tempfile::tempdir()?;
        let result = known_acs(dir.path());
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");

        let specs = dir.path().join("docs").join("specs");
        std::fs::create_dir_all(&specs)?;
        std::fs::write(
            specs.join("00-umbrella.md"),
            "| Phase | Name | Repo |\n|---|---|---|\n| P00 | A | theos-editor |\n",
        )?;
        let result = known_acs(dir.path());
        assert!(matches!(result, Err(Error::Config { .. })), "{result:?}");
        Ok(())
    }
}
