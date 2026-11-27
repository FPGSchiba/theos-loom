//! Dep-info files: the list of input files rustc writes next to every artifact.
//!
//! Cargo runs rustc from the workspace root, so the paths in a dep-info file are relative to it
//! (or absolute, for files outside it). [`parse_dep_info`] reads the file's list,
//! [`repo_relative`](crate::repo_path::repo_relative) turns each path into the form the rest of the
//! gate uses, and [`dep_info_of`] says where the dep-info of an artifact is.

use std::path::{Path, PathBuf};

/// The files a dep-info text lists: the lines that end with `:` (rustc writes one such target
/// per input file, with no dependencies after it; the output lines have dependencies after the
/// colon), with a leading `\\?\` dropped, `\ ` read as a space, other backslashes as `/`, and
/// comment lines (`# env-dep:` and the like) skipped. Sorted and without duplicates.
#[must_use]
pub fn parse_dep_info(text: &str) -> Vec<String> {
    let mut files: Vec<String> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.strip_suffix(':'))
        .map(|path| path.strip_prefix(r"\\?\").unwrap_or(path))
        .filter(|path| !path.is_empty())
        .map(unescape)
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Reads rustc's one escape (`\ ` is a space), and takes every other backslash for a Windows
/// path separator.
fn unescape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
        } else if chars.next_if_eq(&' ').is_some() {
            out.push(' ');
        } else {
            out.push('/');
        }
    }
    out
}

/// The dep-info file of the artifact whose first file is `artifact_file`: the file beside it
/// named like it, without a leading `lib` and with the extension `.d` (`libxtask-<hash>.rmeta`
/// gives `xtask-<hash>.d`, `build_script_build-<hash>.exe` gives `build_script_build-<hash>.d`).
#[must_use]
pub fn dep_info_of(artifact_file: &Path) -> PathBuf {
    let stem = artifact_file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = stem.strip_prefix("lib").unwrap_or(&stem);
    artifact_file.with_file_name(format!("{stem}.d"))
}

/// The dep-info file of a build script whose files are in `folder`, which cargo names
/// `<package>-<hash>`: `build_script_build-<hash>.d` in it. The first file cargo reports for a
/// build script is a hard link without the hash (`build-script-build`), so [`dep_info_of`] cannot
/// find it from there. `None` when the folder's name has no hash.
#[must_use]
pub fn build_script_dep_info(folder: &Path) -> Option<PathBuf> {
    let name = folder.file_name()?.to_string_lossy();
    let (_, hash) = name.rsplit_once('-')?;
    (!hash.is_empty()).then(|| folder.join(format!("build_script_build-{hash}.d")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_path::repo_relative;

    #[test]
    fn files_are_the_targets_without_dependencies() {
        let text = "target\\debug\\deps\\xtask-0a1b.d: xtask\\src\\main.rs xtask\\src\\lib.rs\n\
                    \n\
                    target\\debug\\deps\\libxtask-0a1b.rmeta: xtask\\src\\main.rs\n\
                    \n\
                    xtask\\src\\main.rs:\n\
                    xtask\\src\\main.rs:\n\
                    c\\ d.rs:\n\
                    # env-dep:CARGO_PKG_NAME=xtask\n";
        assert_eq!(parse_dep_info(text), ["c d.rs", "xtask/src/main.rs"]);
    }

    #[test]
    fn rustc_escapes_only_spaces() {
        assert_eq!(parse_dep_info("crates\\#x\\a.rs:\n"), ["crates/#x/a.rs"]);
    }

    #[test]
    fn a_verbatim_line_reaches_repo_relative_without_its_prefix() {
        let root = std::env::temp_dir().join("repo");
        let line = format!(
            "\\\\?\\{}:\n",
            root.join("crates").join("a").join("data.txt").display()
        );
        let files = parse_dep_info(&line);
        assert_eq!(files.len(), 1);
        assert_eq!(repo_relative(&root, &files[0]), "crates/a/data.txt");
    }

    #[test]
    fn a_relative_root_is_not_a_root_directory() {
        assert_eq!(
            repo_relative(Path::new("."), "crates/a/x.rs"),
            "crates/a/x.rs"
        );
    }

    #[test]
    fn a_build_script_has_its_dep_info_in_its_folder_by_hash() {
        let folder = Path::new("target")
            .join("debug")
            .join("build")
            .join("my-pkg-0a1b");
        assert_eq!(
            build_script_dep_info(&folder),
            Some(folder.join("build_script_build-0a1b.d"))
        );
        assert_eq!(build_script_dep_info(Path::new("nohash")), None);
    }

    #[test]
    fn repo_relative_resolves_dots_and_strips_the_verbatim_prefix() {
        let root = std::env::temp_dir().join("repo");
        assert_eq!(
            repo_relative(&root, "crates/a/src/../x.rs"),
            "crates/a/x.rs"
        );
        assert_eq!(repo_relative(&root, "./crates/a/x.rs"), "crates/a/x.rs");
        assert_eq!(repo_relative(&root, "/elsewhere/x.rs"), "/elsewhere/x.rs");
        let inside = root.join("clippy.toml");
        assert_eq!(
            repo_relative(&root, &inside.to_string_lossy()),
            "clippy.toml"
        );
        if cfg!(windows) {
            let verbatim = format!(r"\\?\{}", inside.display());
            assert_eq!(repo_relative(&root, &verbatim), "clippy.toml");
            let shouting = inside.to_string_lossy().to_uppercase();
            assert_eq!(repo_relative(&root, &shouting), "CLIPPY.TOML");
        }
    }

    #[test]
    fn dep_info_path_strips_lib_and_the_extension() {
        let dir = Path::new("target").join("debug").join("deps");
        assert_eq!(
            dep_info_of(&dir.join("libxtask-0a1b.rmeta")),
            dir.join("xtask-0a1b.d")
        );
        assert_eq!(
            dep_info_of(&dir.join("build_script_build-0a1b.exe")),
            dir.join("build_script_build-0a1b.d")
        );
        assert_eq!(
            dep_info_of(&dir.join("liblibfoo-0a1b.rmeta")),
            dir.join("libfoo-0a1b.d")
        );
    }
}
