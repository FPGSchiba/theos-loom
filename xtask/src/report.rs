//! How verdicts reach people: a table on the output, workflow annotations and a step summary.
//!
//! Locally only the table is written. Under GitHub Actions [`emit`] also writes one `::error`
//! annotation per finding and appends a markdown summary to `$GITHUB_STEP_SUMMARY`.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// Writes one line per check, `pass`, `skip` (not needed) or `FAIL` and the check's name, and
/// under each failing check its findings as `  - path:line: message`. Notes follow as
/// `  note: …`, and the reason a check was not needed as `  note: not needed: …`.
///
/// A finding or note that spans several lines continues on lines that start with `    | `.
///
/// Finding and note text can come from a pull request, and the runner reads a line as a workflow
/// command when it starts with `::` once leading whitespace is gone. So no line of the table
/// starts with the text itself: each one starts with a marker (`- `, `note:` or `| `), and the
/// text is split into lines at every kind of line break first, so a break cannot hide a
/// line start in the middle of one of ours.
///
/// # Errors
///
/// Fails when `out` cannot be written.
pub fn write_table(verdicts: &[Verdict], out: &mut dyn Write) -> io::Result<()> {
    for verdict in verdicts {
        let status = if !verdict.passed() {
            "FAIL"
        } else if verdict.is_not_needed() {
            "skip"
        } else {
            "pass"
        };
        writeln!(out, "{status}  {}", verdict.check.name())?;
        for finding in &verdict.findings {
            let text = match location(finding) {
                Some(place) => format!("{place}: {}", finding.message),
                None => finding.message.clone(),
            };
            write_marked(out, "- ", &text)?;
        }
        if let Some(reason) = &verdict.not_needed {
            write_marked(out, "note: ", &format!("not needed: {reason}"))?;
        }
        for note in &verdict.notes {
            write_marked(out, "note: ", note)?;
        }
    }
    Ok(())
}

/// Writes `text` indented, its first line after `marker` and every further line after `| `.
fn write_marked(out: &mut dyn Write, marker: &str, text: &str) -> io::Result<()> {
    let mut lines = lines_of(text).into_iter();
    writeln!(out, "  {marker}{}", lines.next().unwrap_or_default())?;
    for line in lines {
        writeln!(out, "    | {line}")?;
    }
    Ok(())
}

/// Writes `error` for stderr: the first line after `error: `, every further line after `  | `.
///
/// An error may hold text a pull request controls (a file name, a revision), and the runner reads
/// stderr for workflow commands too, so no line of it may start with the text itself.
///
/// # Errors
///
/// Fails when `out` cannot be written to.
pub fn write_error(out: &mut dyn Write, error: &dyn std::fmt::Display) -> io::Result<()> {
    let mut lines = lines_of(&error.to_string()).into_iter();
    writeln!(out, "error: {}", lines.next().unwrap_or_default())?;
    for line in lines {
        writeln!(out, "  | {line}")?;
    }
    Ok(())
}

/// Every character a reader of our output might end a line at: LF, CR and the other Unicode
/// line breaks. The runner is only known to split at the first two; the rest is for safety.
const LINE_BREAKS: [char; 7] = [
    '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
];

