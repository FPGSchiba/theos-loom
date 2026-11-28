//! The paths a run or a pull request changes: the one reader `approval` and the gate's docs-only
//! rule share (P00 spec §5.3), through the `git` command line.
//!
//! - A pull request in CI: the paths between the merge base of the event's base and head, and
//!   the head, with renames as a deletion and an addition ([`of_pull_request`]).
//! - A local run: the current branch, and every path that differs from the merge base with
//!   `origin/main`: committed, staged, unstaged and untracked but not ignored. A local run
//!   judges the work tree, so uncommitted code counts.
//! - A push, another event, or a detached `HEAD`: no rule.
//!
//! Paths are repo-relative with `/` on every OS, as git writes them.

use std::path::Path;

use crate::ci::{CiEvent, PullRequestEvent};
use crate::git::Git;
use crate::{Error, Result};

/// What a run changes, as far as it can be told.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChangedPaths {
    /// The head branch and the changed paths, sorted, without duplicates.
    Known {
        /// The branch the work is on: the pull request's head branch, or the current branch.
        branch: String,
        /// The changed paths.
        paths: Vec<String>,
    },
    /// This kind of run has no rule: a push, another event, a detached `HEAD`.
    NoRule {
        /// Why.
        why: String,
    },
    /// The paths cannot be listed: a missing commit, git failing.
    Unreadable {
        /// What went wrong.
        why: String,
    },
}

/// The changed paths of the run at `root`, for the event `ci` (`None` for a local run).
#[must_use]
pub fn of_run(root: &Path, ci: Option<&CiEvent>) -> ChangedPaths {
    let git = Git::new(root);
    match ci {
        Some(CiEvent::PullRequest(pr)) => match of_pull_request(&git, pr) {
            Ok(paths) => ChangedPaths::Known {
                branch: pr.head_ref.clone(),
                paths,
            },
            Err(error) => unreadable(&error),
        },
        Some(CiEvent::Push(_) | CiEvent::Other) => ChangedPaths::NoRule {
            why: "this event is not a pull request".to_owned(),
        },
        None => local(&git),
    }
}

/// The paths that differ between the merge base of the pull request's base and head, and its head.
/// Fetches the head of the pull request when the checkout lacks it (the `approval` job checks out
/// the base commit).
///
/// # Errors
///
/// Fails when a commit is missing or git fails.
pub fn of_pull_request(git: &Git, pr: &PullRequestEvent) -> Result<Vec<String>> {
    let head = resolve_head(git, pr)?;
    let merge_base = git.merge_base(&pr.base_sha, &head)?;
    git.diff_name_only_no_renames(&merge_base, &head)
}

/// The full id of the head commit of `pr`, fetching `refs/pull/<n>/head` when the repository
/// lacks it. Resolved to the commit id, it is also hex all through, which the comment of
/// `approval` relies on.
///
/// # Errors
///
/// Fails when the commit is neither here nor fetchable.
pub(crate) fn resolve_head(git: &Git, pr: &PullRequestEvent) -> Result<String> {
    // `^{commit}` makes git look the object up: a bare id is accepted whether or not it exists.
    let commit = format!("{}^{{commit}}", pr.head_sha);
    match git.rev_parse(&commit) {
        Err(Error::Command { .. }) => {
            git.fetch_pull_head(pr.number)?;
            git.rev_parse(&commit)
        }
        found => found,
    }
}

fn local(git: &Git) -> ChangedPaths {
    let branch = match git.current_branch() {
        Ok(Some(branch)) => branch,
        Ok(None) => {
            return ChangedPaths::NoRule {
                why: "HEAD is detached".to_owned(),
            };
        }
        Err(error) => return unreadable(&error),
    };
    let listed = git
        .merge_base("origin/main", "HEAD")
        .and_then(|base| {
            let mut paths = git.diff_name_only_worktree(&base)?;
            paths.extend(git.untracked()?);
            Ok(paths)
        })
        .map(|mut paths| {
            paths.sort();
            paths.dedup();
            paths
        });
    match listed {
        Ok(paths) => ChangedPaths::Known { branch, paths },
        Err(error) => unreadable(&error),
    }
}

