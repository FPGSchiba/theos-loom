//! The workflow kit at its pinned version (spec §5.3 step 3).
//!
//! Each version is cloned once to `<data_dir>/kits/<hash of repository>/<version>`, then its
//! files are made read-only: a run mounts the kit read-only, and nothing edits it on the host.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::{KitRef, RunConfig};
use crate::git::{self, GitError};
use crate::ident::short_hash;
use crate::locks;

/// The kit could not be prepared.
#[derive(Debug, Error)]
pub enum KitError {
    /// A git command failed.
    #[error(transparent)]
    Git(#[from] GitError),
    /// A file operation failed.
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
    /// The version would leave the kit directory or be read as a git option.
    #[error("invalid kit version {0:?}")]
    BadVersion(String),
}

/// A checkout of the kit.
#[derive(Debug, Clone)]
pub struct Kit {
    path: PathBuf,
}

impl Kit {
    /// Makes sure the checkout of `kit` exists, cloning it once with `--depth 1`.
    ///
    /// # Errors
    /// [`KitError`] for a bad version, a failed git command or a failed file operation.
    pub async fn prepare(config: &RunConfig, kit: &KitRef) -> Result<Self, KitError> {
        let version = &kit.version;
        if version.is_empty()
            || version.starts_with('-')
            || version == ".."
            || version.contains(['/', '\\'])
        {
            return Err(KitError::BadVersion(version.clone()));
        }
        let base = config
            .data_dir
            .join("kits")
            .join(short_hash(&kit.repository));
        let path = base.join(version);
        let _guard = locks::lock(&path).await;
        if path.is_dir() {
            return Ok(Self { path });
        }
        tokio::fs::create_dir_all(&base)
            .await
            .map_err(|source| KitError::Io {
                what: "create",
                path: base.clone(),
                source,
            })?;
        // Clone beside the target and rename, so a crash never leaves a half checkout there.
        let partial = base.join(format!("{version}.partial"));
        if partial.exists() {
            remove_dir(&partial).await?;
        }
        let args = [
            "clone".as_ref(),
            "--quiet".as_ref(),
            "--depth".as_ref(),
            "1".as_ref(),
            "--branch".as_ref(),
            OsStr::new(version),
            "--".as_ref(),
            OsStr::new(&kit.repository),
            partial.as_os_str(),
        ];
        if let Err(error) = git::run(None, &args).await {
            // Best effort: the error that matters is the clone's.
            let _ = remove_dir(&partial).await;
            return Err(error.into());
        }
        make_read_only(&partial).await?;
        tokio::fs::rename(&partial, &path)
            .await
            .map_err(|source| KitError::Io {
                what: "rename",
                path: partial.clone(),
                source,
            })?;
        Ok(Self { path })
    }

    /// The checkout's directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

async fn remove_dir(path: &Path) -> Result<(), KitError> {
    tokio::fs::remove_dir_all(path)
        .await
        .map_err(|source| KitError::Io {
            what: "remove",
            path: path.to_owned(),
            source,
        })
}

/// Marks every file below `root` read-only. Directories stay writable, so the tree can still be
/// deleted on Windows and Unix alike.
async fn make_read_only(root: &Path) -> Result<(), KitError> {
    let owned = root.to_owned();
    match tokio::task::spawn_blocking(move || read_only_tree(&owned)).await {
        Ok(result) => result,
        Err(join) => Err(KitError::Io {
            what: "make read-only",
            path: root.to_owned(),
            source: std::io::Error::other(join),
        }),
    }
}

fn read_only_tree(dir: &Path) -> Result<(), KitError> {
    let io = |what, path: &Path| {
        let path = path.to_owned();
        move |source| KitError::Io { what, path, source }
    };
    for entry in std::fs::read_dir(dir).map_err(io("read", dir))? {
        let entry = entry.map_err(io("read", dir))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(io("stat", &path))?;
        if kind.is_dir() {
            read_only_tree(&path)?;
        } else if kind.is_file() {
            let mut permissions = std::fs::metadata(&path)
                .map_err(io("stat", &path))?
                .permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&path, permissions).map_err(io("chmod", &path))?;
        }
    }
    Ok(())
}
