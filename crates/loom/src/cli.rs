//! The command line: clap types only.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Loom: the server and CLI.
#[derive(Debug, Parser)]
#[command(name = "loom", version, about)]
pub struct Cli {
    /// What to do; without one, the help is printed.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Work with a planning repo.
    #[command(subcommand)]
    Plan(PlanCommand),
}

/// The `loom plan` subcommands.
#[derive(Debug, Subcommand)]
pub enum PlanCommand {
    /// Check a planning repo; exit 0 without errors, 1 with errors, 2 when it can't be read.
    Check {
        /// The root of the planning repo.
        path: PathBuf,
    },
    /// Write the JSON schema of every front-matter type into a directory.
    Schemas {
        /// The directory to write into; it must exist.
        dir: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn parses_plan_check_and_no_subcommand() -> TestResult {
        let cli = Cli::try_parse_from(["loom", "plan", "check", "repo"])?;
        assert!(matches!(
            cli.command,
            Some(Command::Plan(PlanCommand::Check { .. }))
        ));
        assert!(Cli::try_parse_from(["loom"])?.command.is_none());
        assert!(Cli::try_parse_from(["loom", "plan", "check"]).is_err());
        Ok(())
    }
}
