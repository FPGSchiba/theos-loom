//! `loom`: the server and CLI. For now it has the `plan` commands.

mod cli;
mod plan_cmd;
mod report;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command, PlanCommand};

/// Parses the command line and runs it; a failure to write is reported as exit code 2.
fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();
    exit_code(run(Cli::parse()), &mut io::stderr().lock())
}

/// Maps the result of [`run`] to an exit code; a failure is written to `err` and is exit code 2.
fn exit_code(result: Result<u8>, err: &mut impl Write) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            // Nothing sensible is left to do when stderr itself can't be written.
            let _ = writeln!(err, "loom: {e:#}");
            ExitCode::from(plan_cmd::UNREADABLE)
        }
    }
}

fn run(cli: Cli) -> Result<u8> {
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let code = match cli.command {
        None => {
            Cli::command().write_help(&mut out)?;
            writeln!(out)?;
            plan_cmd::OK
        }
        Some(Command::Plan(PlanCommand::Check { path })) => {
            plan_cmd::check(&path, &mut out, &mut err)?
        }
        Some(Command::Plan(PlanCommand::Schemas { dir })) => {
            plan_cmd::schemas(&dir, &mut out, &mut err)?
        }
    };
    out.flush()?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_written_to_err_and_exits_2() {
        let mut err = Vec::new();
        let result = Err(anyhow::anyhow!("disk gone"));
        assert_eq!(exit_code(result, &mut err), ExitCode::from(2));
        assert_eq!(
            String::from_utf8_lossy(&err),
            "loom: disk gone
"
        );
    }

    #[test]
    fn a_success_keeps_its_code_and_writes_nothing() {
        let mut err = Vec::new();
        assert_eq!(exit_code(Ok(1), &mut err), ExitCode::from(1));
        assert_eq!(err.len(), 0);
    }
}
