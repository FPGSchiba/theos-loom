//! Which changes of a pull request are protected: `.loom/protected.toml` (`docs/gate.md` §5).
//!
//! A pull request is protected when it changes a path that matches `protected` and not `except`,
//! or when a table listed under `tables` (the `[workspace.lints]` of the root `Cargo.toml`) is not
//! the same in the head as in the merge base. Everything about the pull request is read from git
//! objects with `git diff` and `git show`, never from a work tree.
//!
//! - Globs are read by `globset` with `literal_separator(true)`: `*` stays inside a folder and
//!   `docs/**` reaches every depth.
//! - `protected` patterns ignore case, because `DOCS/x.md` is the folder `docs` on Windows and on
//!   a Mac. `except` patterns do not: a path that only differs in case from an exception is not
//!   excepted, and stays protected.
//! - A rename counts as the old path and the new path, so moving a file out of `docs/` still
//!   touches `docs/` ([`Git::diff_name_only_no_renames`]).
//! - Tables are compared as parsed TOML, so a reformatted, reordered or moved table is not a
//!   change. A table that cannot be read on either side is a change: when in doubt it is
//!   protected.
//!
//! Errors and findings never quote the file or a path: in the one case where the rules come from
//! the pull request itself (the P00 build PR, which has no `main` gate to run), the file is the
//! pull request's text.

use std::fs;
use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::checks::lints;
use crate::git::Git;
use crate::{Error, Result};

/// The rules file, relative to the root of the checkout the check runs from.
pub const CONFIG: &str = ".loom/protected.toml";

/// A table inside a file: the file, and the dotted path of the table in it.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableRule {
    /// The file, repo-relative with `/`, such as `Cargo.toml`.
    pub file: String,
    /// The dotted path of the table, such as `workspace.lints`.
    pub table: String,
}

/// The paths and tables that wait for Jann.
#[derive(Debug)]
pub struct ProtectedPaths {
    protected: GlobSet,
    except: GlobSet,
    tables: Vec<TableRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rules {
    protected: Vec<String>,
    #[serde(default)]
    except: Vec<String>,
    #[serde(default)]
    tables: Vec<TableRule>,
}

impl ProtectedPaths {
    /// Reads `.loom/protected.toml` of the checkout at `root`.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read, is not TOML of the expected shape, lists no protected
    /// pattern or has a pattern that is not a glob.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CONFIG);
        let text = fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
        Self::parse(&text)
    }

    /// Reads the rules from the text of `.loom/protected.toml`.
    ///
    /// # Errors
    ///
    /// The same as [`ProtectedPaths::load`], but for reading the file.
    pub fn parse(text: &str) -> Result<Self> {
        // The TOML error would quote the line, so only its place is passed on.
        let table = lints::parse(CONFIG, text).map_err(|error| match error {
            Error::Parse { message, .. } => config_error(message),
            other => other,
        })?;
        let rules: Rules = table.try_into().map_err(|_| {
            config_error(
                "has an unexpected shape: `protected` and `except` are lists of patterns, \
                 `tables` is a list of `file` and `table`",
            )
        })?;
        if rules.protected.is_empty() {
            return Err(config_error(
                "has no protected pattern, so nothing would wait for Jann",
            ));
        }
        Ok(Self {
            protected: glob_set(CONFIG, "protected", &rules.protected, true)?,
            except: glob_set(CONFIG, "except", &rules.except, false)?,
            tables: rules.tables,
        })
    }

    /// Whether changing `path` (repo-relative, with `/`) waits for Jann: it matches `protected`
    /// and not `except`.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        self.protected.is_match(path) && !self.except.is_match(path)
    }

    /// The tables that make a pull request protected when they change.
    #[must_use]
    pub fn tables(&self) -> &[TableRule] {
        &self.tables
    }
}

/// The protected things the changes from `merge_base` to `head` touch, for changed paths that
/// are already listed (by `changes::of_pull_request`): the protected paths, then one entry
/// `<file> [<table>]` for each table that is not the same on both sides. Empty when the pull
/// request is not protected.
///
/// # Errors
///
/// Fails when either revision is unknown or git cannot be run.
pub fn touched_in(
    git: &Git,
    rules: &ProtectedPaths,
    paths: &[String],
    merge_base: &str,
    head: &str,
) -> Result<Vec<String>> {
    let mut touched: Vec<String> = paths
        .iter()
        .filter(|path| rules.matches(path))
        .cloned()
        .collect();
    for rule in &rules.tables {
        if table_changed(git, rule, merge_base, head)? {
            touched.push(format!("{} [{}]", rule.file, rule.table));
        }
    }
    Ok(touched)
}

