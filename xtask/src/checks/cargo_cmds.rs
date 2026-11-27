//! The checks that are one or two cargo commands: `fmt`, `clippy`, `test`, `docs` and `deny`,
//! and the command of `slow-tests`, which [`crate::checks::slow`] runs when it is needed.
//!
//! The commands are `docs/gate.md` §1, with `--locked` where cargo takes it. The same lists run
//! locally and in CI, which is what makes the verdicts the same (CE-08.4). Cargo and the tools
//! write their own output to the terminal; a command that exits non-zero is one finding that
//! names it.

use std::io::Write;
use std::path::Path;

use crate::ci::CiEvent;
use crate::process::{CargoCommand, run_cargo};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

fn cargo(args: &[&str]) -> CargoCommand {
    CargoCommand {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: Vec::new(),
        env_remove: Vec::new(),
    }
}

/// `cargo fmt --all --check`.
#[must_use]
pub fn fmt() -> Vec<CargoCommand> {
    vec![cargo(&["fmt", "--all", "--check"])]
}

/// `cargo clippy --workspace --all-targets --locked -- -D warnings`.
#[must_use]
pub fn clippy() -> Vec<CargoCommand> {
    vec![cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings",
    ])]
}

/// `cargo nextest run --workspace --locked --profile <ci|default>`, then
/// `cargo test --doc --workspace --locked`. The profile is `ci` when `ci` is true. Both profiles
/// leave out the slow tests, which [`slow_tests`] runs.
#[must_use]
pub fn test(ci: bool) -> Vec<CargoCommand> {
    let profile = if ci { "ci" } else { "default" };
    vec![
        cargo(&[
            "nextest",
            "run",
            "--workspace",
            "--locked",
            "--profile",
            profile,
        ]),
        cargo(&["test", "--doc", "--workspace", "--locked"]),
    ]
}

/// `cargo nextest run --workspace --locked --profile slow --no-tests=warn`: the slow tests, the
/// ones the profiles of [`test`](fn@test) leave out (`gate/nextest.toml`), locally and in CI alike. A
/// workspace without slow tests passes with nextest's warning. The `slow-tests` check
/// ([`crate::checks::slow`]) decides whether these run.
#[must_use]
pub fn slow_tests() -> Vec<CargoCommand> {
    vec![cargo(&[
        "nextest",
        "run",
        "--workspace",
        "--locked",
        "--profile",
        "slow",
        "--no-tests=warn",
    ])]
}

/// `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` on the child.
#[must_use]
pub fn docs() -> Vec<CargoCommand> {
    vec![CargoCommand {
        env: vec![("RUSTDOCFLAGS".to_owned(), "-D warnings".to_owned())],
        ..cargo(&["doc", "--workspace", "--no-deps", "--locked"])
    }]
}

/// `cargo deny --locked check`: licences, advisories, bans and sources, with `deny.toml`.
#[must_use]
pub fn deny() -> Vec<CargoCommand> {
    vec![cargo(&["deny", "--locked", "check"])]
}

/// The commands of a check, or `None` for a check that is not a cargo command. `Test` takes its
/// profile from the run: `ci` under GitHub Actions, `default` otherwise.
fn commands_for(check: Check, ci: Option<&CiEvent>) -> Option<Vec<CargoCommand>> {
    match check {
        Check::Fmt => Some(fmt()),
        Check::Clippy => Some(clippy()),
        Check::Test => Some(test(ci.is_some())),
        Check::Docs => Some(docs()),
        Check::Deny => Some(deny()),
        _ => None,
    }
}

/// Runs the cargo check `check` in `root`: all of its commands, in order.
///
/// Under GitHub Actions (`ci`) the tests use the nextest profile `ci`. `_out` is not written to:
/// cargo's own output goes straight to the terminal and the caller writes the verdict.
///
/// # Errors
///
/// Fails when `check` is not one of `fmt`, `clippy`, `test`, `docs` and `deny`, or when cargo
/// cannot be started. A command that runs and fails is a finding, not an error.
pub fn run(
    check: Check,
    root: &Path,
    ci: Option<&CiEvent>,
    _out: &mut dyn Write,
) -> Result<Verdict> {
    let cmds = commands_for(check, ci).ok_or_else(|| Error::Missing {
        what: format!("cargo commands for the `{}` check", check.name()),
    })?;
    run_cargo_check(root, check, &cmds)
}

/// Runs every command of `cmds` in `root`, even after one has failed, so one run shows all that
/// is wrong. Each command that exits non-zero is one finding that names it.
///
/// # Errors
///
/// Fails when cargo cannot be started.
pub fn run_cargo_check(root: &Path, check: Check, cmds: &[CargoCommand]) -> Result<Verdict> {
    run_commands(check, cmds, |cmd| run_cargo(root, cmd))
}

fn run_commands(
    check: Check,
    cmds: &[CargoCommand],
    mut run: impl FnMut(&CargoCommand) -> Result<bool>,
) -> Result<Verdict> {
    let mut findings = Vec::new();
    for cmd in cmds {
        if !run(cmd)? {
            findings.push(Finding::new(format!(
                "`{}` failed; its output is above",
                describe(cmd)
            )));
        }
    }
    Ok(Verdict::with(check, findings))
}

