//! The one error type of the library.
//!
//! Every fallible function in this crate returns [`Result`]. `main` turns it into exit code 2;
//! a check that merely finds something wrong returns a failing verdict, not an error.

use std::path::PathBuf;

/// Why a check could not run to a verdict.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file or directory could not be read or written.
    ///
    /// The message already has the text of `source`, which is also the source of the error. Print
    /// it with `{}`: `{:#}` adds the chain of sources and would say it twice.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A child process did not start or did not succeed.
    #[error("{}", describe_command(program, args, *status, stderr))]
    Command {
        /// The program that was run, such as `git`.
        program: String,
        /// Its arguments.
        args: Vec<String>,
        /// Its exit code, or `None` when it did not start or was ended by a signal.
        status: Option<i32>,
        /// What it wrote to stderr, or why it did not start.
        stderr: String,
    },
    /// Text that should have had a known shape did not.
    #[error("cannot parse {what}: {message}")]
    Parse {
        /// What was being read, such as `phase` or `push event payload`.
        what: String,
        /// What is wrong with it.
        message: String,
    },
    /// A file under `gate/` or another gate setting is wrong.
    #[error("{file}: {message}")]
    Config {
        /// The configuration file, repo-relative.
        file: String,
        /// What is wrong with it.
        message: String,
    },
    /// GitHub refused a request or answered with something unexpected.
    #[error("GitHub: {message}")]
    GitHub {
        /// What GitHub or `gh` said.
        message: String,
    },
    /// Something the run needs is not there: an input, a variable, a tool or a feature.
    #[error("missing {what}")]
    Missing {
        /// What is missing.
        what: String,
    },
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

fn describe_command(program: &str, args: &[String], status: Option<i32>, stderr: &str) -> String {
    let command = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let outcome = match status {
        Some(code) => format!("exited with status {code}"),
        None => "did not run to completion".to_owned(),
    };
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("`{command}` {outcome}")
    } else {
        format!("`{command}` {outcome}: {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(status: Option<i32>, stderr: &str) -> Error {
        Error::Command {
            program: "git".to_owned(),
            args: vec!["rev-parse".to_owned(), "nope".to_owned()],
            status,
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn command_errors_name_the_command_and_the_status() {
        assert_eq!(
            command(Some(128), "fatal: bad revision").to_string(),
            "`git rev-parse nope` exited with status 128: fatal: bad revision"
        );
        assert_eq!(
            command(None, "program not found").to_string(),
            "`git rev-parse nope` did not run to completion: program not found"
        );
        assert_eq!(
            command(Some(1), "").to_string(),
            "`git rev-parse nope` exited with status 1"
        );
    }

    #[test]
    fn other_errors_say_what_failed() {
        let io = Error::Io {
            path: PathBuf::from("a/b.txt"),
            source: std::io::Error::other("disk on fire"),
        };
        assert_eq!(io.to_string(), "a/b.txt: disk on fire");
        let parse = Error::Parse {
            what: "phase".to_owned(),
            message: "expected P and two digits".to_owned(),
        };
        assert_eq!(
            parse.to_string(),
            "cannot parse phase: expected P and two digits"
        );
        let config = Error::Config {
            file: "gate/tools.toml".to_owned(),
            message: "no tools".to_owned(),
        };
        assert_eq!(config.to_string(), "gate/tools.toml: no tools");
        let github = Error::GitHub {
            message: "rate limited".to_owned(),
        };
        assert_eq!(github.to_string(), "GitHub: rate limited");
        let missing = Error::Missing {
            what: "the `fmt` check (not built yet)".to_owned(),
        };
        assert_eq!(
            missing.to_string(),
            "missing the `fmt` check (not built yet)"
        );
    }
}