fn unreadable(error: &Error) -> ChangedPaths {
    ChangedPaths::Unreadable {
        why: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::{TestResult, commit, git, init_repo};
    use std::fs;

    const NO_SUCH: &str = "0123456789012345678901234567890123456789";

    /// A repository on `main` with the file `crates/a.rs` committed, and a bare `origin` that
    /// has `main`.
    fn repo_with_origin() -> TestResult<(tempfile::TempDir, tempfile::TempDir)> {
        let dir = init_repo()?;
        fs::create_dir(dir.path().join("crates"))?;
        fs::write(dir.path().join("crates").join("a.rs"), "a\n")?;
        commit(dir.path(), "feat: first")?;
        let origin = tempfile::tempdir()?;
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"])?;
        let url = origin.path().to_str().ok_or("path is not UTF-8")?;
        git(dir.path(), &["remote", "add", "origin", url])?;
        git(dir.path(), &["push", "-q", "origin", "main"])?;
        git(dir.path(), &["fetch", "-q", "origin"])?;
        Ok((dir, origin))
    }

    fn pull_request(base: &str, head: &str) -> PullRequestEvent {
        PullRequestEvent {
            action: "synchronize".to_owned(),
            number: 7,
            title: "feat: x".to_owned(),
            body: None,
            base_sha: base.to_owned(),
            head_sha: head.to_owned(),
            head_ref: "docs/x".to_owned(),
            label: None,
        }
    }

    fn known(branch: &str, paths: &[&str]) -> ChangedPaths {
        ChangedPaths::Known {
            branch: branch.to_owned(),
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    // AC: TU-00.3
    #[test]
    fn a_pull_request_lists_the_paths_since_the_merge_base() -> TestResult {
        let (dir, _origin) = repo_with_origin()?;
        let path = dir.path();
        git(path, &["switch", "-q", "-c", "docs/x"])?;
        fs::create_dir(path.join("docs"))?;
        fs::write(path.join("docs").join("a.md"), "a\n")?;
        let head = commit(path, "docs: a")?;
        git(path, &["switch", "-q", "main"])?;
        fs::write(path.join("crates").join("later.rs"), "later\n")?;
        let base = commit(path, "feat: main moves on")?;

        let event = CiEvent::PullRequest(pull_request(&base, &head));
        assert_eq!(of_run(path, Some(&event)), known("docs/x", &["docs/a.md"]));
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn a_rename_out_of_crates_is_a_deletion_and_an_addition() -> TestResult {
        let (dir, _origin) = repo_with_origin()?;
        let path = dir.path();
        let base = git(path, &["rev-parse", "HEAD"])?;
        git(path, &["switch", "-q", "-c", "docs/x"])?;
        fs::create_dir(path.join("docs"))?;
        fs::rename(
            path.join("crates").join("a.rs"),
            path.join("docs").join("a.rs"),
        )?;
        let head = commit(path, "docs: move a")?;

        let event = CiEvent::PullRequest(pull_request(&base, &head));
        let changed = of_run(path, Some(&event));
        assert_eq!(changed, known("docs/x", &["crates/a.rs", "docs/a.rs"]));
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn locally_uncommitted_and_untracked_files_count() -> TestResult {
        let (dir, _origin) = repo_with_origin()?;
        let path = dir.path();
        git(path, &["switch", "-q", "-c", "docs/x"])?;
        fs::write(path.join("crates").join("a.rs"), "changed\n")?;
        fs::write(path.join("crates").join("b.rs"), "new\n")?;

        let changed = of_run(path, None);
        assert_eq!(changed, known("docs/x", &["crates/a.rs", "crates/b.rs"]));
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn a_missing_base_commit_is_unreadable() -> TestResult {
        let (dir, _origin) = repo_with_origin()?;
        let head = git(dir.path(), &["rev-parse", "HEAD"])?;
        let event = CiEvent::PullRequest(pull_request(NO_SUCH, &head));
        let changed = of_run(dir.path(), Some(&event));
        assert!(
            matches!(changed, ChangedPaths::Unreadable { .. }),
            "{changed:?}"
        );
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn a_push_or_a_detached_head_has_no_rule() -> TestResult {
        let (dir, _origin) = repo_with_origin()?;
        let path = dir.path();
        let tip = git(path, &["rev-parse", "HEAD"])?;
        let push = CiEvent::Push(crate::ci::PushEvent {
            before: NO_SUCH.to_owned(),
            after: tip.clone(),
        });
        for event in [push, CiEvent::Other] {
            let changed = of_run(path, Some(&event));
            assert!(
                matches!(changed, ChangedPaths::NoRule { .. }),
                "{changed:?}"
            );
        }
        git(path, &["switch", "-q", "--detach", &tip])?;
        let changed = of_run(path, None);
        assert!(
            matches!(changed, ChangedPaths::NoRule { .. }),
            "{changed:?}"
        );
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn changed_paths_use_slashes() -> TestResult {
        let (dir, _origin) = repo_with_origin()?;
        let path = dir.path();
        git(path, &["switch", "-q", "-c", "docs/x"])?;
        fs::create_dir_all(path.join("a").join("b"))?;
        fs::write(path.join("a").join("b").join("c.rs"), "c\n")?;
        assert_eq!(of_run(path, None), known("docs/x", &["a/b/c.rs"]));
        commit(path, "feat: c")?;
        assert_eq!(of_run(path, None), known("docs/x", &["a/b/c.rs"]));
        Ok(())
    }
}