/// The lines of `text`, split at every line break (CRLF counts as one). Trailing empty lines
/// are dropped.
fn lines_of(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .replace("\r\n", "\n")
        .split(LINE_BREAKS)
        .map(str::to_owned)
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The workflow command that shows `finding` on the pull request:
/// `::error file=<file>,line=<line>,title=<check>::<message>`.
///
/// Without a file the command is `::error title=<check>::<message>`. `%`, CR and LF in the
/// message are escaped as `%25`, `%0D` and `%0A`, and the other Unicode line breaks become a
/// space, so the command is always one line. The file also has `:` and `,` escaped, which
/// would otherwise end its property.
#[must_use]
pub fn annotation(check: Check, finding: &Finding) -> String {
    let mut command = String::from("::error ");
    if let Some(file) = &finding.file {
        command.push_str("file=");
        command.push_str(&escape_property(file));
        command.push(',');
        if let Some(line) = finding.line {
            // Writing to a `String` cannot fail.
            let _ = write!(command, "line={line},");
        }
    }
    command.push_str("title=");
    command.push_str(check.name());
    command.push_str("::");
    command.push_str(&escape_data(&finding.message));
    command
}

/// The verdicts as markdown: a table with a row per check, then a section for every check that
/// has findings or notes. This is what goes to `$GITHUB_STEP_SUMMARY`.
#[must_use]
pub fn summary_markdown(verdicts: &[Verdict]) -> String {
    let mut text = String::from("### xtask\n\n| Check | Result |\n|---|---|\n");
    for verdict in verdicts {
        let status = if !verdict.passed() {
            "**FAIL**"
        } else if verdict.is_not_needed() {
            "not needed"
        } else {
            "pass"
        };
        // Writing to a `String` cannot fail.
        let _ = writeln!(text, "| `{}` | {status} |", verdict.check.name());
    }
    text.push('\n');
    for verdict in verdicts {
        if verdict.findings.is_empty() && verdict.notes.is_empty() && !verdict.is_not_needed() {
            continue;
        }
        let _ = write!(text, "#### `{}`\n\n", verdict.check.name());
        if let Some(reason) = &verdict.not_needed {
            let reason = reason.lines().collect::<Vec<_>>().join(" ");
            let _ = write!(text, "Not needed: {reason}\n\n");
        }
        for finding in &verdict.findings {
            let message = finding.message.lines().collect::<Vec<_>>().join(" ");
            match location(finding) {
                Some(place) => {
                    let _ = writeln!(text, "- `{place}`: {message}");
                }
                None => {
                    let _ = writeln!(text, "- {message}");
                }
            }
        }
        if !verdict.findings.is_empty() {
            text.push('\n');
        }
        if !verdict.notes.is_empty() {
            text.push_str("```text\n");
            for note in &verdict.notes {
                text.push_str(note);
                text.push('\n');
            }
            text.push_str("```\n\n");
        }
    }
    text
}

/// Writes the verdict to `out`, and under GitHub Actions (`ci`) also its annotations and the
/// summary appended to the file named by `$GITHUB_STEP_SUMMARY`.
///
/// # Errors
///
/// Fails when `out` or the summary file cannot be written.
pub fn emit(verdict: &Verdict, ci: bool, out: &mut dyn Write) -> Result<()> {
    let summary = std::env::var_os("GITHUB_STEP_SUMMARY")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    emit_with(verdict, ci, summary.as_deref(), out)
}

/// Writes the verdicts of several checks as [`emit`] writes one: the table with a row for each,
/// and under GitHub Actions (`ci`) the annotations of every finding and one summary with a row
/// per verdict.
///
/// # Errors
///
/// Fails when `out` or the summary file cannot be written.
pub fn emit_all(verdicts: &[Verdict], ci: bool, out: &mut dyn Write) -> Result<()> {
    let summary = std::env::var_os("GITHUB_STEP_SUMMARY")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    emit_all_with(verdicts, ci, summary.as_deref(), out)
}

fn emit_with(
    verdict: &Verdict,
    ci: bool,
    summary: Option<&Path>,
    out: &mut dyn Write,
) -> Result<()> {
    emit_all_with(std::slice::from_ref(verdict), ci, summary, out)
}

fn emit_all_with(
    verdicts: &[Verdict],
    ci: bool,
    summary: Option<&Path>,
    out: &mut dyn Write,
) -> Result<()> {
    write_table(verdicts, out).map_err(output_error)?;
    if ci {
        for verdict in verdicts {
            for finding in &verdict.findings {
                writeln!(out, "{}", annotation(verdict.check, finding)).map_err(output_error)?;
            }
        }
        if let Some(path) = summary {
            append(path, &summary_markdown(verdicts))?;
        }
    }
    Ok(())
}

fn append(path: &Path, text: &str) -> Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
}

/// The error for an output that cannot be written.
pub(crate) fn output_error(source: io::Error) -> Error {
    Error::Io {
        path: PathBuf::from("<output>"),
        source,
    }
}

/// `file:line`, or `file`, or nothing when the finding has no file.
fn location(finding: &Finding) -> Option<String> {
    let file = finding.file.as_ref()?;
    Some(match finding.line {
        Some(line) => format!("{file}:{line}"),
        None => file.clone(),
    })
}

