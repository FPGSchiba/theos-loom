//! `cargo xtask gate`: every local check of `docs/gate.md` §1, in a fixed order, and one table.
//!
//! The gate calls the same per-check runners as the subcommands do, with no override flags, so a
//! check gives the same verdict here as in its CI job (TU-00.3); only where the inputs come from
//! differs (the repository instead of the event payload). It leaves out the checks that need a
//! pull request (`pr-title`, `pr-body`, `approval`) and the settings check, which needs GitHub.
//! A [`Selection`] runs a part of it (`--only`, `--skip`): a filter over [`GATE_ORDER`] that
//! never removes `tools`. `cargo xtask pr` runs the two checks of a pull request through the
//! same runner ([`run_pr`]).
//!
//! Versions decide verdicts, so a gate whose tools are not the pinned ones would give verdicts
//! that CI does not. `tools` checks the tools of the parts that will run, and when it fails,
//! nothing else runs.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::checks::{
    ac_trace, cargo_cmds, commit_msg, coverage, lints, pr_body, pr_title, secrets,
};
use crate::ci::CiEvent;
use crate::git::Git;
use crate::verdict::{Check, Finding, GATE_ORDER, Verdict};
use crate::view::Shared;
use crate::{Error, Result, report, tools};

/// Which checks of [`GATE_ORDER`] to run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Selection {
    /// Every check.
    All,
    /// Only these checks (and `tools`).
    Only(Vec<Check>),
    /// Every check but these.
    Skip(Vec<Check>),
}

/// Reads the names of `--only` and `--skip` into a [`Selection`].
///
/// # Errors
///
/// Fails when both lists are given, or a name is not a part of the gate (`tools` included:
/// it always runs first). The error names it, so a typo is never a smaller gate.
pub fn selection(only: &[String], skip: &[String]) -> Result<Selection> {
    if !only.is_empty() && !skip.is_empty() {
        return Err(Error::Usage {
            message: "`--only` and `--skip` cannot be used together".to_owned(),
        });
    }
    let parts =
        |names: &[String]| -> Result<Vec<Check>> { names.iter().map(|n| part(n)).collect() };
    if !only.is_empty() {
        Ok(Selection::Only(parts(only)?))
    } else if !skip.is_empty() {
        Ok(Selection::Skip(parts(skip)?))
    } else {
        Ok(Selection::All)
    }
}

/// The part of the gate called `name`: a check of [`GATE_ORDER`] other than `tools`.
fn part(name: &str) -> Result<Check> {
    match Check::from_name(name) {
        Some(check) if check != Check::Tools && GATE_ORDER.contains(&check) => Ok(check),
        _ => {
            let parts: Vec<&str> = GATE_ORDER[1..].iter().map(|check| check.name()).collect();
            Err(Error::Usage {
                message: format!(
                    "`{name}` is not a part of the gate; the parts are {}",
                    parts.join(", ")
                ),
            })
        }
    }
}

/// The checks to run: `tools` first, then the chosen ones in [`GATE_ORDER`].
#[must_use]
pub fn selected(selection: &Selection) -> Vec<Check> {
    GATE_ORDER
        .iter()
        .copied()
        .filter(|check| {
            *check == Check::Tools
                || match selection {
                    Selection::All => true,
                    Selection::Only(only) => only.contains(check),
                    Selection::Skip(skip) => !skip.contains(check),
                }
        })
        .collect()
}

/// Runs the selected checks of [`GATE_ORDER`] in `root`, and writes the verdict table to `out`
/// after the output of the checks themselves (and, under GitHub Actions, the annotations and the
/// summary). Returns the verdicts; the gate passes when all of them do.
///
/// A check that fails does not stop the others, and a check that cannot run is a failing verdict
/// that carries the error. Only a failing `tools` check stops the gate, as the module
/// documentation says: the checks after it are not run, and its verdict notes which.
///
/// # Errors
///
/// Fails only when `out` cannot be written.
pub fn run(
    root: &Path,
    ci: Option<&CiEvent>,
    selection: &Selection,
    out: &mut dyn Write,
) -> Result<Vec<Verdict>> {
    // The checks that read code share one view, built when the first of them asks for it.
    let shared = Shared::new();
    let order = selected(selection);
    run_checks(
        &order,
        "gate",
        ci.is_some(),
        |check, out| run_check(check, root, ci, &order, &shared, out),
        out,
    )
}

