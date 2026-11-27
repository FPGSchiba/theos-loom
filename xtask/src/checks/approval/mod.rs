//! The `approval` check: a pull request that touches a protected path fails until Jann adds the
//! label `approved`, and an approval stays tied to the content it was given for (`docs/gate.md`
//! §5; spec §5.7, decisions D2 and D3).
//!
//! The check runs from the base commit of the pull request, with that commit's
//! `gate/protected-paths.toml` and `gate/repo-settings.toml` (D3), so a pull request cannot change
//! the rules it is judged by. It reads the pull request's side only as git objects. The steps are:
//!
//! 1. [`protected::touched`]: the protected paths and tables the pull request changes. None: pass,
//!    and nothing is read from or written to GitHub.
//!    Then the head of the pull request as GitHub has it: an event for an earlier commit fails and
//!    changes nothing.
//! 2. [`fingerprint::compute`]: the fingerprint of its changes (D2).
//! 3. The comments and the labels as they are now, read from GitHub (the event may be stale).
//! 4. [`decide::decide`]: a verdict, and the actions to make on GitHub, which are made in order.
//!    The verdict is returned only when they all worked, so an approval that could not be recorded
//!    is not a pass.
//!
//! [`run`] is the check for `main`, which needs a pull request event and a GitHub token and so
//! only works under GitHub Actions. [`check`] is the same with everything handed in.

use std::io::Write;
use std::path::Path;

use crate::ci::{CiEvent, PullRequestEvent};
use crate::git::Git;
use crate::github::gh_cli::GhCli;
use crate::github::{LabelSpec, PullRequests, settings};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

pub mod decide;
pub mod fingerprint;
pub mod protected;

use decide::{Action, Event};
use protected::ProtectedPaths;

/// The failure of a run whose event is not for the current head of the pull request.
const STALE_RUN: &str =
    "this run is for an earlier commit of the pull request; the run for its newest commit decides";

/// Runs the check on the checkout at `root` for the pull request of `ci`, reading and changing the
/// labels and comments of the repository in `GITHUB_REPOSITORY` with the token in `GH_TOKEN`.
///
/// # Errors
///
/// [`Error::Missing`] when there is no pull request event or `GH_TOKEN` or `GITHUB_REPOSITORY` is
/// not set, which is the case outside GitHub Actions. Otherwise the errors of [`check`] and of
/// reading the rules and the labels.
pub fn run(root: &Path, ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    let pr = pull_request(ci)?;
    let gh = client(|name| std::env::var(name).ok())?;
    let rules = ProtectedPaths::load(root)?;
    let labels = settings::labels(root)?;
    check(&Git::new(root), &rules, &labels, pr, &gh)
}

/// Checks `pr` in the repository `git` reads, with the rules `rules`, and reads and changes its
/// labels and comments through `gh`. `labels` say how the labels of the gate look, for the case
/// that the repository lacks one.
///
/// When the head commit of the pull request is not in the repository (the job checks out the
/// base commit), it is fetched as `refs/pull/<n>/head` first. A pull request that touches no
/// protected path passes before `gh` is called at all. For one that does, the head GitHub has now
/// is read first: when it is not the head of the event, the run is for an earlier commit and fails
/// without recording, removing or writing anything.
///
/// # Errors
///
/// Fails when git fails, the head commit cannot be found, or, for a protected pull request, when
/// GitHub fails or `labels` lack a label that the check wants to create.
pub fn check(
    git: &Git,
    rules: &ProtectedPaths,
    labels: &[LabelSpec],
    pr: &PullRequestEvent,
    gh: &dyn PullRequests,
) -> Result<Verdict> {
    // The head comes first: a merge base needs both commits.
    let head = resolve_head(git, pr)?;
    let merge_base = git.merge_base(&pr.base_sha, &head)?;
    let touched = protected::touched(git, rules, &merge_base, &head)?;
    // Nothing protected: pass before anything else is read, so that a pull request that needs no
    // approval does not depend on GitHub being up.
    if touched.is_empty() {
        return Ok(Verdict::pass(Check::Approval));
    }
    // A run for an earlier commit (an old event run again) must not record or undo anything: what
    // it would write belongs to a head that is no longer the pull request.
    if !gh.head_sha(pr.number)?.eq_ignore_ascii_case(&pr.head_sha) {
        return Ok(Verdict::with(
            Check::Approval,
            vec![Finding::new(STALE_RUN)],
        ));
    }
    let current = fingerprint::compute(git, &merge_base, &head)?;
    let comments = gh.comments(pr.number)?;
    // Labels last, so that they are as fresh as they can be.
    let on_pr = gh.labels(pr.number)?;
    let recorded = decide::recorded(&comments);
    let (verdict, actions) = decide::decide(
        &touched,
        &Event::from_pull_request(pr),
        &on_pr,
        recorded.as_ref(),
        &current,
        &head,
    );
    for action in &actions {
        apply(gh, pr.number, action, labels)?;
    }
    Ok(verdict)
}

