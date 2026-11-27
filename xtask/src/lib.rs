//! Gate tooling of Loom (`docs/gate.md`): the parts, the docs-only rule, the repository settings.
//!
//! The library holds the checks; the `xtask` binary is the command line in front of them.
//!
//! This is the core every check builds on:
//!
//! - [`verdict`]: what a check returns, the findings in it, and the order of the gate.
//! - [`report`]: how verdicts are written, as a table, as workflow annotations and as a summary.
//! - [`ci`]: the GitHub Actions event the run belongs to, if any.
//! - [`branch`]: the branch naming rules of `CONTRIBUTING.md` §1.
//! - [`conventional`]: the commit message rules of `CONTRIBUTING.md` §2, as a parser and the
//!   list of what it forbids; `commit-msg` and `pr-title` share it.
//! - [`git`], [`files`] and [`process`]: the only places that start git and cargo or walk the tree.
//! - [`github`]: the labels and comments of a pull request, and the rulesets, settings and labels
//!   of the repository, through the `gh` command line; [`github::settings`] keeps the latter as
//!   files in `gate/` and compares them with the live repository.
//! - [`changes`], [`code_paths`] and [`need`]: the paths a run or a pull request changes, the
//!   lists of `gate/code-paths.toml`, and from both which parts of the gate are needed.
//! - [`error`]: the one error type of the library.
//!
//! And the checks built on it:
//!
//! - [`checks`]: the checks of `docs/gate.md` §1; [`checks::cargo_cmds`] has the ones that are
//!   cargo commands (`fmt`, `clippy`, `test`, `docs`, `deny`), [`checks::commit_msg`] the
//!   `commit-msg` check, [`checks::pr_title`] the `pr-title` check, [`checks::pr_body`] the
//!   `pr-body` check, [`checks::ac_trace`] the `ac-trace` check, [`checks::lints`] the `lints`
//!   check, [`checks::coverage`] the `coverage` check and [`checks::approval`] the
//!   `approval` check.
//! - [`umbrella`]: the umbrella spec's phases and acceptance criteria, which `ac-trace` and
//!   `commit-msg` read their ids from.
//! - [`tools`]: the pinned tools (`gate/tools.toml`) and the `tools` check.
//! - [`view`]: the workspace view, the files cargo compiles and what `syn` reads in them, which
//!   the checks that read code share.
//! - [`gate`]: `cargo xtask gate`, which runs the checks above that need no pull request, in
//!   order, and writes one verdict table.

pub mod branch;
pub mod changes;
pub mod checks;
pub mod ci;
pub mod code_paths;
pub mod conventional;
pub mod error;
pub mod files;
pub mod gate;
pub mod git;
pub mod github;
pub mod markdown;
pub mod need;
pub mod process;
pub mod repo_path;
pub mod report;
pub mod tools;
pub mod umbrella;
pub mod verdict;
pub mod view;

pub use error::{Error, Result};
