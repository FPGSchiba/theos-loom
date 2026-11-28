//! `loom plan check` and `loom plan schemas`: run the library and map the result to an exit code.

use std::io::{self, Write};
use std::path::Path;

use loom_plan::Severity;

use crate::report;

/// Exit code: the command succeeded.
pub const OK: u8 = 0;
/// Exit code: `check` found at least one error.
pub const FINDINGS: u8 = 1;
/// Exit code: the path could not be read or written.
pub const UNREADABLE: u8 = 2;

/// Reads and checks the planning repo at `path`, writing the report to `out`.
///
/// Returns 0 without errors, 1 with at least one error, 2 (message on `err`) when `path` can't be
/// read as a planning repo.
///
/// # Errors
/// Fails when `out` or `err` can't be written.
pub fn check(path: &Path, out: &mut impl Write, err: &mut impl Write) -> io::Result<u8> {
    let repo = match loom_plan::read(path) {
        Ok(repo) => repo,
        Err(e) => {
            writeln!(err, "loom: {e}")?;
            return Ok(UNREADABLE);
        }
    };
    let findings = loom_plan::check(&repo);
    report::render(&findings, out)?;
    Ok(if report::count(&findings, Severity::Error) > 0 {
        FINDINGS
    } else {
        OK
    })
}

/// Writes every schema into `dir` and lists the files on `out`. Returns 0, or 2 (message on
/// `err`) when `dir` can't be written.
///
/// # Errors
/// Fails when `out` or `err` can't be written.
pub fn schemas(dir: &Path, out: &mut impl Write, err: &mut impl Write) -> io::Result<u8> {
    match loom_plan::schema::write_schemas(dir) {
        Ok(paths) => {
            for path in paths {
                writeln!(out, "{}", path.display())?;
            }
            Ok(OK)
        }
        Err(e) => {
            writeln!(err, "loom: cannot write schemas to {}: {e}", dir.display())?;
            Ok(UNREADABLE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../loom-plan/tests/fixtures")
            .join(name)
    }

    #[test]
    fn check_maps_findings_and_read_errors_to_exit_codes() -> TestResult {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(check(&fixture("valid"), &mut out, &mut err)?, OK);
        let text = String::from_utf8(out)?;
        assert!(text.contains("0 errors"), "{text}");
        let mut out = Vec::new();
        let tree = fixture("invalid/duplicate-id-key");
        assert_eq!(check(&tree, &mut out, &mut err)?, FINDINGS);
        assert!(String::from_utf8(out)?.contains("error[duplicate-id]"));
        let mut out = Vec::new();
        assert_eq!(check(&fixture("missing"), &mut out, &mut err)?, UNREADABLE);
        assert_eq!(out.len(), 0);
        assert!(String::from_utf8(err)?.contains("not a directory"));
        Ok(())
    }

    #[test]
    fn schemas_writes_files_and_reports_an_unwritable_dir() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(schemas(dir.path(), &mut out, &mut err)?, OK);
        assert!(String::from_utf8(out)?.contains("plan.schema.json"));
        let mut out = Vec::new();
        let missing = dir.path().join("missing");
        assert_eq!(schemas(&missing, &mut out, &mut err)?, UNREADABLE);
        assert!(String::from_utf8(err)?.contains("cannot write schemas"));
        Ok(())
    }
}
