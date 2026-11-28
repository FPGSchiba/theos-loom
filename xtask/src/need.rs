//! Which parts of the gate a run needs (P00 spec §5.3): the docs-only rule and the `ui` list.
//!
//! [`decide`] reads the changed paths ([`ChangedPaths`]) and the lists of `gate/code-paths.toml`
//! ([`CodePaths`]). Every uncertain case adds parts; none removes one:
//!
//! 1. `commit-msg` and `secrets` always run: they read the commits, not the files.
//! 2. A `phase/` branch runs every part (`ac-trace` reads the phase from it).
//! 3. Paths that cannot be listed, or no changed path at all, run every part.
//! 4. Every path non-code and none always-code: every other part is not needed.
//! 5. Otherwise every part runs.
//!
//! `ui` is the exception to 2 and 5: with known, non-empty changed paths, it runs only when one
//! is on the `ui` list, on every kind of branch.

use crate::changes::ChangedPaths;
use crate::code_paths::{CodePaths, FILE};
use crate::verdict::Check;

/// Whether a part runs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Need {
    /// The part runs.
    Run,
    /// The part cannot change its verdict; the reason is shown in the gate's table.
    NotNeeded(String),
}

/// Decides one part of the gate. `Tools` is decided by the runner, from the parts that run; it
/// is never asked here, and would run.
#[must_use]
pub fn decide(check: Check, changes: &ChangedPaths, lists: &CodePaths) -> Need {
    if matches!(check, Check::Tools | Check::CommitMsg | Check::Secrets) {
        return Need::Run;
    }
    let ChangedPaths::Known {
        branch,
        paths: files,
    } = changes
    else {
        return Need::Run;
    };
    if check == Check::Ui && !files.is_empty() && !lists.touches_ui(files) {
        return Need::NotNeeded(format!("no changed path is on the `ui` list of {FILE}"));
    }
    if !branch.starts_with("phase/") && lists.is_docs_only(files) {
        return Need::NotNeeded(format!(
            "only files that are not code changed, so this part cannot change ({FILE})"
        ));
    }
    Need::Run
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Result;

    fn lists() -> Result<CodePaths> {
        CodePaths::parse(include_str!("../../gate/code-paths.toml"))
    }

    fn known(branch: &str, paths: &[&str]) -> ChangedPaths {
        ChangedPaths::Known {
            branch: branch.to_owned(),
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    const PARTS: [Check; 9] = [
        Check::Fmt,
        Check::Clippy,
        Check::Test,
        Check::Coverage,
        Check::Deny,
        Check::Lints,
        Check::AcTrace,
        Check::Docs,
        Check::Ui,
    ];

    /// Every part but `Ui` runs; `Ui` runs when `ui` says so.
    fn assert_everything_runs(changes: &ChangedPaths, ui: bool) -> Result<()> {
        let lists = lists()?;
        for check in PARTS {
            let wanted = check != Check::Ui || ui;
            let got = decide(check, changes, &lists) == Need::Run;
            assert_eq!(got, wanted, "{} for {changes:?}", check.name());
        }
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn a_docs_only_change_runs_only_commit_msg_and_secrets() -> Result<()> {
        let lists = lists()?;
        let changes = known("docs/x", &["docs/notes.md"]);
        assert_eq!(decide(Check::CommitMsg, &changes, &lists), Need::Run);
        assert_eq!(decide(Check::Secrets, &changes, &lists), Need::Run);
        for check in PARTS {
            let need = decide(check, &changes, &lists);
            assert!(
                matches!(&need, Need::NotNeeded(reason) if reason.contains("gate/code-paths.toml")),
                "{}: {need:?}",
                check.name()
            );
        }
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn an_always_code_path_under_docs_runs_everything() -> Result<()> {
        for path in [
            "docs/specs/00-umbrella.md",
            "docs/specs/phases/P01-x.md",
            "docs/gate.md",
            "docs/x/Cargo.toml",
            "docs/a.ts",
            "docs/a.tsx",
        ] {
            assert_everything_runs(&known("docs/x", &[path]), false)?;
        }
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn a_phase_branch_runs_everything() -> Result<()> {
        assert_everything_runs(&known("phase/P01-x", &["docs/notes.md"]), false)?;
        assert_everything_runs(&known("phase/P01-x", &["web/src/App.tsx"]), true)
    }

    // AC: TU-00.3
    #[test]
    fn no_rule_unreadable_or_empty_runs_everything() -> Result<()> {
        let why = "why".to_owned();
        assert_everything_runs(&ChangedPaths::NoRule { why: why.clone() }, true)?;
        assert_everything_runs(&ChangedPaths::Unreadable { why }, true)?;
        assert_everything_runs(&known("docs/x", &[]), true)
    }

    // AC: TU-00.3
    #[test]
    fn ui_runs_only_when_a_ui_path_changed() -> Result<()> {
        let lists = lists()?;
        let other = known("fix/x", &["crates/loom-core/src/lib.rs"]);
        let need = decide(Check::Ui, &other, &lists);
        assert!(matches!(need, Need::NotNeeded(_)), "{need:?}");
        assert_eq!(decide(Check::Test, &other, &lists), Need::Run);
        for path in ["web/src/App.tsx", "gate/tools.toml", "xtask/src/lib.rs"] {
            let changes = known("fix/x", &[path]);
            assert_eq!(decide(Check::Ui, &changes, &lists), Need::Run, "{path}");
        }
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn always_code_ignores_case_and_non_code_does_not() -> Result<()> {
        let lists = lists()?;
        let upper = known("docs/x", &["DOCS/notes.md"]);
        assert_eq!(decide(Check::Test, &upper, &lists), Need::Run);
        let cargo = known("docs/x", &["docs/x/CARGO.TOML"]);
        assert_eq!(decide(Check::Test, &cargo, &lists), Need::Run);
        Ok(())
    }
}
