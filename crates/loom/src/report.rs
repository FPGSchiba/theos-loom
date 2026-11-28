//! The plain-text report of `loom plan check`.

use std::io::{self, Write};

use loom_plan::{Finding, Severity};

/// Writes errors, then notes, grouped by path, then a count line.
///
/// Each line is `error[<rule>] <path>:<line>: <message>` (`note[...]` for notes; `:<line>` is
/// left out when the line is unknown). The last line is `N errors, M notes`, with `1 error` and
/// `1 note` in the singular.
///
/// # Errors
/// Fails when `out` can't be written.
pub fn render(findings: &[Finding], out: &mut impl Write) -> io::Result<()> {
    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by(|a, b| (a.severity, &a.path, a.line).cmp(&(b.severity, &b.path, b.line)));
    for finding in &sorted {
        let kind = match finding.severity {
            Severity::Error => "error",
            Severity::Note => "note",
        };
        write!(out, "{kind}[{}] {}", finding.rule, finding.path)?;
        if let Some(line) = finding.line {
            write!(out, ":{line}")?;
        }
        writeln!(out, ": {}", finding.message)?;
    }
    let errors = count(findings, Severity::Error);
    let notes = count(findings, Severity::Note);
    writeln!(
        out,
        "{errors} {}, {notes} {}",
        plural(errors, "error"),
        plural(notes, "note")
    )
}

/// How many findings have `severity`.
#[must_use]
pub fn count(findings: &[Finding], severity: Severity) -> usize {
    findings.iter().filter(|f| f.severity == severity).count()
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use loom_plan::Rule;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn render_to_string(findings: &[Finding]) -> Result<String, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        render(findings, &mut out)?;
        Ok(String::from_utf8(out)?)
    }

    #[test]
    fn report_lists_errors_before_notes() -> TestResult {
        let mut note = Finding::note(Rule::Layout, "a.md", "a note".to_owned());
        note.line = Some(3);
        let error = Finding::error(Rule::Cycle, "z.md", "an error".to_owned());
        let text = render_to_string(&[note, error])?;
        assert_eq!(
            text,
            "error[cycle] z.md: an error\nnote[layout] a.md:3: a note\n1 error, 1 note\n"
        );
        Ok(())
    }

    #[test]
    fn empty_report_has_only_the_count_line() -> TestResult {
        assert_eq!(render_to_string(&[])?, "0 errors, 0 notes\n");
        Ok(())
    }

    #[test]
    fn findings_group_by_path_within_a_severity() -> TestResult {
        let findings = [
            Finding::error(Rule::Drift, "b.md", "two".to_owned()),
            Finding::error(Rule::Drift, "a.md", "one".to_owned()),
            Finding::error(Rule::Drift, "b.md", "three".to_owned()),
        ];
        let text = render_to_string(&findings)?;
        assert!(text.starts_with("error[drift] a.md: one\nerror[drift] b.md: "));
        assert!(text.ends_with("3 errors, 0 notes\n"));
        Ok(())
    }
}
