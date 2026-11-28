//! The `lints` check: nothing outside `gate/` can weaken the lints, the tool configuration or the
//! test run, and no source file hides code from a check (`docs/gate.md` §1).
//!
//! The check is a list of rules, [`RULES`], each one module with one job. A rule reads the
//! [`Inputs`]: the repository files, the workspace view (what cargo compiles, from its dep-info,
//! and every Rust file of it parsed once with `syn`) and the cfg names that code may test. A new
//! rule is one module and one entry in the list.
//!
//! - `sources` (rule 1): library, binary and build-script code is compiled from the `src/` of its
//!   crate only, and has no `include!`, so that `coverage` measures what ships.
//! - `cfg_names` (rule 2): `cfg` and `cfg_attr` attributes use only the names of
//!   `gate/cfg-names.toml`, so that every check builds and measures the same code.
//! - `attributes` (rule 3): no `allow` or `warn` attribute anywhere, no `expect` or `ignore`
//!   without a reason anywhere (clippy does not see macro tokens, nor an item that a `cfg` strips
//!   on the checking machine), and no `allow` or `expect` of a lint that guards the others
//!   (`unexpected_cfgs`, the two allow lints, `ignore_without_reason`) or of the groups
//!   `clippy::pedantic` and `clippy::restriction` that contain them. It reads the syntax tree and
//!   the tokens of macro calls and `macro_rules!` bodies.
//! - `config_files` (rule 4): the lint tables, the manifests and the mirrored configuration
//!   files, which are read as text and TOML and need no view.
//!
//! What is left to the reviews: an attribute that a macro forwards through a metavariable
//! (`#[$m]` in a macro body) is not judged, while the tokens of the invocation, where the
//! attribute is written, are; a cfg predicate with a metavariable is not judged, `cfg!(…)` in an
//! expression is not an attribute, and a file that no target compiles is not read at all.
//!
//! The view's own findings (a file that does not parse) come first. Findings name the file and the
//! line and say what is wrong, never what is written there, with one exception: the cfg rule
//! repeats a rejected cfg name, as ASCII letters, digits and `_` only (every other character as
//! `?`) and at most 40 characters. That cannot carry a format or bidi character, `::`, a newline
//! or a `%`, so it cannot form a workflow command or an option. The repository is the pull
//! request's, and what a check prints is read by the runner.

use std::io::Write;
use std::path::Path;

use crate::Result;
use crate::ci::CiEvent;
use crate::files::repo_files;
use crate::verdict::{Check, Finding, Verdict};
use crate::view::{Shared, View};

mod attributes;
mod attrs;
pub mod cfg_names;
pub mod config_files;
mod sources;

pub use cfg_names::CfgNames;
pub(crate) use config_files::parse;
pub use config_files::{same_text, workspace_lints_match};

/// What a rule reads.
pub struct Inputs<'a> {
    /// The repository root.
    pub root: &'a Path,
    /// The files of the repository, repo-relative with `/`.
    pub files: &'a [String],
    /// What cargo compiles, and every Rust file of it parsed.
    pub view: &'a View,
    /// The cfg names that code may test.
    pub cfg_names: &'a CfgNames,
}

/// One rule of the check.
pub struct Rule {
    /// The rule's name.
    pub name: &'static str,
    /// Runs the rule; its findings are sorted by file and line.
    pub run: fn(&Inputs<'_>) -> Result<Vec<Finding>>,
}

/// Every rule, in the order their findings are reported.
pub const RULES: &[Rule] = &[
    sources::RULE,
    cfg_names::RULE,
    attributes::RULE,
    config_files::RULE,
];

/// Runs the check on the repository at `root`, on the files that git lists there (a walk
/// outside a work tree), over the view that `shared` holds.
///
/// # Errors
///
/// Fails when the cfg names, the files or the view cannot be read, or a rule fails.
pub fn run(
    root: &Path,
    _ci: Option<&CiEvent>,
    shared: &Shared,
    _out: &mut dyn Write,
) -> Result<Verdict> {
    // A bad `gate/cfg-names.toml` fails before cargo runs.
    let cfg_names = CfgNames::load(root)?;
    let files = repo_files(root)?;
    check_with(root, &files, shared.view(root)?, &cfg_names)
}

/// Checks the repository at `root`, whose files are `files` (repo-relative, joined with `/`),
/// over `view`: the findings of the view first, then those of each rule in the order of
/// [`RULES`].
///
/// # Errors
///
/// Fails when `gate/cfg-names.toml` or a file that a rule reads cannot be read.
pub fn check(root: &Path, files: &[String], view: &View) -> Result<Verdict> {
    check_with(root, files, view, &CfgNames::load(root)?)
}

fn check_with(root: &Path, files: &[String], view: &View, cfg_names: &CfgNames) -> Result<Verdict> {
    let inputs = Inputs {
        root,
        files,
        view,
        cfg_names,
    };
    let mut findings = view.findings.clone();
    for rule in RULES {
        findings.extend((rule.run)(&inputs)?);
    }
    Ok(Verdict::with(Check::Lints, findings))
}

/// A finding about the whole of `file`.
fn at_file(file: &str, message: impl Into<String>) -> Finding {
    Finding {
        file: Some(file.to_owned()),
        line: None,
        message: message.into(),
    }
}

/// What the tests of this module and of the files below it have in common.
#[cfg(test)]
mod testing {
    pub(super) fn paths(files: &[&str]) -> Vec<String> {
        files.iter().map(|file| (*file).to_owned()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_is_registered_once() {
        let names: Vec<&str> = RULES.iter().map(|rule| rule.name).collect();
        assert_eq!(
            names,
            ["sources", "cfg-names", "attributes", "config-files"]
        );
    }
}
