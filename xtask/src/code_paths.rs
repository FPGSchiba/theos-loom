//! `gate/code-paths.toml`: which changed paths can change which part's verdict (P00 spec §5.3).
//!
//! The file has three lists of globs, read by `globset` with `literal_separator(true)` the way
//! `.loom/protected.toml` is (one builder, `glob_set`): `*` stays inside a folder, `**` crosses
//! folders. Paths are repo-relative with `/` on every OS.
//!
//! - `non_code` matches exactly: paths that are not code. Everything else is code.
//! - `always_code` ignores letter case and wins over `non_code`: paths that are code wherever
//!   they are, because a part reads them.
//! - `ui` matches exactly: the paths that can change the verdict of the `ui` part.
//!
//! A missing or broken file is an [`Error::Config`], never a smaller run.

use std::fs;
use std::path::Path;

use globset::GlobSet;
use serde::Deserialize;

use crate::checks::approval::protected::glob_set;
use crate::checks::lints;
use crate::{Error, Result};

/// The lists, relative to the root of the checkout.
pub const FILE: &str = "gate/code-paths.toml";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lists {
    non_code: Vec<String>,
    always_code: Vec<String>,
    ui: Vec<String>,
}

/// The three lists of `gate/code-paths.toml`, ready to match.
#[derive(Debug)]
pub struct CodePaths {
    lists: [Vec<String>; 3],
    non_code: GlobSet,
    always_code: GlobSet,
    ui: GlobSet,
}

impl CodePaths {
    /// Reads `gate/code-paths.toml` of the checkout at `root`.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when the file cannot be read, is not TOML of the expected shape, or has
    /// a pattern that is not a glob.
    pub fn load(root: &Path) -> Result<Self> {
        let text = fs::read_to_string(root.join(FILE))
            .map_err(|source| config_error(format!("cannot be read: {source}")))?;
        Self::parse(&text)
    }

    /// Reads the lists from the text of `gate/code-paths.toml`.
    ///
    /// # Errors
    ///
    /// The same as [`CodePaths::load`], but for reading the file.
    pub fn parse(text: &str) -> Result<Self> {
        // The TOML error would quote the line, so only its place is passed on.
        let table = lints::parse(FILE, text).map_err(|error| match error {
            Error::Parse { message, .. } => config_error(message),
            other => other,
        })?;
        let lists: Lists = table.try_into().map_err(|_| {
            config_error(
                "has an unexpected shape: `non_code`, `always_code` and `ui` are lists of patterns",
            )
        })?;
        Ok(Self {
            non_code: glob_set(FILE, "non_code", &lists.non_code, false)?,
            always_code: glob_set(FILE, "always_code", &lists.always_code, true)?,
            ui: glob_set(FILE, "ui", &lists.ui, false)?,
            lists: [lists.non_code, lists.always_code, lists.ui],
        })
    }

    /// Whether every path is non-code and none is always-code. No paths is not docs-only.
    #[must_use]
    pub fn is_docs_only(&self, paths: &[String]) -> bool {
        !paths.is_empty()
            && paths
                .iter()
                .all(|path| self.non_code.is_match(path) && !self.always_code.is_match(path))
    }

    /// Whether any path is on the `ui` list.
    #[must_use]
    pub fn touches_ui(&self, paths: &[String]) -> bool {
        paths.iter().any(|path| self.ui.is_match(path))
    }

    /// The `non_code` patterns as written.
    #[must_use]
    pub fn non_code(&self) -> &[String] {
        &self.lists[0]
    }

    /// The `always_code` patterns as written.
    #[must_use]
    pub fn always_code(&self) -> &[String] {
        &self.lists[1]
    }

    /// The `ui` patterns as written.
    #[must_use]
    pub fn ui(&self) -> &[String] {
        &self.lists[2]
    }
}

fn config_error(message: impl Into<String>) -> Error {
    Error::Config {
        file: FILE.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Result<CodePaths> {
        CodePaths::parse(include_str!("../../gate/code-paths.toml"))
    }

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    // AC: TU-00.3
    #[test]
    fn the_real_lists_classify_every_input_a_part_reads() -> Result<()> {
        let lists = real()?;
        let code = [
            "docs/specs/00-umbrella.md",
            "docs/gate.md",
            "docs/specs/phases/P00-foundation-and-gate.md",
            "CONTRIBUTING.md",
            ".github/workflows/ci.yml",
            ".loom/protected.toml",
            ".loom/checks.toml",
            "gate/tools.toml",
            "web/src/App.tsx",
            "notes/x.ts",
            "Cargo.toml",
            "crates/a/Cargo.toml",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            "deny.toml",
            ".gitleaksignore",
        ];
        for path in code {
            assert!(!lists.is_docs_only(&paths(&[path])), "{path} is code");
        }
        let non_code = [
            "docs/plans/P01-x.md",
            "docs/trial/P01-x.md",
            "docs/decisions/0005-x.md",
            "README.md",
            "CLAUDE.md",
            ".claude/skills/x/SKILL.md",
        ];
        for path in non_code {
            assert!(lists.is_docs_only(&paths(&[path])), "{path} is not code");
        }
        Ok(())
    }

    #[test]
    fn the_lists_are_kept_as_written_and_empty_paths_are_not_docs_only() -> Result<()> {
        let lists = real()?;
        assert!(lists.non_code().contains(&"docs/**".to_owned()));
        assert!(lists.always_code().contains(&"**/*.ts".to_owned()));
        assert!(lists.ui().contains(&"web/**".to_owned()));
        assert!(!lists.is_docs_only(&[]));
        Ok(())
    }

    #[test]
    fn a_broken_file_is_a_config_error_naming_it() {
        let texts = [
            "",
            "non_code = 3",
            "non_code = [\"[\"]\nalways_code = []\nui = []",
        ];
        for text in texts {
            let error = CodePaths::parse(text).err();
            assert!(
                matches!(&error, Some(Error::Config { file, .. }) if file == FILE),
                "{text:?}: {error:?}"
            );
        }
    }
}
