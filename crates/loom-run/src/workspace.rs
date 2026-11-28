//! The per-run local clone of a shared mirror (spec §5.3 step 2, D3).
//!
//! One mirror per repository lives under `<data_dir>/mirrors`, fetched before each run. Each run
//! gets its own `git clone --local` of it under `<data_dir>/runs/<run>/work`, so no two runs ever
//! share a worktree.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use loom_core::run::RunId;
use thiserror::Error;

use crate::config::{RunConfig, RunSpec};
use crate::git::{self, GitError};
use crate::ident::short_hash;
use crate::locks;

/// The clone could not be made.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// A git command failed.
    #[error(transparent)]
    Git(#[from] GitError),
    /// A directory could not be created or removed.
    #[error("{what} {path}: {source}")]
    Io {
        /// What was being done.
        what: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// A branch name that git would read as an option.
    #[error("invalid branch name {0:?}")]
    BadBranch(String),
}

/// A run's own clone of the story's repository.
#[derive(Debug)]
pub struct Workspace {
    run_dir: PathBuf,
    work: PathBuf,
}

impl Workspace {
    /// Fetches the mirror, clones it for `run` and checks out the story branch.
    ///
    /// The story branch is created from `origin/<default_branch>` when it does not exist yet.
    /// `origin` of the clone points at `spec.repo_url`, so a push goes to the real repository.
    ///
    /// # Errors
    /// [`WorkspaceError`] when a git command or a directory operation fails; a half-made run
    /// directory is removed.
    pub async fn prepare(
        config: &RunConfig,
        run: RunId,
        spec: &RunSpec,
    ) -> Result<Self, WorkspaceError> {
        for branch in [&spec.story_branch, &spec.default_branch] {
            if branch.is_empty() || branch.starts_with('-') {
                return Err(WorkspaceError::BadBranch(branch.clone()));
            }
        }
        let mirror = config
            .data_dir
            .join("mirrors")
            .join(format!("{}.git", short_hash(&spec.repo_url)));
        let run_dir = config.data_dir.join("runs").join(run.to_string());
        let work = run_dir.join("work");

        if let Err(error) = clone_for_run(&mirror, &run_dir, &work, spec).await {
            if let Err(cleanup) = tokio::fs::remove_dir_all(&run_dir).await {
                tracing::warn!(%cleanup, run_dir = %run_dir.display(), "could not remove a failed run directory");
            }
            return Err(error);
        }
        Ok(Self { run_dir, work })
    }

    /// The clone's directory, the worktree of the run.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.work
    }

    /// Deletes the run directory, the clone included.
    ///
    /// # Errors
    /// [`WorkspaceError::Io`] when it cannot be deleted.
    pub async fn remove(self) -> Result<(), WorkspaceError> {
        tokio::fs::remove_dir_all(&self.run_dir)
            .await
            .map_err(|source| WorkspaceError::Io {
                what: "remove",
                path: self.run_dir.clone(),
                source,
            })
    }
}

async fn clone_for_run(
    mirror: &Path,
    run_dir: &Path,
    work: &Path,
    spec: &RunSpec,
) -> Result<(), WorkspaceError> {
    let parent = mirror.parent().unwrap_or(mirror);
    for dir in [parent, run_dir] {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|source| WorkspaceError::Io {
                what: "create",
                path: dir.to_owned(),
                source,
            })?;
    }
    let url = OsStr::new(&spec.repo_url);
    {
        // The fetch changes the mirror and the clone reads it: hold the lock for both.
        let _guard = locks::lock(mirror).await;
        if mirror.exists() {
            git::run(
                Some(mirror),
                &["fetch".as_ref(), "--prune".as_ref(), "--quiet".as_ref()],
            )
            .await?;
        } else {
            let args = [
                "clone".as_ref(),
                "--mirror".as_ref(),
                "--quiet".as_ref(),
                "--".as_ref(),
                url,
                mirror.as_os_str(),
            ];
            git::run(None, &args).await?;
        }
        let args = [
            "clone".as_ref(),
            "--local".as_ref(),
            "--quiet".as_ref(),
            "--".as_ref(),
            mirror.as_os_str(),
            work.as_os_str(),
        ];
        git::run(None, &args).await?;
    }
    let origin_branch = format!("refs/remotes/origin/{}", spec.story_branch);
    let exists = git::run_str(
        Some(work),
        &["rev-parse", "--verify", "--quiet", &origin_branch],
    )
    .await
    .is_ok();
    if exists {
        git::run_str(Some(work), &["switch", "--quiet", &spec.story_branch]).await?;
    } else {
        let start = format!("origin/{}", spec.default_branch);
        git::run_str(
            Some(work),
            &["switch", "--quiet", "-c", &spec.story_branch, &start],
        )
        .await?;
    }
    git::run(
        Some(work),
        &[
            "remote".as_ref(),
            "set-url".as_ref(),
            "origin".as_ref(),
            url,
        ],
    )
    .await?;
    Ok(())
}