/// What `cargo xtask pr` was given instead of the pull request event. `None` takes it from the
/// event, as the `pr-title` and `pr-body` subcommands do.
#[derive(Clone, Default, Debug)]
pub struct PrInputs {
    /// The title to check.
    pub title: Option<String>,
    /// A file with the description to check.
    pub body_file: Option<PathBuf>,
    /// The branch the title must match.
    pub branch: Option<String>,
}

/// Runs `pr-title` and then `pr-body` through the gate's runner, with no `tools`, and writes one
/// table. Both always run, whatever the first one finds.
///
/// # Errors
///
/// Fails only when `out` cannot be written.
pub fn run_pr(
    root: &Path,
    ci: Option<&CiEvent>,
    inputs: &PrInputs,
    out: &mut dyn Write,
) -> Result<Vec<Verdict>> {
    run_checks(
        &[Check::PrTitle, Check::PrBody],
        "pr",
        ci.is_some(),
        |check, out| match check {
            Check::PrTitle => match &inputs.title {
                None => pr_title::run(root, ci, out),
                Some(title) => {
                    let branch =
                        commit_msg::branch_kind(&Git::new(root), inputs.branch.as_deref())?;
                    Ok(pr_title::check_title(title, &branch, None))
                }
            },
            _ => match &inputs.body_file {
                None => pr_body::run(root, ci, out),
                Some(path) => {
                    let template = pr_body::read_template(root)?;
                    let body = std::fs::read_to_string(path).map_err(|source| Error::Io {
                        path: path.clone(),
                        source,
                    })?;
                    Ok(pr_body::check_body(&template, Some(&body)))
                }
            },
        },
        out,
    )
}

/// The runner of one gate check: the function its subcommand calls. `parts` are the checks that
/// will run, which `tools` needs to know which tools to ask for.
///
/// There is no wildcard arm, so a new [`Check`] does not compile until it is decided here.
fn run_check(
    check: Check,
    root: &Path,
    ci: Option<&CiEvent>,
    parts: &[Check],
    shared: &Shared,
    out: &mut dyn Write,
) -> Result<Verdict> {
    match check {
        Check::Tools => tools::run_for(root, parts, out),
        Check::Fmt | Check::Clippy | Check::Test | Check::Docs | Check::Deny => {
            cargo_cmds::run(check, root, ci, out)
        }
        Check::Lints => lints::run(root, ci, shared, out),
        Check::AcTrace => ac_trace::run(root, ci, shared, out),
        Check::CommitMsg => commit_msg::run(root, ci, out),
        Check::Secrets => secrets::run(root, ci, out),
        Check::Coverage => coverage::run(root, ci, out),
        Check::PrTitle | Check::PrBody | Check::Approval | Check::Github => Err(Error::Missing {
            what: format!(
                "a way to run the `{}` check in the gate: it needs a pull request or GitHub",
                check.name()
            ),
        }),
    }
}

/// Runs the checks of `order` with `run_one`, then writes the table under `heading`. See [`run`].
fn run_checks(
    order: &[Check],
    heading: &str,
    ci: bool,
    mut run_one: impl FnMut(Check, &mut dyn Write) -> Result<Verdict>,
    out: &mut dyn Write,
) -> Result<Vec<Verdict>> {
    let mut verdicts = Vec::new();
    for (position, &check) in order.iter().enumerate() {
        // Cargo writes to the terminal on its own, so the heading goes out before it starts.
        writeln!(out, "==> {}", check.name()).map_err(report::output_error)?;
        out.flush().map_err(report::output_error)?;
        let mut verdict = run_one(check, out).unwrap_or_else(|error| could_not_run(check, &error));
        let stop = check == Check::Tools && !verdict.passed();
        if stop && position + 1 < order.len() {
            let skipped: Vec<&str> = order[position + 1..]
                .iter()
                .map(|skipped| skipped.name())
                .collect();
            verdict.notes.push(format!(
                "not run: {}; install the pinned tools first",
                skipped.join(", ")
            ));
        }
        verdicts.push(verdict);
        if stop {
            break;
        }
    }
    writeln!(out, "==> {heading}").map_err(report::output_error)?;
    report::emit_all(&verdicts, ci, out)?;
    out.flush().map_err(report::output_error)?;
    Ok(verdicts)
}

