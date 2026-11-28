//! The `secrets` part: no commit of the change holds a secret (umbrella R10, the repository is
//! public).
//!
//! gitleaks does the scanning (directive 2). It scans the commits of a range, not the working
//! tree, so the verdict depends only on the commits and local and CI agree on the same commit.
//! [`range`] decides the range, [`command`] builds the gitleaks arguments, [`findings`] reads its
//! JSON report, and [`run`] ties them together. A finding names the rule, the file, the line and
//! the commit, never the secret.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::ci::CiEvent;
use crate::git::Git;
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The commits gitleaks scans.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Range {
    /// The commits reachable from `to` and not from `from` (`git log from..to`).
    Commits {
        /// The revision the range starts after; its own commit is not scanned.
        from: String,
        /// The last commit scanned.
        to: String,
    },
    /// Every commit of the repository.
    History,
}

/// How many characters of a commit id name it in a finding.
const SHORT_SHA: usize = 7;

/// Where the report goes, relative to the repository root.
const REPORT: [&str; 3] = ["target", "xtask", "gitleaks.json"];

/// The range to scan.
///
/// A pull request scans from the merge base of its base and head to its head. A push scans from
/// the tip before to the tip after, and the whole history when it created the branch. Any other
/// event scans the whole history. Without an event, the range is the merge base with
/// `origin/main` to `HEAD`, and a detached `HEAD` scans the whole history.
///
/// # Errors
///
/// Fails when git cannot find the merge base.
pub fn range(git: &Git, ci: Option<&CiEvent>) -> Result<Range> {
    match ci {
        Some(CiEvent::PullRequest(pull_request)) => Ok(Range::Commits {
            from: git.merge_base(&pull_request.base_sha, &pull_request.head_sha)?,
            to: pull_request.head_sha.clone(),
        }),
        Some(CiEvent::Push(push)) if is_all_zeros(&push.before) => Ok(Range::History),
        Some(CiEvent::Push(push)) => Ok(Range::Commits {
            from: push.before.clone(),
            to: push.after.clone(),
        }),
        Some(CiEvent::Other) => Ok(Range::History),
        None if git.current_branch()?.is_none() => Ok(Range::History),
        None => Ok(Range::Commits {
            from: git.merge_base("HEAD", "origin/main")?,
            to: "HEAD".to_owned(),
        }),
    }
}

/// What git writes for "no commit" in the `before` of a push that creates a branch.
fn is_all_zeros(sha: &str) -> bool {
    !sha.is_empty() && sha.chars().all(|c| c == '0')
}

/// The arguments of gitleaks (after the program name) to scan `range` and write a redacted JSON
/// report to `report`.
///
/// `--ignore-gitleaks-allow` keeps a `gitleaks:allow` comment from hiding a finding (decision
/// 0007); the exceptions live in the protected `.gitleaksignore`.
///
/// The exit code is 0 even with leaks, so the report decides and a crash of gitleaks stays an
/// error.
#[must_use]
pub fn command(range: &Range, report: &Path) -> Vec<String> {
    let mut args = vec!["git".to_owned()];
    if let Range::Commits { from, to } = range {
        args.push(format!("--log-opts={from}..{to}"));
    }
    args.extend(
        [
            "--redact",
            "--no-banner",
            "--ignore-gitleaks-allow",
            "--exit-code",
            "0",
            "--report-format",
            "json",
            "--report-path",
        ]
        .map(str::to_owned),
    );
    args.push(report.display().to_string());
    args
}

/// One entry of gitleaks' JSON report; the `Secret` field is deliberately not read.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Leak {
    #[serde(rename = "RuleID")]
    rule_id: String,
    file: String,
    start_line: u32,
    commit: String,
    #[serde(default)]
    description: String,
}

/// The findings of a gitleaks JSON report: one per leak, at `File:StartLine`, naming the rule and
/// the short commit. The secret is never part of one.
///
/// # Errors
///
/// Fails when `report_json` is not a gitleaks report.
pub fn findings(report_json: &str) -> Result<Vec<Finding>> {
    let leaks: Vec<Leak> = serde_json::from_str(report_json).map_err(|error| Error::Parse {
        what: "the gitleaks report".to_owned(),
        message: error.to_string(),
    })?;
    Ok(leaks
        .into_iter()
        .map(|leak| {
            let commit = leak.commit.get(..SHORT_SHA).unwrap_or(&leak.commit);
            let what = if leak.description.is_empty() {
                String::new()
            } else {
                format!(": {}", leak.description.trim_end_matches('.'))
            };
            Finding {
                file: Some(leak.file.replace('\\', "/")),
                line: Some(leak.start_line),
                message: format!(
                    "gitleaks rule `{}` matched in commit {commit}{what}; remove the secret from \
                     the history and revoke it",
                    leak.rule_id
                ),
            }
        })
        .collect())
}

