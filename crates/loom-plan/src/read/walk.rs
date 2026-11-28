//! Listing the files of a planning repo (R-f): std `read_dir`, no symlink following, sorted.

use std::fs;
use std::path::{Path, PathBuf};

use super::ReadError;
use crate::finding::{Finding, Rule};

/// One file to read.
#[derive(Debug, Clone)]
pub(crate) struct WalkedFile {
    /// Repo-relative path with `/` separators.
    pub rel: String,
    /// Where to read it.
    pub abs: PathBuf,
}

/// The file every dotfile but this one is skipped for.
const MEMLOG: &str = ".memlog.md";
/// The kit's own folder (R-f).
const BMAD: &str = "_bmad";

/// Every file under `root`, sorted by path. `.git`, `_bmad` and dotfiles other than
/// `.memlog.md` are skipped, symlinks are never followed. A folder below the root that cannot be
/// listed is a finding; the root itself is a [`ReadError`].
pub(crate) fn walk(root: &Path, findings: &mut Vec<Finding>) -> Result<Vec<WalkedFile>, ReadError> {
    let mut files = Vec::new();
    let top = entries(root).map_err(|source| ReadError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut stack = vec![(String::new(), top)];
    while let Some((prefix, list)) = stack.pop() {
        for (name, path, kind) in list {
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() {
                if name.starts_with('.') || name == BMAD {
                    continue;
                }
                match entries(&path) {
                    Ok(inner) => stack.push((rel, inner)),
                    Err(err) => findings.push(Finding::error(
                        Rule::Parse,
                        &rel,
                        format!("cannot list the folder {rel}: {err}"),
                    )),
                }
            } else if kind.is_file() && (!name.starts_with('.') || name == MEMLOG) {
                files.push(WalkedFile { rel, abs: path });
            }
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

/// The entries of one folder in name order, with their own (not followed) file type.
fn entries(dir: &Path) -> std::io::Result<Vec<(String, PathBuf, fs::FileType)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        out.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
            kind,
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