/// The verdict of a check that stopped with an error: one finding with the error text.
fn could_not_run(check: Check, error: &Error) -> Verdict {
    Verdict::with(check, vec![Finding::new(format!("could not run: {error}"))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::PR_ONLY;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// Where a check runs.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Place {
        /// In the gate, after the version check.
        Gate,
        /// Only on a pull request; the gate leaves it out.
        PrOnly,
        /// Neither: the version check that opens the gate, and the settings check that needs
        /// GitHub.
        Apart,
    }

    /// Lists every check once, and builds `ALL` and `place` from that list. `place` has no
    /// wildcard arm, so a new `Check` does not compile here until it is placed.
    macro_rules! placed {
        ($($check:ident => $place:ident),+ $(,)?) => {
            const ALL: &[Check] = &[$(Check::$check),+];

            fn place(check: Check) -> Place {
                match check {
                    $(Check::$check => Place::$place),+
                }
            }
        };
    }

    placed! {
        Tools => Apart,
        Fmt => Gate,
        Clippy => Gate,
        Test => Gate,
        Docs => Gate,
        Deny => Gate,
        Coverage => Gate,
        Lints => Gate,
        AcTrace => Gate,
        CommitMsg => Gate,
        Secrets => Gate,
        PrTitle => PrOnly,
        PrBody => PrOnly,
        Approval => PrOnly,
        Github => Apart,
    }

    fn names(checks: &[Check]) -> Vec<&'static str> {
        checks.iter().map(|check| check.name()).collect()
    }

    // AC: TU-00.3
    #[test]
    fn gate_runs_every_ci_check_except_pr_only() {
        let after_tools: Vec<Check> = GATE_ORDER
            .iter()
            .copied()
            .filter(|check| *check != Check::Tools)
            .collect();
        assert_eq!(
            GATE_ORDER.first(),
            Some(&Check::Tools),
            "tools opens the gate"
        );
        for &check in ALL {
            let expected = match place(check) {
                Place::Gate => (true, false),
                Place::PrOnly => (false, true),
                Place::Apart => (false, false),
            };
            assert_eq!(
                (after_tools.contains(&check), PR_ONLY.contains(&check)),
                expected,
                "{} is not where `place` says",
                check.name()
            );
        }
        // No duplicates in or between the lists, and nothing in a list that is not a check.
        let apart: Vec<Check> = ALL
            .iter()
            .copied()
            .filter(|check| place(*check) == Place::Apart)
            .collect();
        assert_eq!(names(&apart), ["tools", "github"]);
        assert_eq!(
            after_tools.len() + PR_ONLY.len() + apart.len(),
            ALL.len(),
            "every check is in exactly one of the gate, the PR-only checks and the set apart"
        );
    }

    fn fail(check: Check) -> Verdict {
        Verdict::with(
            check,
            vec![Finding::new(format!("{} is wrong", check.name()))],
        )
    }

    /// Runs the orchestration over `order` with `decide` as every check, and returns the checks
    /// that were run, the verdicts and everything written to the output.
    fn run_with(
        order: &[Check],
        mut decide: impl FnMut(Check) -> Result<Verdict>,
    ) -> TestResult<(Vec<Check>, Vec<Verdict>, String)> {
        let mut ran = Vec::new();
        let mut out = Vec::new();
        let verdicts = run_checks(
            order,
            "gate",
            false,
            |check, out| {
                ran.push(check);
                writeln!(out, "output of {}", check.name()).map_err(|source| Error::Io {
                    path: "<test>".into(),
                    source,
                })?;
                decide(check)
            },
            &mut out,
        )?;
        Ok((ran, verdicts, String::from_utf8(out)?))
    }

    fn checks_of(verdicts: &[Verdict]) -> Vec<Check> {
        verdicts.iter().map(|verdict| verdict.check).collect()
    }

    fn only_failing(failing: &'static [Check]) -> impl FnMut(Check) -> Result<Verdict> {
        move |check| {
            Ok(if failing.contains(&check) {
                fail(check)
            } else {
                Verdict::pass(check)
            })
        }
    }

    #[test]
    fn checks_run_in_gate_order_and_all_pass() -> TestResult {
        let (ran, verdicts, _) = run_with(&GATE_ORDER, only_failing(&[]))?;
        assert_eq!(ran, GATE_ORDER);
        assert_eq!(checks_of(&verdicts), GATE_ORDER);
        assert!(verdicts.iter().all(Verdict::passed));
        Ok(())
    }

    #[test]
    fn a_failing_check_does_not_stop_the_rest() -> TestResult {
        let (ran, verdicts, _) = run_with(&GATE_ORDER, only_failing(&[Check::Fmt, Check::Clippy]))?;
        assert_eq!(ran, GATE_ORDER);
        let failed: Vec<&str> = verdicts
            .iter()
            .filter(|verdict| !verdict.passed())
            .map(|verdict| verdict.check.name())
            .collect();
        assert_eq!(failed, ["fmt", "clippy"]);
        Ok(())
    }

    #[test]
    fn a_failing_tools_check_skips_the_rest() -> TestResult {
        let (ran, verdicts, out) = run_with(&GATE_ORDER, only_failing(&[Check::Tools]))?;
        assert_eq!(ran, [Check::Tools]);
        assert_eq!(checks_of(&verdicts), [Check::Tools]);
        assert!(!verdicts[0].passed());
        let [note] = verdicts[0].notes.as_slice() else {
            return Err(format!("expected one note, got {:?}", verdicts[0].notes).into());
        };
        assert!(note.contains("install the pinned tools first"), "{note}");
        for skipped in &GATE_ORDER[1..] {
            assert!(
                note.contains(skipped.name()),
                "{} missing from {note}",
                skipped.name()
            );
        }
        assert!(out.contains("FAIL  tools"), "{out}");
        assert!(!out.contains("output of fmt"), "{out}");
        Ok(())
    }

    #[test]
    fn an_erroring_tools_check_skips_the_rest_too() -> TestResult {
        let (ran, verdicts, _) = run_with(&GATE_ORDER, |check| {
            if check == Check::Tools {
                Err(Error::Missing {
                    what: "gate/tools.toml".to_owned(),
                })
            } else {
                Ok(Verdict::pass(check))
            }
        })?;
        assert_eq!(ran, [Check::Tools]);
        assert!(!verdicts[0].passed());
        assert!(
            verdicts[0].notes[0].contains("install the pinned tools first"),
            "{:?}",
            verdicts[0].notes
        );
        Ok(())
    }

    #[test]
    fn an_error_becomes_a_failing_verdict_with_its_text() -> TestResult {
        let (ran, verdicts, out) = run_with(&GATE_ORDER, |check| {
            if check == Check::Lints {
                Err(Error::Missing {
                    what: "the lint list".to_owned(),
                })
            } else {
                Ok(Verdict::pass(check))
            }
        })?;
        assert_eq!(ran, GATE_ORDER, "the checks after an error still run");
        let failed: Vec<&Verdict> = verdicts
            .iter()
            .filter(|verdict| !verdict.passed())
            .collect();
        let [lints] = failed.as_slice() else {
            return Err(format!("expected one failure, got {failed:?}").into());
        };
        assert_eq!(lints.check, Check::Lints);
        let [finding] = lints.findings.as_slice() else {
            return Err(format!("expected one finding, got {:?}", lints.findings).into());
        };
        assert!(
            finding.message.contains("missing the lint list"),
            "{}",
            finding.message
        );
        assert!(out.contains("FAIL  lints"), "{out}");
        Ok(())
    }

    #[test]
    fn each_checks_output_comes_first_and_the_table_last() -> TestResult {
        let (_, verdicts, out) = run_with(&GATE_ORDER, only_failing(&[Check::Docs]))?;
        let mut table = Vec::new();
        report::write_table(&verdicts, &mut table)?;
        let table = String::from_utf8(table)?;
        assert!(out.ends_with(&table), "{out}");
        let before_table = &out[..out.len() - table.len()];
        for check in GATE_ORDER {
            assert!(
                before_table.contains(&format!("output of {}\n", check.name())),
                "{} wrote no output before the table: {out}",
                check.name()
            );
        }
        assert!(table.contains("FAIL  docs\n  - docs is wrong\n"), "{table}");
        Ok(())
    }

    #[test]
    fn a_check_that_needs_a_pull_request_is_not_run_by_the_gate() {
        let mut out = Vec::new();
        for check in PR_ONLY.into_iter().chain([Check::Github]) {
            let result = run_check(check, Path::new("."), None, &[], &Shared::new(), &mut out);
            assert!(
                matches!(result, Err(Error::Missing { .. })),
                "{check:?}: {result:?}"
            );
        }
        assert_eq!(
            out,
            Vec::<u8>::new(),
            "nothing is written for a check that is not run"
        );
    }

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn selected_names(only: &[&str], skip: &[&str]) -> TestResult<Vec<&'static str>> {
        let selection = selection(&strings(only), &strings(skip))?;
        Ok(names(&selected(&selection)))
    }

    // AC: TU-00.3
    #[test]
    fn only_runs_tools_and_the_named_parts_in_gate_order() -> TestResult {
        assert_eq!(
            selected_names(&["test", "fmt"], &[])?,
            ["tools", "fmt", "test"]
        );
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn skip_runs_every_other_part() -> TestResult {
        let expected: Vec<Check> = GATE_ORDER
            .iter()
            .copied()
            .filter(|check| !matches!(check, Check::Test | Check::Coverage))
            .collect();
        assert_eq!(
            selected_names(&[], &["test", "coverage"])?,
            names(&expected)
        );
        assert_eq!(names(&selected(&selection(&[], &[])?)), names(&GATE_ORDER));
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn only_and_skip_refuse_a_name_that_is_not_a_gate_part() {
        for name in ["tset", "pr-title", "tools", "github", "approval"] {
            for (only, skip) in [(vec![name], vec![]), (vec![], vec![name])] {
                let result = selection(&strings(&only), &strings(&skip));
                let Err(error) = result else {
                    panic!("`{name}` was accepted");
                };
                assert!(matches!(error, Error::Usage { .. }), "{error:?}");
                assert!(error.to_string().contains(name), "{error}");
            }
        }
        assert!(selection(&strings(&["fmt"]), &strings(&["test"])).is_err());
    }

    // AC: TU-00.3
    #[test]
    fn pr_runs_title_and_body_and_reports_both() -> TestResult {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dir = tempfile::tempdir()?;
        let body = dir.path().join("body.md");
        std::fs::write(&body, "nothing of the template")?;
        let inputs = PrInputs {
            title: Some("not a conventional title".to_owned()),
            body_file: Some(body),
            branch: Some("fix/x".to_owned()),
        };
        let mut out = Vec::new();
        let verdicts = run_pr(&root, None, &inputs, &mut out)?;
        assert_eq!(checks_of(&verdicts), [Check::PrTitle, Check::PrBody]);
        assert!(!verdicts[0].passed(), "a bad title fails");
        assert!(
            !verdicts[1].passed(),
            "the body ran after the failing title"
        );
        let out = String::from_utf8(out)?;
        assert!(out.contains("FAIL  pr-title"), "{out}");
        assert!(out.contains("FAIL  pr-body"), "{out}");
        Ok(())
    }

    /// A writer that refuses everything.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_output_that_cannot_be_written_is_an_error() {
        let result = run_checks(
            &GATE_ORDER,
            "gate",
            false,
            |check, _| Ok(Verdict::pass(check)),
            &mut Closed,
        );
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
    }
}