fn escape_data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        // CR and LF are gone by now; this takes the remaining line breaks.
        .replace(LINE_BREAKS, " ")
}

fn escape_property(text: &str) -> String {
    escape_data(text).replace(':', "%3A").replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn failing() -> Verdict {
        Verdict::with(
            Check::Lints,
            vec![
                Finding::at("crates/a/src/lib.rs", 3, "allow attribute"),
                Finding::new("no clippy.toml"),
            ],
        )
    }

    fn table(verdicts: &[Verdict]) -> TestResult<String> {
        let mut out = Vec::new();
        write_table(verdicts, &mut out)?;
        Ok(String::from_utf8(out)?)
    }

    #[test]
    fn annotation_escapes_newlines_and_percent() {
        let finding = Finding::at("a.rs", 2, "a%b\nc\rd");
        assert_eq!(
            annotation(Check::Lints, &finding),
            "::error file=a.rs,line=2,title=lints::a%25b%0Ac%0Dd"
        );
    }

    #[test]
    fn annotation_is_one_line_whatever_the_line_breaks() {
        let finding = Finding::new("a\u{b}b\u{c}c\u{85}d\u{2028}e\u{2029}f");
        assert_eq!(
            annotation(Check::Lints, &finding),
            "::error title=lints::a b c d e f"
        );
    }

    #[test]
    fn annotation_without_file() {
        assert_eq!(
            annotation(Check::PrTitle, &Finding::new("missing scope")),
            "::error title=pr-title::missing scope"
        );
    }

    #[test]
    fn annotation_without_line_names_only_the_file() {
        let finding = Finding {
            file: Some("Cargo.toml".to_owned()),
            line: None,
            message: "no version".to_owned(),
        };
        assert_eq!(
            annotation(Check::Lints, &finding),
            "::error file=Cargo.toml,title=lints::no version"
        );
    }

    #[test]
    fn annotation_escapes_separators_in_the_file() {
        let finding = Finding::at("odd,name:x.rs", 1, "m");
        assert_eq!(
            annotation(Check::Lints, &finding),
            "::error file=odd%2Cname%3Ax.rs,line=1,title=lints::m"
        );
    }

    #[test]
    fn table_lists_failures_with_locations() -> TestResult {
        let text = table(&[Verdict::pass(Check::Fmt), failing()])?;
        assert_eq!(
            text,
            "pass  fmt\nFAIL  lints\n  - crates/a/src/lib.rs:3: allow attribute\n  - no clippy.toml\n"
        );
        Ok(())
    }

    #[test]
    fn table_continues_a_multi_line_message_on_marked_lines() -> TestResult {
        let finding = Finding::at("a.rs", 1, "first\nsecond\n\nfourth\n");
        let text = table(&[Verdict::with(Check::PrBody, vec![finding])])?;
        assert_eq!(
            text,
            "FAIL  pr-body\n  - a.rs:1: first\n    | second\n    | \n    | fourth\n"
        );
        Ok(())
    }

    #[test]
    fn an_error_continues_on_marked_lines() -> TestResult {
        let mut out = Vec::new();
        write_error(&mut out, &"a\n::error::x.rs: gone\r\nlast\n")?;
        let text = String::from_utf8(out)?;
        assert_eq!(text, "error: a\n  | ::error::x.rs: gone\n  | last\n");
        assert_eq!(command_lines(&text), Vec::<&str>::new());
        Ok(())
    }

    /// Lines a reader of the output could take for a workflow command: they start with `::` once
    /// leading whitespace is gone. Lines end at CR, LF and the other Unicode line breaks.
    fn command_lines(text: &str) -> Vec<&str> {
        text.split([
            '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
        ])
        .filter(|line| line.trim_start().starts_with("::"))
        .collect()
    }

    fn hostile() -> Verdict {
        Verdict {
            not_needed: None,
            check: Check::CommitMsg,
            findings: vec![
                Finding::new("x\n::error::forged"),
                Finding::new("::stop-commands::token"),
                Finding::new("  ::add-mask::value"),
                Finding::new("cr only\r::warning::forged"),
                Finding::new("crlf\r\n::notice::forged"),
                Finding::new("unicode\u{2028}::error::forged"),
                Finding::new("next line\u{85}::error::forged"),
                Finding::at("::error::file", 1, "message"),
                Finding::at("name\n::error::forged.rs", 2, "message"),
                Finding {
                    file: Some("::add-mask::x".to_owned()),
                    line: None,
                    message: "message".to_owned(),
                },
            ],
            notes: vec![
                "note first\n::error::in a note".to_owned(),
                "::notice::note".to_owned(),
            ],
        }
    }

    #[test]
    fn table_cannot_form_workflow_commands() -> TestResult {
        let text = table(&[hostile()])?;
        assert_eq!(command_lines(&text), Vec::<&str>::new(), "{text}");
        assert!(text.contains("::error::forged"), "kept readable: {text}");
        assert!(
            text.contains("::stop-commands::token"),
            "kept readable: {text}"
        );
        // Only the newlines the table writes itself end a line.
        for line_break in ['\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'] {
            assert!(!text.contains(line_break), "{line_break:?} in {text:?}");
        }
        Ok(())
    }

    #[test]
    fn table_keeps_the_forged_command_example_out_of_column_zero() -> TestResult {
        let finding = Finding::new("x\n::error::forged");
        let text = table(&[Verdict::with(Check::CommitMsg, vec![finding])])?;
        assert_eq!(text, "FAIL  commit-msg\n  - x\n    | ::error::forged\n");
        Ok(())
    }

    #[test]
    fn table_shows_a_finding_with_only_a_file() -> TestResult {
        let finding = Finding {
            file: Some("Cargo.toml".to_owned()),
            line: None,
            message: "no version".to_owned(),
        };
        let text = table(&[Verdict::with(Check::Lints, vec![finding])])?;
        assert_eq!(text, "FAIL  lints\n  - Cargo.toml: no version\n");
        Ok(())
    }

    #[test]
    fn table_shows_notes_under_their_check() -> TestResult {
        let mut verdict = Verdict::pass(Check::AcTrace);
        verdict
            .notes
            .push("TU-00.1\n  workspace_members".to_owned());
        let text = table(&[verdict])?;
        assert_eq!(
            text,
            "pass  ac-trace\n  note: TU-00.1\n    |   workspace_members\n"
        );
        Ok(())
    }

    #[test]
    fn summary_markdown_has_a_row_per_check_and_a_section_per_finding_list() {
        let text = summary_markdown(&[Verdict::pass(Check::Fmt), failing()]);
        assert_eq!(
            text,
            "### xtask\n\
             \n\
             | Check | Result |\n\
             |---|---|\n\
             | `fmt` | pass |\n\
             | `lints` | **FAIL** |\n\
             \n\
             #### `lints`\n\
             \n\
             - `crates/a/src/lib.rs:3`: allow attribute\n\
             - no clippy.toml\n\
             \n"
        );
    }

    #[test]
    fn summary_markdown_puts_notes_in_a_text_block() {
        let mut verdict = Verdict::pass(Check::AcTrace);
        verdict.notes.push("TU-00.1 -> a_test".to_owned());
        let text = summary_markdown(&[verdict]);
        assert!(
            text.contains("#### `ac-trace`\n\n```text\nTU-00.1 -> a_test\n```\n"),
            "{text}"
        );
    }

    #[test]
    fn summary_markdown_keeps_a_finding_on_one_line() {
        let text = summary_markdown(&[Verdict::with(
            Check::PrBody,
            vec![Finding::new("first\nsecond")],
        )]);
        assert!(text.contains("- first second\n"), "{text}");
    }

    #[test]
    fn emit_outside_ci_writes_the_table_only() -> TestResult {
        let summary = tempfile::NamedTempFile::new()?;
        let mut out = Vec::new();
        emit_with(&failing(), false, Some(summary.path()), &mut out)?;
        assert_eq!(
            String::from_utf8(out)?,
            "FAIL  lints\n  - crates/a/src/lib.rs:3: allow attribute\n  - no clippy.toml\n"
        );
        assert_eq!(std::fs::read_to_string(summary.path())?, "");
        Ok(())
    }

    #[test]
    fn emit_outside_ci_forms_no_workflow_command() -> TestResult {
        let mut out = Vec::new();
        emit_with(&hostile(), false, None, &mut out)?;
        let text = String::from_utf8(out)?;
        assert_eq!(command_lines(&text), Vec::<&str>::new(), "{text}");
        Ok(())
    }

    #[test]
    fn emit_in_ci_forms_only_its_own_annotations() -> TestResult {
        let finding = Finding::new("x\n::error::forged");
        let verdict = Verdict::with(Check::CommitMsg, vec![finding]);
        let mut out = Vec::new();
        emit_with(&verdict, true, None, &mut out)?;
        let text = String::from_utf8(out)?;
        assert_eq!(
            command_lines(&text),
            ["::error title=commit-msg::x%0A::error::forged"],
            "{text}"
        );

        let mut out = Vec::new();
        emit_with(&hostile(), true, None, &mut out)?;
        let text = String::from_utf8(out)?;
        let commands = command_lines(&text);
        assert_eq!(commands.len(), hostile().findings.len(), "{text}");
        assert!(
            commands.iter().all(|line| line.starts_with("::error ")),
            "{text}"
        );
        Ok(())
    }

    #[test]
    fn emit_in_ci_adds_annotations_and_appends_the_summary() -> TestResult {
        let summary = tempfile::NamedTempFile::new()?;
        std::fs::write(summary.path(), "earlier step\n")?;
        let mut out = Vec::new();
        emit_with(&failing(), true, Some(summary.path()), &mut out)?;
        let text = String::from_utf8(out)?;
        assert!(text.starts_with("FAIL  lints\n"), "{text}");
        assert!(
            text.ends_with(
                "::error file=crates/a/src/lib.rs,line=3,title=lints::allow attribute\n\
                 ::error title=lints::no clippy.toml\n"
            ),
            "{text}"
        );
        let written = std::fs::read_to_string(summary.path())?;
        assert!(
            written.starts_with("earlier step\n### xtask\n"),
            "{written}"
        );
        assert!(written.contains("| `lints` | **FAIL** |"), "{written}");
        Ok(())
    }

    #[test]
    fn emit_in_ci_without_a_summary_file_still_annotates() -> TestResult {
        let mut out = Vec::new();
        emit_with(&failing(), true, None, &mut out)?;
        assert!(String::from_utf8(out)?.contains("::error "));
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn a_not_needed_part_passes_and_shows_its_reason() -> TestResult {
        let verdict = Verdict::not_needed(Check::Fmt, "only docs changed (gate/code-paths.toml)");
        assert!(verdict.passed() && verdict.is_not_needed());
        let table = table(std::slice::from_ref(&verdict))?;
        assert_eq!(
            table,
            "skip  fmt
  note: not needed: only docs changed (gate/code-paths.toml)
"
        );
        let summary = summary_markdown(&[verdict]);
        assert!(summary.contains("| `fmt` | not needed |"), "{summary}");
        assert!(summary.contains("only docs changed"), "{summary}");
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn the_gate_writes_one_summary_with_every_part() -> TestResult {
        let summary = tempfile::NamedTempFile::new()?;
        let verdicts = [
            Verdict::pass(Check::Tools),
            failing(),
            Verdict::not_needed(Check::Fmt, "docs only"),
        ];
        let mut out = Vec::new();
        emit_all_with(&verdicts, true, Some(summary.path()), &mut out)?;
        let text = String::from_utf8(out)?;
        assert!(
            text.starts_with(
                "pass  tools
FAIL  lints
"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "::error title=lints::no clippy.toml
"
            ),
            "{text}"
        );
        let written = std::fs::read_to_string(summary.path())?;
        assert_eq!(written.matches("### xtask").count(), 1, "{written}");
        for row in [
            "| `tools` | pass |",
            "| `lints` | **FAIL** |",
            "| `fmt` | not needed |",
        ] {
            assert!(written.contains(row), "{row} missing from {written}");
        }
        Ok(())
    }

    #[test]
    fn emit_reports_an_unwritable_summary_as_an_io_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        let mut out = Vec::new();
        let result = emit_with(&failing(), true, Some(dir.path()), &mut out);
        assert!(matches!(result, Err(crate::Error::Io { .. })), "{result:?}");
        Ok(())
    }
}