/// Whether the table of `rule` differs between `merge_base` and `head`. Identical text is the
/// same table; otherwise the parsed tables are compared, and a file that cannot be read as TOML
/// on either side counts as a change.
fn table_changed(git: &Git, rule: &TableRule, merge_base: &str, head: &str) -> Result<bool> {
    let before = git.show(merge_base, &rule.file)?;
    let after = git.show(head, &rule.file)?;
    if before == after {
        return Ok(false);
    }
    Ok(
        match (
            lookup(before.as_deref(), rule),
            lookup(after.as_deref(), rule),
        ) {
            (Lookup::Unreadable, _) | (_, Lookup::Unreadable) => true,
            (before, after) => before != after,
        },
    )
}

/// What a file says about a table.
#[derive(PartialEq)]
enum Lookup {
    /// The file is not there, or has no such table.
    Absent,
    /// The table, as parsed.
    Found(toml::Value),
    /// The file is not TOML.
    Unreadable,
}

/// The table of `rule` in `text`, the content of its file.
fn lookup(text: Option<&str>, rule: &TableRule) -> Lookup {
    let Some(text) = text else {
        return Lookup::Absent;
    };
    let Ok(file) = lints::parse(&rule.file, text) else {
        return Lookup::Unreadable;
    };
    let file = toml::Value::Table(file);
    rule.table
        .split('.')
        .try_fold(&file, |value, key| value.get(key))
        .map_or(Lookup::Absent, |table| Lookup::Found(table.clone()))
}

/// The globs of one list in the file `file`. A bad pattern is named by its place in the list, not
/// quoted. `gate/code-paths.toml` builds its lists the same way.
pub(crate) fn glob_set(
    file: &str,
    list: &str,
    patterns: &[String],
    ignore_case: bool,
) -> Result<GlobSet> {
    let error = |message: String| Error::Config {
        file: file.to_owned(),
        message,
    };
    let mut set = GlobSetBuilder::new();
    for (index, pattern) in patterns.iter().enumerate() {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(true)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|_| {
                error(format!(
                    "`{list}` pattern {} is not a valid glob",
                    index + 1
                ))
            })?;
        set.add(glob);
    }
    set.build()
        .map_err(|_| error(format!("`{list}` cannot be read as globs")))
}