/// The command as a person would type it: the environment, `cargo`, and the arguments.
fn describe(cmd: &CargoCommand) -> String {
    let env = cmd
        .env
        .iter()
        .map(|(key, value)| format!("{key}=\"{value}\""));
    env.chain(std::iter::once("cargo".to_owned()))
        .chain(cmd.args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = crate::Result<()>;

    fn cmd(args: &[&str], env: &[(&str, &str)]) -> CargoCommand {
        CargoCommand {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: env
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            env_remove: Vec::new(),
        }
    }

    // AC: CE-08.4
    #[test]
    fn commands_match_gate_md() {
        assert_eq!(fmt(), [cmd(&["fmt", "--all", "--check"], &[])]);
        assert_eq!(
            clippy(),
            [cmd(
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "-D",
                    "warnings"
                ],
                &[]
            )]
        );
        assert_eq!(
            test(true),
            [
                cmd(
                    &[
                        "nextest",
                        "run",
                        "--workspace",
                        "--locked",
                        "--profile",
                        "ci"
                    ],
                    &[]
                ),
                cmd(&["test", "--doc", "--workspace", "--locked"], &[]),
            ]
        );
        assert_eq!(
            test(false),
            [
                cmd(
                    &[
                        "nextest",
                        "run",
                        "--workspace",
                        "--locked",
                        "--profile",
                        "default"
                    ],
                    &[]
                ),
                cmd(&["test", "--doc", "--workspace", "--locked"], &[]),
            ]
        );
        assert_eq!(
            slow_tests(),
            [cmd(
                &[
                    "nextest",
                    "run",
                    "--workspace",
                    "--locked",
                    "--profile",
                    "slow",
                    "--no-tests=warn"
                ],
                &[]
            )]
        );
        assert_eq!(
            docs(),
            [cmd(
                &["doc", "--workspace", "--no-deps", "--locked"],
                &[("RUSTDOCFLAGS", "-D warnings")]
            )]
        );
        assert_eq!(deny(), [cmd(&["deny", "--locked", "check"], &[])]);
    }

    #[test]
    fn cargo_checks_have_commands_and_other_checks_have_none() {
        assert_eq!(commands_for(Check::Fmt, None), Some(fmt()));
        assert_eq!(commands_for(Check::Clippy, None), Some(clippy()));
        assert_eq!(commands_for(Check::Docs, None), Some(docs()));
        assert_eq!(commands_for(Check::Deny, None), Some(deny()));
        assert_eq!(commands_for(Check::Lints, None), None);
        // `slow-tests` first decides whether to run its command (`checks::slow`).
        assert_eq!(commands_for(Check::SlowTests, None), None);
        assert_eq!(commands_for(Check::Tools, None), None);
    }

    #[test]
    fn test_uses_the_ci_profile_under_ci_and_the_default_profile_otherwise() {
        assert_eq!(commands_for(Check::Test, None), Some(test(false)));
        assert_eq!(
            commands_for(Check::Test, Some(&CiEvent::Other)),
            Some(test(true))
        );
    }

    #[test]
    fn a_command_is_described_as_a_shell_user_would_type_it() {
        assert_eq!(
            describe(&clippy()[0]),
            "cargo clippy --workspace --all-targets --locked -- -D warnings"
        );
        assert_eq!(
            describe(&docs()[0]),
            "RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --locked"
        );
    }

    #[test]
    fn every_command_runs_and_each_failure_is_a_finding() -> TestResult {
        let cmds = test(true);
        let mut seen = Vec::new();
        let verdict = run_commands(Check::Test, &cmds, |command| {
            seen.push(command.args[0].clone());
            Ok(command.args[0] != "nextest")
        })?;
        assert_eq!(seen, ["nextest", "test"]);
        assert_eq!(verdict.check, Check::Test);
        assert_eq!(verdict.findings.len(), 1);
        let message = &verdict.findings[0].message;
        assert!(
            message.contains("cargo nextest run --workspace --locked --profile ci"),
            "{message}"
        );
        assert!(!verdict.passed());
        Ok(())
    }

    #[test]
    fn commands_that_all_succeed_pass() -> TestResult {
        let verdict = run_commands(Check::Fmt, &fmt(), |_| Ok(true))?;
        assert_eq!(verdict, Verdict::pass(Check::Fmt));
        Ok(())
    }

    #[test]
    fn a_command_that_cannot_run_is_an_error_not_a_finding() {
        let result = run_commands(Check::Fmt, &fmt(), |_| {
            Err(crate::Error::Missing {
                what: "cargo".to_owned(),
            })
        });
        assert!(
            matches!(&result, Err(crate::Error::Missing { what }) if what == "cargo"),
            "{result:?}"
        );
    }

    #[test]
    fn a_check_without_cargo_commands_cannot_be_run_here() {
        let mut out = Vec::new();
        let result = run(Check::Lints, std::path::Path::new("."), None, &mut out);
        assert!(
            matches!(&result, Err(crate::Error::Missing { .. })),
            "{result:?}"
        );
        assert_eq!(out, b"");
    }
}
