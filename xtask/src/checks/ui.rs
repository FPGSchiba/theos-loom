//! The `ui` part: the web app's type check, lint and tests, run with pnpm in `web/`.
//!
//! Every call is `pnpm` with fixed arguments: `pnpm exec <tool>`, never a `package.json` script,
//! because scripts are unprotected text that could make the part pass without running a tool. The
//! install passes `--ignore-scripts`, so no lifecycle script of the root project runs either.
//! `pnpm` is found with the `which` crate, since on Windows it is a `.cmd` shim that
//! `std::process::Command` does not find by name. `tsc -b` is the type check (the template's
//! tsconfigs set `noEmit`), `ESLint` runs with no warnings allowed, and Vitest runs once.

use std::io::Write;
use std::path::Path;

use crate::ci::CiEvent;
use crate::process::{CargoCommand, run_program};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The web app's directory, relative to the repository root.
pub const WEB_DIR: &str = "web";

/// The `pnpm` calls of the part, in order. The first installs from the lockfile as it is, without
/// running any lifecycle script.
#[must_use]
pub fn commands() -> Vec<Vec<&'static str>> {
    vec![
        vec!["install", "--frozen-lockfile", "--ignore-scripts"],
        vec!["exec", "tsc", "-b"],
        vec!["exec", "eslint", ".", "--max-warnings", "0"],
        vec!["exec", "vitest", "run"],
    ]
}

/// Runs the calls of [`commands`] in `web/`, showing their output. A call that exits non-zero is
/// a finding that names it. Later calls still run, except after a failed install, because they
/// would only fail for the same reason. The calls are the same locally and under GitHub Actions;
/// `_ci` and `_out` are not used.
///
/// # Errors
///
/// Fails when `pnpm` is not on the path or cannot be started (the `tools` part reports its
/// version).
pub fn run(root: &Path, _ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    let pnpm = which::which("pnpm").map_err(|error| Error::Missing {
        what: format!("pnpm on the path ({error}); gate/tools.toml pins it"),
    })?;
    let web = root.join(WEB_DIR);
    let mut findings = Vec::new();
    for (position, args) in commands().into_iter().enumerate() {
        let command = CargoCommand {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: Vec::new(),
            env_remove: Vec::new(),
        };
        if !run_program(pnpm.as_os_str(), &web, &command)? {
            findings.push(Finding::new(format!(
                "`pnpm {}` failed in {WEB_DIR}/; its output is above",
                args.join(" ")
            )));
            if position == 0 {
                break;
            }
        }
    }
    Ok(Verdict::with(Check::Ui, findings))
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC: TU-00.3
    #[test]
    fn the_ui_part_runs_fixed_pnpm_exec_calls() {
        assert_eq!(
            commands(),
            [
                vec!["install", "--frozen-lockfile", "--ignore-scripts"],
                vec!["exec", "tsc", "-b"],
                vec!["exec", "eslint", ".", "--max-warnings", "0"],
                vec!["exec", "vitest", "run"],
            ]
        );
        assert!(commands().iter().all(|args| args[0] != "run"));
    }
}