/// The full id of the head commit of `pr`, fetching `refs/pull/<n>/head` when the repository
/// lacks it. Resolved to the commit id, it is also hex all through, which the comment relies on.
fn resolve_head(git: &Git, pr: &PullRequestEvent) -> Result<String> {
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

fn apply(gh: &dyn PullRequests, pr: u64, action: &Action, labels: &[LabelSpec]) -> Result<()> {
    match action {
        Action::EnsureLabel(name) => gh.ensure_label(label_spec(labels, name)?),
        Action::AddLabel(name) => gh.add_label(pr, name),
        Action::RemoveLabel(name) => gh.remove_label(pr, name),
        Action::Comment(body) => gh.comment(pr, body),
    }
}

/// How the label `name` looks, from `gate/repo-settings.toml`.
fn label_spec<'a>(labels: &'a [LabelSpec], name: &str) -> Result<&'a LabelSpec> {
    labels
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| Error::Config {
            file: settings::FILE.to_owned(),
            message: format!("has no label `{name}`, which `approval` puts on a pull request"),
        })
}

/// The pull request event, or [`Error::Missing`] when the run has none.
fn pull_request(ci: Option<&CiEvent>) -> Result<&PullRequestEvent> {
    match ci {
        Some(CiEvent::PullRequest(pr)) => Ok(pr),
        _ => Err(Error::Missing {
            what:
                "pull request event: `approval` only runs for a pull request under GitHub Actions"
                    .to_owned(),
        }),
    }
}