fn config_error(message: impl Into<String>) -> Error {
    Error::Config {
        file: CONFIG.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Result<ProtectedPaths> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent();
        ProtectedPaths::load(root.unwrap_or_else(|| Path::new(".")))
    }

    fn rules(text: &str) -> Result<ProtectedPaths> {
        ProtectedPaths::parse(text)
    }

    // AC: TU-00.4
    #[test]
    fn docs_protected_plans_and_trial_not() -> Result<()> {
        let rules = real()?;
        assert!(rules.matches("docs/gate.md"));
        assert!(rules.matches("docs/specs/00-umbrella.md"));
        assert!(!rules.matches("docs/plans/P00-x.md"));
        assert!(!rules.matches("docs/plans/deeper/P00-x.md"));
        assert!(!rules.matches("docs/trial/P00-x.md"));
        assert!(!rules.matches("docs/trial/deeper/P00-x.md"));
        // Only `docs/trial/**` is excepted, not a file of that name somewhere else in `docs`.
        assert!(rules.matches("docs/specs/trial/P00-x.md"));
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn case_variants_are_protected() -> Result<()> {
        let rules = real()?;
        for path in [
            "DOCS/x.md",
            "Docs/Gate.md",
            "Claude.md",
            "claude.MD",
            "XTASK/src/main.rs",
            ".GitHub/workflows/ci.yml",
            ".Loom/protected.toml",
            "Deny.toml",
            ".Cargo/config.toml",
            ".GitLeaksIgnore",
        ] {
            assert!(rules.matches(path), "{path}");
        }
        // The exceptions are case-sensitive, so a case variant of one is not excepted.
        assert!(rules.matches("DOCS/plans/P00-x.md"));
        assert!(rules.matches("docs/Plans/P00-x.md"));
        assert!(rules.matches("docs/Trial/P00-x.md"));
        assert!(rules.matches("docs/TRIAL/P00-x.md"));
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn star_does_not_cross_slash() -> Result<()> {
        let rules = rules("protected = [\"a/*.md\", \"b/**\"]\nexcept = [\"b/*.tmp\"]")?;
        assert!(rules.matches("a/x.md"));
        assert!(!rules.matches("a/deeper/x.md"));
        assert!(!rules.matches("x.md"));
        // `**` crosses folders, `*` in an exception does not.
        assert!(rules.matches("b/deeper/x.md"));
        assert!(!rules.matches("b/x.tmp"));
        assert!(rules.matches("b/deeper/x.tmp"));
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn the_real_rules_leave_ordinary_code_alone() -> Result<()> {
        let rules = real()?;
        for path in [
            "crates/loom-core/src/lib.rs",
            "web/src/App.tsx",
            "web/package.json",
            "Cargo.toml",
            "Cargo.lock",
            "README.md",
            "docs/plans/P01-x.md",
            "docs/trial/P01-x.md",
            "docs",
            "xtask-notes.md",
            "CLAUDE.mdx",
            "clippy.toml",
        ] {
            assert!(!rules.matches(path), "{path}");
        }
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn the_real_rules_protect_the_gate() -> Result<()> {
        let rules = real()?;
        for path in [
            "CLAUDE.md",
            ".claude/skills/x/SKILL.md",
            "CONTRIBUTING.md",
            "docs/gate.md",
            "docs/specs/00-umbrella.md",
            "docs/specs/phases/P01-x.md",
            "docs/decisions/0005-x.md",
            ".github/workflows/ci.yml",
            "gate/tools.toml",
            "xtask/src/lib.rs",
            "deny.toml",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            ".loom/checks.toml",
            ".loom/protected.toml",
            "web/eslint.config.js",
            "web/src/eslint.config.mjs",
            "web/tsconfig.app.json",
            "web/vite.config.ts",
            "web/vitest.config.ts",
            "web/vitest.workspace.ts",
            ".gitleaksignore",
            ".gitleaks.toml",
            "Docs/Gate.md",
        ] {
            assert!(rules.matches(path), "{path}");
        }
        assert_eq!(
            rules.tables(),
            [TableRule {
                file: "Cargo.toml".to_owned(),
                table: "workspace.lints".to_owned()
            }]
        );
        Ok(())
    }

    #[test]
    fn rules_without_a_protected_pattern_or_with_a_bad_one_are_refused() {
        let message = |text: &str| match ProtectedPaths::parse(text) {
            Err(Error::Config { file, message }) => {
                assert_eq!(file, CONFIG);
                message
            }
            other => panic!("expected a config error for {text:?}, got {other:?}"),
        };
        assert!(message("protected = []").contains("no protected pattern"));
        assert!(message("except = [\"a\"]").contains("unexpected shape"));
        assert!(message("protected = \"docs/**\"").contains("unexpected shape"));
        assert!(message("protected = [\"a\"]\nunknown = 1").contains("unexpected shape"));
        assert!(
            message("protected = [\"a\"]\n[[tables]]\nfile = \"x\"").contains("unexpected shape")
        );
        assert!(message("this is not toml").contains("near line 1"));
        // A bad glob is named by its place in the list, never quoted.
        let bad = message("protected = [\"ok\", \"SECRET[\"]");
        assert!(bad.contains("`protected` pattern 2"), "{bad}");
        assert!(!bad.contains("SECRET"), "{bad}");
        let bad = message("protected = [\"ok\"]\nexcept = [\"SECRET{a\"]");
        assert!(bad.contains("`except` pattern 1"), "{bad}");
        assert!(!bad.contains("SECRET"), "{bad}");
    }

    #[test]
    fn a_rules_file_that_is_missing_is_an_io_error() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|source| Error::Io {
            path: "temporary directory".into(),
            source,
        })?;
        let result = ProtectedPaths::load(dir.path());
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
        Ok(())
    }

    #[test]
    fn rules_are_read_from_the_checkout_that_is_given() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|source| Error::Io {
            path: "temporary directory".into(),
            source,
        })?;
        let io = |source| Error::Io {
            path: dir.path().to_path_buf(),
            source,
        };
        fs::create_dir(dir.path().join(".loom")).map_err(io)?;
        fs::write(dir.path().join(CONFIG), "protected = [\"only/**\"]\n").map_err(io)?;
        let rules = ProtectedPaths::load(dir.path())?;
        assert!(rules.matches("only/x"));
        assert!(!rules.matches("docs/gate.md"));
        assert_eq!(rules.tables(), []);
        Ok(())
    }
}
