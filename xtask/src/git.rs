//! The `git` command line: the only way this crate reads a repository.
//!
//! Every call runs `git -C <root> -c core.quotepath=false …` through `std::process::Command`
//! with an argument list, never a shell, and returns what git wrote to stdout. A call that does
//! not start or does not exit 0 is an [`Error::Command`] with git's stderr. Revisions given by
//! callers may come from flags or event payloads, so one that starts with `-` is refused rather
//! than read by git as an option.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::{Error, Result};

/// A git repository, reached with `git -C <root>`.
#[derive(Clone, Debug)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// The repository whose work tree is at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The object id `rev` names (`git rev-parse --verify`).
    ///
    /// # Errors
    ///
    /// Fails when `rev` names no object or git cannot run.
    pub fn rev_parse(&self, rev: &str) -> Result<String> {
        refuse_option(rev)?;
        Ok(self.run(&["rev-parse", "--verify", rev])?.trim().to_owned())
    }

    /// The best common ancestor of two revisions (`git merge-base`).
    ///
    /// # Errors
    ///
    /// Fails when either revision is unknown or they have no common ancestor.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<String> {
        refuse_option(a)?;
        refuse_option(b)?;
        Ok(self.run(&["merge-base", a, b])?.trim().to_owned())
    }

    /// The commits in `range` (`a..b` or a single revision), newest first (`git rev-list`).
    ///
    /// # Errors
    ///
    /// Fails when the range is invalid.
    pub fn rev_list(&self, range: &str) -> Result<Vec<String>> {
        refuse_option(range)?;
        Ok(lines(&self.run(&["rev-list", range])?))
    }

    /// The parents of a commit; none for a root commit, two or more for a merge.
    ///
    /// # Errors
    ///
    /// Fails when `sha` is not a commit.
    pub fn parents(&self, sha: &str) -> Result<Vec<String>> {
        refuse_option(sha)?;
        let line = self.run(&["rev-list", "--parents", "-n", "1", sha])?;
        Ok(line.split_whitespace().skip(1).map(str::to_owned).collect())
    }

    /// The message of a commit as stored, without its trailing newline.
    ///
    /// # Errors
    ///
    /// Fails when `sha` is not a commit.
    pub fn message(&self, sha: &str) -> Result<String> {
        refuse_option(sha)?;
        let commit = self.run(&["cat-file", "commit", sha])?;
        // The headers end at the first empty line; the message is everything after it.
        let message = commit.split_once("\n\n").map_or("", |(_, message)| message);
        Ok(message.trim_end_matches('\n').to_owned())
    }

    /// The files that differ between two revisions, with renames shown as a delete and an add
    /// (`git diff --name-only --no-renames`). Paths are repo-relative with `/`.
    ///
    /// # Errors
    ///
    /// Fails when either revision is unknown.
    pub fn diff_name_only_no_renames(&self, base: &str, head: &str) -> Result<Vec<String>> {
        refuse_option(base)?;
        refuse_option(head)?;
        let names = self.run(&["diff", "--name-only", "--no-renames", "-z", base, head])?;
        Ok(names
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// The raw diff between two revisions: one line per changed file with modes, full object
    /// ids and status (`git diff --raw --no-renames --no-abbrev`).
    ///
    /// # Errors
    ///
    /// Fails when either revision is unknown.
    pub fn diff_raw(&self, base: &str, head: &str) -> Result<String> {
        refuse_option(base)?;
        refuse_option(head)?;
        self.run(&["diff", "--raw", "--no-renames", "--no-abbrev", base, head])
    }

    /// The text of `path` at `rev`, or `None` when the revision has no such file.
    ///
    /// # Errors
    ///
    /// Fails when `rev` is unknown, or `path` is not a file.
    pub fn show(&self, rev: &str, path: &str) -> Result<Option<String>> {
        refuse_option(rev)?;
        // An unknown revision is a failure; only a missing path means "absent".
        self.rev_parse(rev)?;
        let spec = format!("{rev}:{path}");
        let lookup = ["rev-parse", "--verify", "--quiet", spec.as_str()];
        let found = self.output(&lookup)?;
        if found.status.code() == Some(1) {
            return Ok(None);
        }
        let id = Self::stdout_of(&lookup, &found)?;
        Ok(Some(self.run(&["cat-file", "blob", id.trim()])?))
    }

    /// Fetches the head of pull request `number` from `origin`, which makes its commits available
    /// here (`git fetch --no-tags origin refs/pull/<number>/head`). No ref is created. The
    /// refspec is made from a number, so it cannot be read as an option.
    ///
    /// # Errors
    ///
    /// Fails when there is no `origin` or it has no such pull request.
    pub fn fetch_pull_head(&self, number: u64) -> Result<()> {
        let refspec = format!("refs/pull/{number}/head");
        self.run(&["fetch", "--no-tags", "origin", refspec.as_str()])?;
        Ok(())
    }

    /// The branch `HEAD` is on, or `None` when `HEAD` is detached.
    ///
    /// # Errors
    ///
    /// Fails when git cannot run or the root is not a repository.
    pub fn current_branch(&self) -> Result<Option<String>> {
        let args = ["symbolic-ref", "--quiet", "--short", "HEAD"];
        let output = self.output(&args)?;
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        Ok(Some(Self::stdout_of(&args, &output)?.trim().to_owned()))
    }

    /// The URL the remote `name` fetches from (`git remote get-url`).
    ///
    /// # Errors
    ///
    /// Fails when there is no such remote or git cannot run.
    pub fn remote_url(&self, name: &str) -> Result<String> {
        refuse_option(name)?;
        Ok(self.run(&["remote", "get-url", name])?.trim().to_owned())
    }

    /// Checks out `sha` in a new work tree at `dir`, with `HEAD` detached. A relative `dir` is
    /// relative to the repository root.
    ///
    /// # Errors
    ///
    /// Fails when `sha` is unknown or `dir` cannot be created.
    pub fn worktree_add_detached(&self, dir: &Path, sha: &str) -> Result<()> {
        refuse_option(sha)?;
        self.run(&[
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--quiet"),
            OsStr::new("--detach"),
            dir.as_os_str(),
            OsStr::new(sha),
        ])?;
        Ok(())
    }

    /// Removes the work tree at `dir`, even if it has changes.
    ///
    /// # Errors
    ///
    /// Fails when `dir` is not a work tree of this repository.
    pub fn worktree_remove(&self, dir: &Path) -> Result<()> {
        self.run(&[
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            dir.as_os_str(),
        ])?;
        Ok(())
    }

    /// Forgets the work trees whose directories are gone (`git worktree prune`).
    ///
    /// # Errors
    ///
    /// Fails when git cannot run or the root is not a repository.
    pub fn worktree_prune(&self) -> Result<()> {
        self.run(&["worktree", "prune"])?;
        Ok(())
    }

    /// Whether the root is inside a git work tree. Any failure means it is not.
    pub(crate) fn is_inside_work_tree(&self) -> bool {
        self.run(&["rev-parse", "--is-inside-work-tree"])
            .is_ok_and(|answer| answer.trim() == "true")
    }

    /// Tracked files and files git does not ignore, as `git ls-files --cached --others
    /// --exclude-standard` lists them.
    pub(crate) fn ls_files(&self) -> Result<Vec<String>> {
        let names = self.run(&[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])?;
        Ok(names
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// Runs git and returns its stdout, or the failure as an [`Error::Command`].
    fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let output = self.output(args)?;
        Self::stdout_of(args, &output)
    }

    fn stdout_of<S: AsRef<OsStr>>(args: &[S], output: &Output) -> Result<String> {
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(Error::Command {
                program: "git".to_owned(),
                args: arg_strings(args),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    /// Runs git and returns its output whatever the exit status; only a git that cannot start
    /// is an error.
    fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Output> {
        tracing::debug!(root = %self.root.display(), args = ?arg_strings(args), "git");
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "core.quotepath=false"])
            .args(args)
            .output()
            .map_err(|source| Error::Command {
                program: "git".to_owned(),
                args: arg_strings(args),
                status: None,
                stderr: source.to_string(),
            })
    }
}

fn arg_strings<S: AsRef<OsStr>>(args: &[S]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect()
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

fn refuse_option(rev: &str) -> Result<()> {
    if rev.starts_with('-') {
        return Err(Error::Parse {
            what: "git revision".to_owned(),
            message: format!("`{rev}` starts with `-` and would be read as an option"),
        });
    }
    Ok(())
}

/// Real repositories for tests, made with the `git` command line like everything else here.
#[cfg(test)]
pub(crate) mod testing {
    use std::error::Error;
    use std::path::Path;
    use std::process::Command;

    pub(crate) type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    /// Runs `git` in `dir` with an identity and no signing, so tests need no git configuration.
    pub(crate) fn git(dir: &Path, args: &[&str]) -> TestResult<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
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

    /// A new repository on branch `main` with no commits. Line endings are left as written, so
    /// a developer's `core.autocrlf` does not change what the tests read back.
    pub(crate) fn init_repo() -> TestResult<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        git(dir.path(), &["init", "-q", "-b", "main"])?;
        git(dir.path(), &["config", "core.autocrlf", "false"])?;
        Ok(dir)
    }

    /// Commits everything in the work tree (an empty commit if nothing changed) and returns
    /// the new commit's id.
    pub(crate) fn commit(dir: &Path, message: &str) -> TestResult<String> {
        git(dir, &["add", "-A"])?;
        git(dir, &["commit", "-q", "--allow-empty", "-m", message])?;
        git(dir, &["rev-parse", "HEAD"])
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{TestResult, commit, git, init_repo};
    use super::*;
    use std::fs;

    #[test]
    fn git_reads_messages_and_parents() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let first = commit(path, "feat: first")?;
        git(path, &["switch", "-q", "-c", "side"])?;
        let side = commit(path, "fix: on the side")?;
        git(path, &["switch", "-q", "main"])?;
        let second = commit(path, "docs: second\n\nA body line.\n\nPhase: P00")?;
        git(
            path,
            &["merge", "-q", "--no-ff", "-m", "chore: merge", "side"],
        )?;
        let merge = git(path, &["rev-parse", "HEAD"])?;

        let repo = Git::new(path);
        assert_eq!(repo.message(&first)?, "feat: first");
        assert_eq!(
            repo.message(&second)?,
            "docs: second\n\nA body line.\n\nPhase: P00"
        );
        assert_eq!(repo.parents(&first)?, Vec::<String>::new());
        assert_eq!(repo.parents(&second)?, [first]);
        assert_eq!(repo.parents(&merge)?, [second, side]);
        Ok(())
    }

    #[test]
    fn git_resolves_revisions_and_ranges() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let base = commit(path, "feat: base")?;
        git(path, &["switch", "-q", "-c", "topic"])?;
        let one = commit(path, "feat: one")?;
        let two = commit(path, "feat: two")?;
        git(path, &["switch", "-q", "main"])?;
        let tip = commit(path, "feat: main moves on")?;

        let repo = Git::new(path);
        assert_eq!(repo.rev_parse("topic")?, two);
        assert_eq!(repo.rev_parse("main~1")?, base);
        assert_eq!(repo.merge_base("main", "topic")?, base);
        assert_eq!(repo.rev_list("main..topic")?, [two, one]);
        assert_eq!(repo.rev_list("topic..topic")?, Vec::<String>::new());
        assert_eq!(repo.rev_list(&format!("{base}..{tip}"))?, [tip]);
        Ok(())
    }

    #[test]
    fn git_names_the_url_of_a_remote() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let repo = Git::new(path);
        assert!(repo.remote_url("origin").is_err());
        git(
            path,
            &["remote", "add", "origin", "git@github.com:owner/name.git"],
        )?;
        assert_eq!(repo.remote_url("origin")?, "git@github.com:owner/name.git");
        assert!(matches!(repo.remote_url("--all"), Err(Error::Parse { .. })));
        Ok(())
    }

    #[test]
    fn git_lists_changed_files_and_raw_diffs() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        fs::write(path.join("kept.txt"), "kept\n")?;
        fs::write(path.join("old name.txt"), "same content\nsame content\n")?;
        fs::write(path.join("edited.txt"), "before\n")?;
        let base = commit(path, "feat: base")?;
        fs::create_dir(path.join("sub"))?;
        fs::write(path.join("sub").join("new.txt"), "new\n")?;
        fs::write(path.join("edited.txt"), "after\n")?;
        fs::rename(path.join("old name.txt"), path.join("new name.txt"))?;
        let head = commit(path, "feat: change")?;

        let repo = Git::new(path);
        let mut names = repo.diff_name_only_no_renames(&base, &head)?;
        names.sort();
        assert_eq!(
            names,
            ["edited.txt", "new name.txt", "old name.txt", "sub/new.txt"]
        );
        assert_eq!(
            repo.diff_name_only_no_renames(&head, &head)?,
            Vec::<String>::new()
        );

        let raw = repo.diff_raw(&base, &head)?;
        let mut lines: Vec<&str> = raw.lines().collect();
        lines.sort_by_key(|line| line.split('\t').nth(1).map(str::to_owned));
        assert_eq!(lines.len(), 4, "{raw}");
        assert!(lines[0].starts_with(":100644 100644 "), "{raw}");
        assert!(lines[0].ends_with(" M\tedited.txt"), "{raw}");
        assert!(lines[1].ends_with(" A\tnew name.txt"), "{raw}");
        assert!(lines[2].ends_with(" D\told name.txt"), "{raw}");
        assert!(lines[3].ends_with(" A\tsub/new.txt"), "{raw}");
        // `--no-abbrev`: both object ids are complete.
        let ids: Vec<&str> = lines[0].split(' ').collect();
        assert_eq!(
            (ids[2].len(), ids[3].len()),
            (base.len(), base.len()),
            "{raw}"
        );
        Ok(())
    }

    #[test]
    fn git_show_reads_blobs_and_reports_absence() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        fs::create_dir(path.join("docs"))?;
        fs::write(path.join("docs").join("a.md"), "one\r\ntwo\n")?;
        let first = commit(path, "docs: add a")?;
        fs::write(path.join("docs").join("a.md"), "changed\n")?;
        commit(path, "docs: change a")?;

        let repo = Git::new(path);
        assert_eq!(
            repo.show(&first, "docs/a.md")?.as_deref(),
            Some("one\r\ntwo\n")
        );
        assert_eq!(
            repo.show("HEAD", "docs/a.md")?.as_deref(),
            Some("changed\n")
        );
        assert_eq!(repo.show(&first, "docs/missing.md")?, None);
        assert_eq!(repo.show("HEAD", "nope/a.md")?, None);
        let bad_revision = repo.show("no-such-revision", "docs/a.md");
        assert!(
            matches!(bad_revision, Err(Error::Command { .. })),
            "{bad_revision:?}"
        );
        Ok(())
    }

    #[test]
    fn git_fetches_the_head_of_a_pull_request_from_origin() -> TestResult {
        let origin = init_repo()?;
        let first = commit(origin.path(), "feat: first")?;
        git(origin.path(), &["switch", "-q", "-c", "topic"])?;
        let pr_head = commit(origin.path(), "feat: the pull request")?;
        git(origin.path(), &["update-ref", "refs/pull/7/head", &pr_head])?;
        git(origin.path(), &["switch", "-q", "main"])?;
        git(origin.path(), &["branch", "-q", "-D", "topic"])?;

        let clone = tempfile::tempdir()?;
        let from = origin
            .path()
            .to_str()
            .ok_or("temporary path is not UTF-8")?;
        let to = clone.path().to_str().ok_or("temporary path is not UTF-8")?;
        git(
            origin.path(),
            // `--no-local` makes a real fetch: a plain local clone copies every object.
            &[
                "clone",
                "-q",
                "--no-local",
                "--no-tags",
                "--single-branch",
                "--branch",
                "main",
                from,
                to,
            ],
        )?;
        let repo = Git::new(clone.path());
        let commit_of = |rev: &str| repo.rev_parse(&format!("{rev}^{{commit}}"));
        assert_eq!(commit_of(&first)?, first);
        assert!(commit_of(&pr_head).is_err(), "the clone lacks the head");

        repo.fetch_pull_head(7)?;
        assert_eq!(commit_of(&pr_head)?, pr_head);
        // The commits are here, but no branch or tag was made for them.
        assert_eq!(git(clone.path(), &["tag", "--list"])?, "");
        assert_eq!(git(clone.path(), &["branch", "--list"])?, "* main");

        let missing = repo.fetch_pull_head(8);
        assert!(matches!(missing, Err(Error::Command { .. })), "{missing:?}");
        let no_origin = Git::new(origin.path()).fetch_pull_head(7);
        assert!(
            matches!(no_origin, Err(Error::Command { .. })),
            "{no_origin:?}"
        );
        Ok(())
    }

    #[test]
    fn git_names_the_current_branch() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let repo = Git::new(path);
        assert_eq!(repo.current_branch()?.as_deref(), Some("main"));
        let first = commit(path, "feat: first")?;
        git(path, &["switch", "-q", "-c", "phase/P00-x"])?;
        assert_eq!(repo.current_branch()?.as_deref(), Some("phase/P00-x"));
        git(path, &["switch", "-q", "--detach", &first])?;
        assert_eq!(repo.current_branch()?, None);
        Ok(())
    }

    #[test]
    fn git_worktrees_can_be_added_and_removed() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        fs::write(path.join("marker.txt"), "here\n")?;
        let first = commit(path, "feat: first")?;
        commit(path, "feat: second")?;

        let repo = Git::new(path);
        let parent = tempfile::tempdir()?;
        let worktree = parent.path().join("base");
        repo.worktree_add_detached(&worktree, &first)?;
        assert_eq!(fs::read_to_string(worktree.join("marker.txt"))?, "here\n");
        assert_eq!(Git::new(&worktree).rev_parse("HEAD")?, first);
        assert_eq!(Git::new(&worktree).current_branch()?, None);

        repo.worktree_remove(&worktree)?;
        assert!(!worktree.exists());
        Ok(())
    }

    #[test]
    fn git_forgets_worktrees_whose_directory_is_gone() -> TestResult {
        let dir = init_repo()?;
        let path = dir.path();
        let first = commit(path, "feat: first")?;
        let repo = Git::new(path);
        let parent = tempfile::tempdir()?;
        let kept = parent.path().join("kept");
        let gone = parent.path().join("gone");
        repo.worktree_add_detached(&kept, &first)?;
        repo.worktree_add_detached(&gone, &first)?;
        fs::remove_dir_all(&gone)?;
        assert_eq!(git(path, &["worktree", "list"])?.lines().count(), 3);

        repo.worktree_prune()?;
        let listed = git(path, &["worktree", "list"])?;
        assert_eq!(listed.lines().count(), 2, "{listed}");
        assert!(!listed.contains("gone"), "{listed}");
        assert!(kept.exists());
        Ok(())
    }

    #[test]
    fn git_failures_carry_the_command_and_its_stderr() -> TestResult {
        let dir = init_repo()?;
        commit(dir.path(), "feat: first")?;
        let error = Git::new(dir.path())
            .rev_parse("no-such-ref")
            .err()
            .ok_or("expected an error")?;
        let Error::Command {
            program,
            args,
            status,
            stderr,
        } = error
        else {
            return Err("expected Error::Command".into());
        };
        assert_eq!(program, "git");
        assert!(args.contains(&"rev-parse".to_owned()), "{args:?}");
        assert!(args.contains(&"no-such-ref".to_owned()), "{args:?}");
        assert_eq!(status, Some(128));
        assert_ne!(stderr, "");
        Ok(())
    }

    #[test]
    fn git_outside_a_repository_is_an_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        let result = Git::new(dir.path()).rev_parse("HEAD");
        assert!(matches!(result, Err(Error::Command { .. })), "{result:?}");
        let result = Git::new(dir.path().join("does-not-exist")).rev_parse("HEAD");
        assert!(matches!(result, Err(Error::Command { .. })), "{result:?}");
        Ok(())
    }

    #[test]
    fn git_does_not_let_a_revision_become_an_option() -> TestResult {
        let dir = init_repo()?;
        commit(dir.path(), "feat: first")?;
        let repo = Git::new(dir.path());
        let output = dir.path().join("planted.txt");
        let flag = format!("--output={}", output.display());
        assert!(matches!(repo.rev_parse(&flag), Err(Error::Parse { .. })));
        assert!(matches!(
            repo.merge_base(&flag, "HEAD"),
            Err(Error::Parse { .. })
        ));
        assert!(matches!(
            repo.merge_base("HEAD", &flag),
            Err(Error::Parse { .. })
        ));
        assert!(matches!(repo.rev_list(&flag), Err(Error::Parse { .. })));
        assert!(matches!(repo.parents(&flag), Err(Error::Parse { .. })));
        assert!(matches!(repo.message(&flag), Err(Error::Parse { .. })));
        assert!(matches!(
            repo.diff_name_only_no_renames(&flag, "HEAD"),
            Err(Error::Parse { .. })
        ));
        assert!(matches!(
            repo.diff_raw("HEAD", &flag),
            Err(Error::Parse { .. })
        ));
        assert!(matches!(repo.show(&flag, "a"), Err(Error::Parse { .. })));
        assert!(matches!(
            repo.worktree_add_detached(&dir.path().join("w"), &flag),
            Err(Error::Parse { .. })
        ));
        assert!(!output.exists());
        Ok(())
    }
}