/// The GitHub client for the environment `var` describes. `gh` reads `GH_TOKEN` itself; it has
/// to be there, or every call would fail on a prompt.
fn client(var: impl Fn(&str) -> Option<String>) -> Result<GhCli> {
    let required = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::Missing {
                what: format!("environment variable {name}: `approval` talks to GitHub"),
            })
    };
    required("GH_TOKEN")?;
    Ok(GhCli {
        repo: required("GITHUB_REPOSITORY")?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    use super::decide::{BOT, marker};
    use super::*;
    use crate::git::testing::{TestResult, commit, git, init_repo};
    use crate::github::Comment;
    use crate::github::fake::{Call, FakeGitHub};

    fn rules() -> TestResult<ProtectedPaths> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("xtask has no parent folder")?;
        Ok(ProtectedPaths::load(root)?)
    }

    fn write(dir: &Path, path: &str, text: &str) -> TestResult {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
        Ok(())
    }

    /// The pull request event of `head` against `base`.
    fn event(action: &str, label: Option<&str>, base: &str, head: &str) -> PullRequestEvent {
        PullRequestEvent {
            action: action.to_owned(),
            number: 7,
            title: "docs: change".to_owned(),
            body: None,
            base_sha: base.to_owned(),
            head_sha: head.to_owned(),
            head_ref: "docs/change".to_owned(),
            label: label.map(str::to_owned),
        }
    }

    /// `main` with a protected and an unprotected file; returns the repository and the commit.
    fn main_repo() -> TestResult<(tempfile::TempDir, String)> {
        let dir = init_repo()?;
        write(dir.path(), "docs/a.md", "one\n")?;
        write(dir.path(), "src/lib.rs", "// one\n")?;
        let base = commit(dir.path(), "feat: base")?;
        Ok((dir, base))
    }

    /// A branch off `main` with one commit that changes `path`; returns the commit.
    fn change(dir: &Path, path: &str, text: &str) -> TestResult<String> {
        write(dir, path, text)?;
        commit(dir, "feat: change")
    }

    /// The labels of `gate/repo-settings.toml` of this repository.
    fn labels() -> TestResult<Vec<LabelSpec>> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("xtask has no parent folder")?;
        Ok(settings::labels(root)?)
    }

    /// What `needs-jann` looks like, as the settings say.
    fn needs_jann() -> TestResult<LabelSpec> {
        let spec = labels()?
            .into_iter()
            .find(|spec| spec.name == "needs-jann")
            .ok_or("the settings have no `needs-jann`")?;
        Ok(spec)
    }

    fn needs_jann_calls() -> TestResult<Vec<Call>> {
        Ok(vec![
            Call::EnsureLabel(needs_jann()?),
            Call::AddLabel(7, "needs-jann".to_owned()),
        ])
    }

    // AC: TU-00.4
    #[test]
    fn an_unprotected_pr_passes_and_changes_nothing() -> TestResult {
        let (dir, base) = main_repo()?;
        git(dir.path(), &["switch", "-q", "-c", "topic"])?;
        let head = change(dir.path(), "src/lib.rs", "// two\n")?;
        let gh = FakeGitHub::new();

        let verdict = check(
            &Git::new(dir.path()),
            &rules()?,
            &labels()?,
            &event("opened", None, &base, &head),
            &gh,
        )?;
        assert!(verdict.passed(), "{verdict:?}");
        // Not a read either: an unprotected PR does not depend on GitHub being up.
        assert_eq!(gh.calls(), []);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_protected_pr_waits_for_jann_and_gets_needs_jann() -> TestResult {
        let (dir, base) = main_repo()?;
        git(dir.path(), &["switch", "-q", "-c", "topic"])?;
        let head = change(dir.path(), "docs/a.md", "two\n")?;
        let gh = FakeGitHub::new().with_head(&head);
        let git_repo = Git::new(dir.path());
        let pr = event("opened", None, &base, &head);

        let verdict = check(&git_repo, &rules()?, &labels()?, &pr, &gh)?;
        assert!(!verdict.passed());
        assert_eq!(
            verdict.findings[0].message,
            "waits for Jann: add the label `approved` after review"
        );
        // Reads first, labels last; then the writes.
        let mut expected = vec![Call::HeadSha(7), Call::Comments(7), Call::Labels(7)];
        expected.extend(needs_jann_calls()?);
        assert_eq!(gh.calls(), expected);
        assert_eq!(gh.current_labels(), ["needs-jann"]);

        // A second run finds the label there and changes nothing.
        let verdict = check(&git_repo, &rules()?, &labels()?, &pr, &gh)?;
        assert!(!verdict.passed());
        assert_eq!(gh.writes(), needs_jann_calls()?);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn approval_follows_the_content_of_the_pr() -> TestResult {
        let (dir, base) = main_repo()?;
        let path = dir.path();
        let git_repo = Git::new(path);
        git(path, &["switch", "-q", "-c", "topic"])?;
        let first = change(path, "docs/a.md", "two\n")?;
        let gh = FakeGitHub::new()
            .with_labels(&["needs-jann"])
            .with_head(&first);

        // Jann adds `approved`: the fingerprint is recorded and the check passes.
        gh.add_label(7, "approved")?;
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("labeled", Some("approved"), &base, &first),
            &gh,
        )?;
        assert!(verdict.passed(), "{verdict:?}");
        let merge_base = git_repo.merge_base(&base, &first)?;
        let fingerprint = fingerprint::compute(&git_repo, &merge_base, &first)?;
        assert_eq!(
            gh.writes().last(),
            Some(&Call::Comment(7, marker(&fingerprint, &first)))
        );

        // `main` moves on and the branch is rebased: the PR's own changes are the same.
        git(path, &["switch", "-q", "main"])?;
        write(path, "src/lib.rs", "// main moved\n")?;
        let tip = commit(path, "feat: main moves")?;
        git(path, &["switch", "-q", "topic"])?;
        git(path, &["rebase", "-q", "main"])?;
        let rebased = git(path, &["rev-parse", "HEAD"])?;
        assert_ne!(rebased, first);
        gh.set_head(&rebased);
        let writes_before = gh.writes().len();
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("synchronize", None, &tip, &rebased),
            &gh,
        )?;
        assert!(verdict.passed(), "{verdict:?}");
        assert_eq!(gh.writes().len(), writes_before, "nothing is changed");

        // A push that changes the PR: the approval goes, `needs-jann` stays.
        let changed = change(path, "docs/a.md", "three\n")?;
        gh.set_head(&changed);
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("synchronize", None, &tip, &changed),
            &gh,
        )?;
        assert!(!verdict.passed());
        let changed_merge_base = git_repo.merge_base(&tip, &changed)?;
        let now = fingerprint::compute(&git_repo, &changed_merge_base, &changed)?;
        let reason = format!(
            "approval removed: the PR changed after approval ({} → {})",
            fingerprint.0, now.0
        );
        assert_eq!(verdict.findings[0].message, reason);
        assert_eq!(
            gh.writes()[writes_before..],
            [
                Call::RemoveLabel(7, "approved".to_owned()),
                Call::Comment(7, reason)
            ]
        );
        assert_eq!(gh.current_labels(), ["needs-jann"]);

        // And the next run waits for Jann again.
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("synchronize", None, &tip, &changed),
            &gh,
        )?;
        assert_eq!(
            verdict.findings[0].message,
            "waits for Jann: add the label `approved` after review"
        );
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_run_for_an_earlier_commit_changes_nothing() -> TestResult {
        let (dir, base) = main_repo()?;
        let path = dir.path();
        let git_repo = Git::new(path);
        git(path, &["switch", "-q", "-c", "topic"])?;
        let old = change(path, "docs/a.md", "two\n")?;
        let new = change(path, "docs/a.md", "three\n")?;
        // Jann approved `new`; someone re-runs the run of the `labeled` event of `old`.
        let gh = FakeGitHub::new()
            .with_labels(&["needs-jann", "approved"])
            .with_head(&new);
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("labeled", Some("approved"), &base, &old),
            &gh,
        )?;
        assert!(!verdict.passed());
        assert_eq!(
            verdict.findings[0].message,
            concat!(
                "this run is for an earlier commit of the pull request; ",
                "the run for its newest commit decides"
            )
        );
        // One read of the head, then nothing: no record, no label change, no comment.
        assert_eq!(gh.calls(), [Call::HeadSha(7)]);
        assert_eq!(gh.current_labels(), ["needs-jann", "approved"]);
        assert_eq!(gh.current_comments(), []);

        // The run for the current head records and passes.
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("labeled", Some("approved"), &base, &new),
            &gh,
        )?;
        assert!(verdict.passed(), "{verdict:?}");
        assert_eq!(gh.current_comments().len(), 1);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_comment_by_someone_else_does_not_keep_an_approval() -> TestResult {
        let (dir, base) = main_repo()?;
        git(dir.path(), &["switch", "-q", "-c", "topic"])?;
        let head = change(dir.path(), "docs/a.md", "two\n")?;
        let git_repo = Git::new(dir.path());
        let merge_base = git_repo.merge_base(&base, &head)?;
        let current = fingerprint::compute(&git_repo, &merge_base, &head)?;
        let at = "2026-10-31T10:00:00Z";
        let forged = |author: &str, updated: &str| Comment {
            author: author.to_owned(),
            body: marker(&current, &head),
            created_at: at.to_owned(),
            updated_at: updated.to_owned(),
        };
        // The right fingerprint, but written by Jann's account, and by the bot but edited.
        for comment in [forged("jann", at), forged(BOT, "2026-10-31T10:05:00Z")] {
            let gh = FakeGitHub::new()
                .with_labels(&["needs-jann", "approved"])
                .with_head(&head)
                .with_comment(comment);
            let verdict = check(
                &git_repo,
                &rules()?,
                &labels()?,
                &event("synchronize", None, &base, &head),
                &gh,
            )?;
            // The forged record counts as none: the check fails, and since the label may be
            // waiting for the run that records it, nothing is removed or written.
            assert!(!verdict.passed());
            assert_eq!(gh.writes(), []);
            assert_eq!(gh.current_labels(), ["needs-jann", "approved"]);
        }
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn approved_with_nothing_recorded_fails_without_removing_or_commenting() -> TestResult {
        let (dir, base) = main_repo()?;
        git(dir.path(), &["switch", "-q", "-c", "topic"])?;
        let head = change(dir.path(), "docs/a.md", "two\n")?;
        let git_repo = Git::new(dir.path());
        // The run of an `unlabeled needs-jann` event sees `approved` before the run of the
        // `labeled approved` event has recorded it.
        let gh = FakeGitHub::new()
            .with_labels(&["approved"])
            .with_head(&head);
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("unlabeled", Some("needs-jann"), &base, &head),
            &gh,
        )?;
        assert!(!verdict.passed());
        assert_eq!(gh.writes(), needs_jann_calls()?);
        assert_eq!(gh.current_labels(), ["approved", "needs-jann"]);
        assert_eq!(gh.current_comments(), []);

        // Then the run of the `labeled approved` event records, and passes.
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("labeled", Some("approved"), &base, &head),
            &gh,
        )?;
        assert!(verdict.passed(), "{verdict:?}");
        assert_eq!(gh.current_comments().len(), 1);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn an_approval_that_cannot_be_recorded_is_not_a_pass() -> TestResult {
        let (dir, base) = main_repo()?;
        git(dir.path(), &["switch", "-q", "-c", "topic"])?;
        let head = change(dir.path(), "docs/a.md", "two\n")?;
        let gh = FakeGitHub::new()
            .with_labels(&["needs-jann", "approved"])
            .with_head(&head)
            .with_failing_comments();
        let result = check(
            &Git::new(dir.path()),
            &rules()?,
            &labels()?,
            &event("labeled", Some("approved"), &base, &head),
            &gh,
        );
        assert!(matches!(result, Err(Error::GitHub { .. })), "{result:?}");
        Ok(())
    }

    #[test]
    fn a_label_the_settings_lack_is_an_error_and_is_not_created() -> TestResult {
        let (dir, base) = main_repo()?;
        git(dir.path(), &["switch", "-q", "-c", "topic"])?;
        let head = change(dir.path(), "docs/a.md", "two\n")?;
        let gh = FakeGitHub::new().with_head(&head);
        let result = check(
            &Git::new(dir.path()),
            &rules()?,
            &[],
            &event("opened", None, &base, &head),
            &gh,
        );
        let Err(Error::Config { file, message }) = &result else {
            return Err(format!("expected a config error, got {result:?}").into());
        };
        assert_eq!(file, "gate/repo-settings.toml");
        assert!(message.contains("`needs-jann`"), "{message}");
        assert_eq!(gh.writes(), []);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_head_that_is_missing_is_fetched_as_the_pull_request_ref() -> TestResult {
        let (origin, base) = main_repo()?;
        let path = origin.path();
        git(path, &["switch", "-q", "-c", "topic"])?;
        let head = change(path, "docs/a.md", "two\n")?;
        // GitHub keeps the head of a pull request in this ref; a clone of `main` lacks it.
        git(path, &["update-ref", "refs/pull/7/head", &head])?;
        git(path, &["switch", "-q", "main"])?;
        git(path, &["branch", "-q", "-D", "topic"])?;
        let checkout = tempfile::tempdir()?;
        let url = path.to_str().ok_or("temporary path is not UTF-8")?;
        let target = checkout
            .path()
            .to_str()
            .ok_or("temporary path is not UTF-8")?;
        git(
            path,
            &[
                "clone",
                "-q",
                "--no-local",
                "--no-tags",
                "--single-branch",
                "--branch",
                "main",
                url,
                target,
            ],
        )?;
        let git_repo = Git::new(checkout.path());
        assert!(git_repo.rev_parse(&format!("{head}^{{commit}}")).is_err());

        let gh = FakeGitHub::new().with_head(&head);
        let verdict = check(
            &git_repo,
            &rules()?,
            &labels()?,
            &event("opened", None, &base, &head),
            &gh,
        )?;
        assert!(!verdict.passed(), "docs/a.md is protected");
        assert_eq!(git_repo.rev_parse(&format!("{head}^{{commit}}"))?, head);
        Ok(())
    }

    #[test]
    fn a_head_that_cannot_be_fetched_is_an_error() -> TestResult {
        let (dir, base) = main_repo()?;
        let gh = FakeGitHub::new();
        // No `origin` and no such commit.
        let result = check(
            &Git::new(dir.path()),
            &rules()?,
            &labels()?,
            &event("opened", None, &base, &"0".repeat(40)),
            &gh,
        );
        assert!(matches!(result, Err(Error::Command { .. })), "{result:?}");
        assert_eq!(gh.calls(), []);
        Ok(())
    }

    #[test]
    fn the_event_must_be_a_pull_request() {
        let missing = |ci: Option<&CiEvent>| match pull_request(ci) {
            Err(Error::Missing { what }) => what,
            other => panic!("expected Error::Missing, got {other:?}"),
        };
        assert!(missing(None).contains("pull request event"));
        assert!(missing(Some(&CiEvent::Other)).contains("pull request event"));
        let pr = event("opened", None, "a", "b");
        let ci = CiEvent::PullRequest(pr.clone());
        assert_eq!(pull_request(Some(&ci)).ok(), Some(&pr));
    }

    #[test]
    fn the_client_needs_a_token_and_a_repository() {
        let vars = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, String> = pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect();
            move |name: &str| map.get(name).cloned()
        };
        let missing = |result: Result<GhCli>| match result {
            Err(Error::Missing { what }) => what,
            other => panic!("expected Error::Missing, got {other:?}"),
        };
        assert!(missing(client(vars(&[]))).contains("GH_TOKEN"));
        assert!(missing(client(vars(&[("GH_TOKEN", "")]))).contains("GH_TOKEN"));
        assert!(missing(client(vars(&[("GH_TOKEN", "t")]))).contains("GITHUB_REPOSITORY"));
        assert!(
            missing(client(vars(&[
                ("GH_TOKEN", "t"),
                ("GITHUB_REPOSITORY", "")
            ])))
            .contains("GITHUB_REPOSITORY")
        );
        assert_eq!(
            client(vars(&[("GH_TOKEN", "t"), ("GITHUB_REPOSITORY", "o/r")])).ok(),
            Some(GhCli {
                repo: "o/r".to_owned()
            })
        );
    }

    #[test]
    fn outside_github_actions_the_check_cannot_run() {
        let root = Path::new(".");
        let result = run(root, None, &mut Vec::new());
        assert!(matches!(result, Err(Error::Missing { .. })), "{result:?}");
    }
}