/// Runs gitleaks over the commits of the change in `root`.
///
/// # Errors
///
/// Fails when gitleaks is not installed, does not run to the end, or writes no readable report.
pub fn run(root: &Path, ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    let range = range(&Git::new(root), ci)?;
    let report: PathBuf = REPORT
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    if let Some(folder) = report.parent() {
        std::fs::create_dir_all(folder).map_err(|source| Error::Io {
            path: folder.to_path_buf(),
            source,
        })?;
    }
    // A stale report must never be read for a run that wrote none.
    if report.exists() {
        std::fs::remove_file(&report).map_err(|source| Error::Io {
            path: report.clone(),
            source,
        })?;
    }
    let args = command(&range, &report);
    let program = which::which("gitleaks").map_err(|_| Error::Missing {
        what: "gitleaks on PATH (see `cargo xtask tools`)".to_owned(),
    })?;
    let output = crate::process::output_program(program.as_os_str(), root, &args)?;
    if !output.status.success() {
        return Err(Error::Command {
            program: "gitleaks".to_owned(),
            args,
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let json = std::fs::read_to_string(&report).map_err(|source| Error::Io {
        path: report.clone(),
        source,
    })?;
    let mut verdict = Verdict::pass(Check::Secrets);
    verdict.findings = findings(&json)?;
    Ok(verdict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::{PullRequestEvent, PushEvent};
    use crate::git::testing::{TestResult, commit, git, init_repo};

    fn pull_request(base: &str, head: &str) -> CiEvent {
        CiEvent::PullRequest(PullRequestEvent {
            action: "synchronize".to_owned(),
            number: 1,
            title: "feat(xtask): a title (P00)".to_owned(),
            body: None,
            base_sha: base.to_owned(),
            head_sha: head.to_owned(),
            head_ref: "phase/P00-x".to_owned(),
            label: None,
        })
    }

    fn commits(from: &str, to: &str) -> Range {
        Range::Commits {
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    // AC: TU-00.3
    #[test]
    fn a_pull_request_scans_its_commits_since_the_merge_base() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let root = commit(path, "chore: root")?;
        git(path, &["switch", "-q", "-c", "topic"])?;
        let head = commit(path, "feat(xtask): topic")?;
        git(path, &["switch", "-q", "main"])?;
        let base = commit(path, "chore: main moved")?;
        let repo = Git::new(path);

        let event = pull_request(&base, &head);
        assert_eq!(range(&repo, Some(&event))?, commits(&root, &head));

        let zeros = "0".repeat(40);
        let push = |before: &str| {
            CiEvent::Push(PushEvent {
                before: before.to_owned(),
                after: head.clone(),
            })
        };
        assert_eq!(range(&repo, Some(&push(&base)))?, commits(&base, &head));
        assert_eq!(range(&repo, Some(&push(&zeros)))?, Range::History);
        assert_eq!(range(&repo, Some(&CiEvent::Other))?, Range::History);

        git(path, &["update-ref", "refs/remotes/origin/main", &base])?;
        git(path, &["switch", "-q", "topic"])?;
        assert_eq!(range(&repo, None)?, commits(&root, "HEAD"));

        git(path, &["checkout", "-q", "--detach"])?;
        assert_eq!(range(&repo, None)?, Range::History);
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn the_command_scans_git_with_a_redacted_json_report() {
        let report = Path::new("target/xtask/gitleaks.json");
        let tail = [
            "--redact",
            "--no-banner",
            "--ignore-gitleaks-allow",
            "--exit-code",
            "0",
            "--report-format",
            "json",
            "--report-path",
            "target/xtask/gitleaks.json",
        ];
        let mut expected = vec!["git".to_owned(), "--log-opts=aaa..bbb".to_owned()];
        expected.extend(tail.map(str::to_owned));
        assert_eq!(command(&commits("aaa", "bbb"), report), expected);

        let mut history = vec!["git".to_owned()];
        history.extend(tail.map(str::to_owned));
        assert_eq!(command(&Range::History, report), history);
    }

    // AC: TU-00.3
    #[test]
    fn each_leak_is_a_finding_without_the_secret() -> TestResult {
        let report = r#"[
          {"RuleID": "generic-api-key", "File": "a/b.txt", "StartLine": 3,
           "Commit": "0123456789abcdef", "Description": "Generic API Key",
           "Secret": "SECRETVALUE", "Match": "key = SECRETVALUE"},
          {"RuleID": "aws-access-token", "File": "c.env", "StartLine": 1,
           "Commit": "fedcba9876543210", "Description": "AWS", "Secret": "SECRETVALUE"}
        ]"#;
        let found = findings(report)?;
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].file.as_deref(), Some("a/b.txt"));
        assert_eq!(found[0].line, Some(3));
        assert!(found[0].message.contains("generic-api-key"));
        assert!(found[0].message.contains("0123456"));
        assert!(!found[0].message.contains("0123456789"));
        assert_eq!(found[1].file.as_deref(), Some("c.env"));
        assert!(
            found
                .iter()
                .all(|finding| !finding.message.contains("SECRETVALUE"))
        );

        assert_eq!(findings("[]")?.len(), 0);
        assert!(matches!(
            findings("no leaks here"),
            Err(Error::Parse { .. })
        ));
        Ok(())
    }
}
