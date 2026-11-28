//! Lexical paths: where a file is, read from its text alone, and whether it lies under a root.
//!
//! The view (dep-info files) and `coverage` (llvm-cov exports) both get paths as text from tools
//! that run elsewhere or print another operating system's spelling: relative to the workspace,
//! with `.` and `..`, with a `\\?\` prefix, in another case on Windows. Nothing is canonicalised,
//! because the file may not exist on this machine or may be reached through a link; the paths are
//! compared as components. This module is the one place that does it.

use std::path::{Component, Path};

/// What a `..` does when there is nothing before it to remove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Climb {
    /// It goes nowhere, as `..` at the top of a directory tree does.
    Drop,
    /// It stays in the path, so a path that leaves its start is never taken for a path inside it.
    Keep,
}

/// How file names are compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    /// Byte for byte.
    Exact,
    /// Ignoring ASCII case.
    Ascii,
    /// Ignoring case as Unicode lower-casing sees it.
    Unicode,
}

impl Case {
    /// How this operating system compares file names: Windows ignores case.
    #[must_use]
    pub fn of_os() -> Self {
        if cfg!(windows) {
            Self::Unicode
        } else {
            Self::Exact
        }
    }

    /// Whether the names `a` and `b` are the same.
    #[must_use]
    pub fn same(self, a: &str, b: &str) -> bool {
        match self {
            Self::Exact => a == b,
            Self::Ascii => a.eq_ignore_ascii_case(b),
            Self::Unicode => a.to_lowercase() == b.to_lowercase(),
        }
    }
}

/// Where a path lexically is: its prefix (a drive, empty when there is none), whether it starts
/// at a root, and its segments with `.` and `..` resolved.
#[derive(Debug)]
pub struct RepoPath {
    prefix: String,
    rooted: bool,
    segments: Vec<String>,
}

impl RepoPath {
    /// The lexical form of `path`, split by the separators of this operating system, with a
    /// `\\?\` prefix dropped.
    #[must_use]
    pub fn of(path: &str, climb: Climb) -> Self {
        let mut normal = Self {
            prefix: String::new(),
            rooted: false,
            segments: Vec::new(),
        };
        for component in Path::new(path).components() {
            match component {
                Component::Prefix(prefix) => {
                    prefix
                        .as_os_str()
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .clone_into(&mut normal.prefix);
                }
                Component::RootDir => normal.rooted = true,
                Component::CurDir => {}
                Component::ParentDir => {
                    let removable = normal.segments.last().is_some_and(|last| last != "..");
                    if removable || climb == Climb::Drop {
                        normal.segments.pop();
                    } else {
                        normal.segments.push("..".to_owned());
                    }
                }
                Component::Normal(segment) => {
                    normal.segments.push(segment.to_string_lossy().into_owned());
                }
            }
        }
        normal
    }

    /// Whether the path starts at a root or a drive, so that it does not depend on a start.
    #[must_use]
    pub fn is_absolute(&self) -> bool {
        self.rooted || !self.prefix.is_empty()
    }

    /// The path as text, with `/`.
    #[must_use]
    pub fn text(&self) -> String {
        let root = if self.rooted { "/" } else { "" };
        format!("{}{root}{}", self.prefix, self.segments.join("/"))
    }

    /// The segments of this path below `root`, or `None` when it is not under it.
    #[must_use]
    pub fn below(&self, root: &Self, case: Case) -> Option<&[String]> {
        let inside = case.same(&self.prefix, &root.prefix)
            && self.rooted == root.rooted
            && self.segments.len() >= root.segments.len()
            && root
                .segments
                .iter()
                .zip(&self.segments)
                .all(|(a, b)| case.same(a, b));
        inside.then(|| &self.segments[root.segments.len()..])
    }
}

/// `file` as a path relative to `root`, with `/` as separator, resolved lexically: a relative
/// path is taken from `root`, a `\\?\` prefix is dropped, `.` and `..` are resolved by
/// components, case is ignored on Windows, and nothing is canonicalised. A file outside `root`
/// keeps its absolute normal form.
#[must_use]
pub fn repo_relative(root: &Path, file: &str) -> String {
    let root = RepoPath::of(&root.to_string_lossy(), Climb::Drop);
    let mut normal = RepoPath::of(file, Climb::Drop);
    if !normal.is_absolute() && !root.text().is_empty() {
        normal = RepoPath::of(&format!("{}/{file}", root.text()), Climb::Drop);
    }
    match normal.below(&root, Case::of_os()) {
        Some(segments) => segments.join("/"),
        None => normal.text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_are_resolved_by_component() {
        let text = |path: &str, climb| RepoPath::of(path, climb).text();
        assert_eq!(text("a/./b/../c", Climb::Drop), "a/c");
        assert_eq!(text("a/./b/../c", Climb::Keep), "a/c");
        assert_eq!(text("a//b/", Climb::Keep), "a/b");
    }

    #[test]
    fn a_parent_with_nothing_to_remove_is_dropped_or_kept() {
        let text = |path: &str, climb| RepoPath::of(path, climb).text();
        assert_eq!(text("../../a", Climb::Drop), "a");
        assert_eq!(text("../../a", Climb::Keep), "../../a");
        assert_eq!(text("a/../../b", Climb::Keep), "../b");
        assert_eq!(text("/..", Climb::Keep), "/..");
        assert_eq!(text("/..", Climb::Drop), "/");
    }

    #[test]
    fn case_follows_the_mode() {
        assert!(Case::Exact.same("a", "a"));
        assert!(!Case::Exact.same("a", "A"));
        assert!(Case::Ascii.same("Src", "sRC"));
        assert!(!Case::Ascii.same("É", "é"));
        assert!(Case::Unicode.same("É", "é"));
        assert_eq!(Case::of_os() == Case::Unicode, cfg!(windows));
    }

    #[test]
    fn below_gives_the_segments_under_a_root() {
        let root = RepoPath::of("/r/repo", Climb::Drop);
        let file = RepoPath::of("/r/repo/a/../b/c.rs", Climb::Drop);
        assert_eq!(
            file.below(&root, Case::Exact),
            Some(&["b".to_owned(), "c.rs".to_owned()][..])
        );
        let other = RepoPath::of("/r/repository/c.rs", Climb::Drop);
        assert_eq!(other.below(&root, Case::Exact), None);
        let upper = RepoPath::of("/R/Repo/c.rs", Climb::Drop);
        assert_eq!(upper.below(&root, Case::Exact), None);
        assert!(upper.below(&root, Case::Ascii).is_some());
        let relative = RepoPath::of("r/repo/c.rs", Climb::Drop);
        assert_eq!(relative.below(&root, Case::Exact), None);
    }

    #[test]
    fn a_relative_path_is_taken_from_the_root_and_a_path_outside_stays_whole() {
        let root = std::env::temp_dir().join("repo");
        assert_eq!(repo_relative(&root, "crates/a/../b/x.rs"), "crates/b/x.rs");
        let outside = std::env::temp_dir().join("elsewhere").join("x.rs");
        let text = repo_relative(&root, &outside.to_string_lossy());
        assert!(text.ends_with("elsewhere/x.rs"), "{text}");
        assert!(RepoPath::of(&text, Climb::Drop).is_absolute(), "{text}");
    }
}
