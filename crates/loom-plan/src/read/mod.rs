//! Reading a whole planning repo: list the files, classify each path, read each file into its
//! model, then put the tree together.

mod assemble;
mod classify;
mod collect;
mod join;
mod walk;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::repo::PlanningRepo;
use collect::Parts;

/// What stops reading a planning repo as a whole. Everything about file content is a
/// [`Finding`](crate::Finding) in [`PlanningRepo::read_findings`] instead.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The path is not a directory, or does not exist.
    #[error("{0} is not a directory")]
    NotADirectory(PathBuf),
    /// The root directory could not be listed.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The directory.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
}

/// Reads the planning repo at `root` into one [`PlanningRepo`].
///
/// A file that does not parse is a finding, never an abort, so one bad file does not hide the
/// others.
///
/// # Errors
/// [`ReadError`] when `root` is not a readable directory.
pub fn read(root: &Path) -> Result<PlanningRepo, ReadError> {
    if !fs::metadata(root).is_ok_and(|m| m.is_dir()) {
        return Err(ReadError::NotADirectory(root.to_path_buf()));
    }
    let mut parts = Parts::default();
    let files = walk::walk(root, &mut parts.findings)?;
    let components: BTreeSet<String> = files
        .iter()
        .filter_map(|f| match f.rel.split('/').collect::<Vec<_>>().as_slice() {
            ["components", id, "component.toml"] => Some((*id).to_owned()),
            _ => None,
        })
        .collect();
    for file in files {
        let _span = tracing::debug_span!("read_file", path = %file.rel).entered();
        let segments: Vec<&str> = file.rel.split('/').collect();
        let kind = classify::classify(&segments, &components);
        tracing::debug!(?kind, "classified");
        parts.touch(&segments, &components);
        match fs::read(&file.abs) {
            Ok(bytes) => parts.add(&file.rel, kind, &bytes),
            Err(err) => {
                parts
                    .findings
                    .push(crate::parse::parse_error(&file.rel, &err.to_string(), None));
            }
        }
    }
    Ok(assemble::assemble(root.to_path_buf(), parts))
}
