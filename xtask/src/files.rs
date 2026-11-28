//! The files of the repository, for the checks that scan them.
//!
//! In a git work tree that is what git lists: tracked files and files it does not ignore, minus
//! tracked files that are deleted from disk. In a directory that is not a work tree, such as a
//! test fixture or an unpacked archive, it is a walk that skips `.git`, `target` and
//! `node_modules`. Paths are relative to the root and use `/`, and the list is sorted.

use std::fs;
use std::path::Path;

use crate::git::Git;
use crate::{Error, Result};

/// Names the walk does not enter or list, at any depth.
const SKIPPED: [&str; 3] = [".git", "target", "node_modules"];

/// The files under `root`, sorted, as paths relative to `root` joined with `/`.
///
/// # Errors
///
/// Fails when git cannot list the files or a directory cannot be read.
pub fn repo_files(root: &Path) -> Result<Vec<String>> {
    let git = Git::new(root);
    let mut files = if git.is_inside_work_tree() {
        let mut listed = git.ls_files()?;
        // `--cached` still lists a tracked file that was deleted from disk and not yet staged.
        listed.retain(|path| root.join(path).exists());
        listed
    } else {
        walk(root)?
    };
    files.sort();
    files.dedup();
    Ok(files)
}

/// Whether `path` (repo-relative, with `/`) is under a `tests/fixtures/` folder: test data that
/// holds deliberate violations, which no check reads as part of the repository.
#[must_use]
pub fn is_fixture(path: &str) -> bool {
    path.starts_with("tests/fixtures/") || path.contains("/tests/fixtures/")
}

/// The extension of `path`, such as `rs`.
#[must_use]
pub fn extension(path: &str) -> Option<&str> {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
}

fn walk(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    // Directories still to read, each with its path relative to the root as a prefix.
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let io_error = |source| Error::Io {
            path: dir.clone(),
            source,
        };
        for entry in fs::read_dir(&dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKIPPED.contains(&name.as_str()) {
                continue;
            }
            // `file_type` does not follow symlinks, so a link to a directory is listed, not
            // entered, and a cycle cannot trap the walk.
            if entry.file_type().map_err(io_error)?.is_dir() {
                pending.push((entry.path(), format!("{prefix}{name}/")));
            } else {
                files.push(format!("{prefix}{name}"));
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::{TestResult, commit, git, init_repo};
    use std::fs;
    use std::path::Path;

    fn write(root: &Path, relative: &str) -> TestResult {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, "x\n")?;
        Ok(())
    }

    #[test]
    fn repo_files_walks_non_git_dirs() -> TestResult {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for relative in [
            "a.rs",
            "src/b.rs",
            "sub/deep/c.ts",
            ".github/workflows/ci.yml",
            ".git/config",
            "target/debug/x.rs",
            "sub/target/y.rs",
            "node_modules/m/index.js",
            "sub/node_modules/z.js",
        ] {
            write(root, relative)?;
        }
        assert_eq!(
            repo_files(root)?,
            [
                ".github/workflows/ci.yml",
                "a.rs",
                "src/b.rs",
                "sub/deep/c.ts"
            ]
        );
        Ok(())
    }

    #[test]
    fn repo_files_in_a_git_work_tree_follows_git() -> TestResult {
        let dir = init_repo()?;
        let root = dir.path();
        fs::write(root.join(".gitignore"), "ignored.txt\ntarget/\n")?;
        for relative in ["tracked.txt", "dir/x.rs", "ignored.txt", "target/y.rs"] {
            write(root, relative)?;
        }
        git(root, &["add", ".gitignore", "tracked.txt", "dir/x.rs"])?;
        commit(root, "feat: start")?;
        write(root, "untracked.txt")?;
        write(root, "dir/untracked.rs")?;

        assert_eq!(
            repo_files(root)?,
            [
                ".gitignore",
                "dir/untracked.rs",
                "dir/x.rs",
                "tracked.txt",
                "untracked.txt"
            ]
        );
        Ok(())
    }

    #[test]
    fn repo_files_in_a_git_work_tree_lists_tracked_files_even_when_ignored_later() -> TestResult {
        let dir = init_repo()?;
        let root = dir.path();
        write(root, "build/kept.rs")?;
        commit(root, "feat: start")?;
        fs::write(root.join(".gitignore"), "build/\n")?;
        assert_eq!(repo_files(root)?, [".gitignore", "build/kept.rs"]);
        Ok(())
    }

    #[test]
    fn repo_files_skips_deleted_tracked_files() -> TestResult {
        let dir = init_repo()?;
        let root = dir.path();
        write(root, "kept.rs")?;
        write(root, "src/gone.rs")?;
        commit(root, "feat: start")?;
        fs::remove_file(root.join("src").join("gone.rs"))?;
        write(root, "fresh.rs")?;

        // `gone.rs` is still in the index, but the scanners must see the tree as it is.
        assert_eq!(git(root, &["ls-files", "src/gone.rs"])?, "src/gone.rs");
        assert_eq!(repo_files(root)?, ["fresh.rs", "kept.rs"]);
        Ok(())
    }

    #[test]
    fn repo_files_of_a_missing_directory_is_an_io_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        let result = repo_files(&dir.path().join("missing"));
        assert!(matches!(result, Err(crate::Error::Io { .. })), "{result:?}");
        Ok(())
    }
}
