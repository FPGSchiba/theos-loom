//! The `pr-body` check: the description of a pull request has every section of the pull request
//! template, and each one has content (`docs/gate.md` §1).
//!
//! Template and description are read with `pulldown-cmark` (`CommonMark`, [`crate::markdown`]), so
//! a heading inside a code block or an HTML comment is no heading. The sections are the level-two
//! headings of `.github/pull_request_template.md`. Each must appear in the description, in any
//! order and with extra sections allowed, and it has content when it holds text or code other than
//! HTML comments and whitespace, so a description that keeps only the comments of the template
//! fails. Headings match by their text after inline parsing, so trailing spaces and a closing
//! sequence do not matter. A missing description is an empty one. The core is [`check_body`]: a
//! template and a description in, a verdict out. [`run`] takes the description from the pull
//! request event. A byte order mark and CRLF line ends are not read as text: Windows tools write
//! the first and the web editor the second.
//!
//! Findings name headings only. The description is written by whoever opened the pull request, and
//! the runner reads workflow commands in what a check prints.

use std::io::Write;
use std::path::Path;

use crate::ci::CiEvent;
use crate::markdown::{self, Section};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The pull request template, relative to the repository root.
const TEMPLATE_PATH: &str = ".github/pull_request_template.md";

/// The level-two headings of `template` as text, in order.
///
/// Text before the first heading belongs to no section, and a heading in a code block or an HTML
/// comment is not one.
#[must_use]
pub fn template_sections(template: &str) -> Vec<String> {
    markdown::sections(template, 2)
        .into_iter()
        .map(|section| section.heading.text)
        .collect()
}

/// Checks the description `body` against the sections of `template`: one finding for each section
/// of the template that the description lacks or leaves empty, in the order of the template.
///
/// `None`, an empty body and one of whitespace alike lack every section. A template without
/// sections asks for nothing; [`read_template`] refuses one.
#[must_use]
pub fn check_body(template: &str, body: Option<&str>) -> Verdict {
    let body = body.unwrap_or_default();
    let found = markdown::sections(body, 2);
    let findings = template_sections(template)
        .iter()
        .filter_map(|heading| problem(heading, &found))
        .map(Finding::new)
        .collect();
    Verdict::with(Check::PrBody, findings)
}

/// Reads the template from `<root>/.github/pull_request_template.md`.
///
/// # Errors
///
/// Fails when the file cannot be read, or when it has no `## ` section: the check would pass
/// every description.
pub fn read_template(root: &Path) -> Result<String> {
    let path = root.join(TEMPLATE_PATH);
    let template = std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
    if template_sections(&template).is_empty() {
        return Err(Error::Config {
            file: TEMPLATE_PATH.to_owned(),
            message: "the template has no `## ` section, so every description would pass"
                .to_owned(),
        });
    }
    Ok(template)
}

/// Runs the check on the description of the pull request event, against the template of `root`.
///
/// # Errors
///
/// Fails when there is no pull request event: a description only exists on a pull request, so a
/// local run has to pass `--body-file`. Also when the template cannot be used, see
/// [`read_template`].
pub fn run(root: &Path, ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    let Some(CiEvent::PullRequest(pull_request)) = ci else {
        return Err(Error::Missing {
            what: "the pull request description: there is no pull request event, so pass \
                   `--body-file`"
                .to_owned(),
        });
    };
    let template = read_template(root)?;
    Ok(check_body(&template, pull_request.body.as_deref()))
}

/// What is wrong with the section `heading` among the sections `found`, if anything. A heading
/// that occurs more than once is filled when one of its sections is.
fn problem(heading: &str, found: &[Section<'_>]) -> Option<String> {
    let named: Vec<&Section<'_>> = found
        .iter()
        .filter(|section| section.heading.text == heading)
        .collect();
    if named.is_empty() {
        Some(format!(
            "the description has no `## {heading}` section: add it, as \
             `.github/pull_request_template.md` has it"
        ))
    } else if named
        .iter()
        .all(|section| !markdown::has_content(section.body))
    {
        Some(format!(
            "the `## {heading}` section is empty: HTML comments and blank lines do not count, so \
             write its content under the heading"
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::{PullRequestEvent, PushEvent};

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// The template of this repository, which the check reads in CI.
    const REAL_TEMPLATE: &str = include_str!("../../../.github/pull_request_template.md");

    /// Its sections, as the check must find them.
    const HEADINGS: [&str; 7] = [
        "Phase",
        "Acceptance criteria",
        "Final review",
        "Rulings",
        "Gate",
        "Protected paths",
        "Trial",
    ];

    /// A description with a section for each pair: the heading and what is under it.
    fn body_of(sections: &[(&str, &str)]) -> String {
        sections
            .iter()
            .map(|(heading, content)| format!("## {heading}\n\n{content}\n\n"))
            .collect::<Vec<_>>()
            .concat()
    }

    /// A description with all of `headings`, each with some text under it.
    fn filled(headings: &[&str]) -> String {
        let sections: Vec<(&str, &str)> =
            headings.iter().map(|heading| (*heading, "Done.")).collect();
        body_of(&sections)
    }

    /// The same, for every section of the real template.
    fn full_body() -> String {
        filled(&HEADINGS)
    }

    /// The headings of `HEADINGS` except `left_out`.
    fn all_but(left_out: &str) -> Vec<&'static str> {
        HEADINGS
            .into_iter()
            .filter(|heading| *heading != left_out)
            .collect()
    }

    fn messages(verdict: &Verdict) -> Vec<&str> {
        verdict
            .findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect()
    }

    fn pull_request(body: Option<&str>) -> CiEvent {
        CiEvent::PullRequest(PullRequestEvent {
            action: "edited".to_owned(),
            number: 1,
            title: "feat(xtask): x (P00)".to_owned(),
            body: body.map(str::to_owned),
            base_sha: "a".repeat(40),
            head_sha: "b".repeat(40),
            head_ref: "phase/P00-x".to_owned(),
            label: None,
        })
    }

    /// A repository root with `template` where the check looks for it.
    fn root_with(template: &str) -> TestResult<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join(".github"))?;
        std::fs::write(
            dir.path().join(".github/pull_request_template.md"),
            template,
        )?;
        Ok(dir)
    }

    fn run_with(root: &Path, ci: Option<&CiEvent>) -> Result<Verdict> {
        run(root, ci, &mut Vec::new())
    }

    #[test]
    fn the_sections_are_the_level_two_headings_of_the_template() {
        assert_eq!(template_sections(REAL_TEMPLATE), HEADINGS);
        // Trailing spaces, CRLF and a heading that is only a comment do not matter; `###` and
        // `##` without a space are not sections; text before the first heading is no section.
        let template =
            "intro\n## A  \n### Not\n## B\r\n<!-- ## Hidden -->\n##C\n<!--\n## Also hidden\n-->\n";
        assert_eq!(template_sections(template), ["A", "B"]);
        assert_eq!(template_sections("no sections\n"), Vec::<String>::new());
    }

    #[test]
    fn full_body_passes() {
        let verdict = check_body(REAL_TEMPLATE, Some(&full_body()));
        assert_eq!(verdict.check, Check::PrBody);
        assert!(verdict.passed(), "{:?}", verdict.findings);
        // The comments of the template may stay, as long as there is text next to them.
        let kept_comments = REAL_TEMPLATE.replace("-->", "-->\n\nDone.");
        let verdict = check_body(REAL_TEMPLATE, Some(&kept_comments));
        assert!(verdict.passed(), "{:?}", verdict.findings);
    }

    #[test]
    fn missing_section_fails() {
        let verdict = check_body(REAL_TEMPLATE, Some(&filled(&all_but("Gate"))));
        assert_eq!(verdict.check, Check::PrBody);
        let found = messages(&verdict);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("no `## Gate` section"), "{found:?}");
        // Every section is needed, not just that one.
        for heading in HEADINGS {
            let verdict = check_body(REAL_TEMPLATE, Some(&filled(&all_but(heading))));
            let found = messages(&verdict);
            assert_eq!(found.len(), 1, "{heading}: {found:?}");
            assert!(found[0].contains(heading), "{heading}: {found:?}");
        }
    }

    #[test]
    fn placeholder_only_section_fails() {
        // Every section has text but the gate's, which has what is under it in the template.
        let with_gate = |gate: &str| {
            let sections: Vec<(&str, &str)> = HEADINGS
                .iter()
                .map(|heading| (*heading, if *heading == "Gate" { gate } else { "Done." }))
                .collect();
            body_of(&sections)
        };
        let placeholder = "<!-- Summary of `cargo xtask gate` on the final commit. -->";
        let verdict = check_body(REAL_TEMPLATE, Some(&with_gate(placeholder)));
        let found = messages(&verdict);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("`## Gate` section is empty"), "{found:?}");
        // A comment over several lines is a placeholder too, and so is nothing but blank space.
        let multi_line = "<!--\n  Summary of\n  the gate.\n-->\n\n  \t\n";
        let verdict = check_body(REAL_TEMPLATE, Some(&with_gate(multi_line)));
        assert_eq!(messages(&verdict).len(), 1, "{:?}", verdict.findings);
        let verdict = check_body(REAL_TEMPLATE, Some(&with_gate(" \t ")));
        assert_eq!(messages(&verdict).len(), 1, "{:?}", verdict.findings);
        // The template as the description is all placeholders: every section is empty.
        let verdict = check_body(REAL_TEMPLATE, Some(REAL_TEMPLATE));
        assert_eq!(
            verdict.findings.len(),
            HEADINGS.len(),
            "{:?}",
            verdict.findings
        );
    }

    #[test]
    fn crlf_body_passes() {
        let crlf = full_body().replace('\n', "\r\n");
        assert!(crlf.contains("\r\n"));
        let verdict = check_body(REAL_TEMPLATE, Some(&crlf));
        assert!(verdict.passed(), "{:?}", verdict.findings);
        // A template checked out with CRLF line ends gives the same sections.
        let crlf_template = REAL_TEMPLATE.replace('\n', "\r\n");
        assert_eq!(template_sections(&crlf_template), HEADINGS);
        let verdict = check_body(&crlf_template, Some(&crlf));
        assert!(verdict.passed(), "{:?}", verdict.findings);
        // The placeholders of a CRLF description are still placeholders.
        let verdict = check_body(REAL_TEMPLATE, Some(&crlf_template));
        assert_eq!(
            verdict.findings.len(),
            HEADINGS.len(),
            "{:?}",
            verdict.findings
        );
    }

    #[test]
    fn reordered_sections_pass() {
        let mut reversed = HEADINGS;
        reversed.reverse();
        let verdict = check_body(REAL_TEMPLATE, Some(&filled(&reversed)));
        assert!(verdict.passed(), "{:?}", verdict.findings);
    }

    #[test]
    fn null_body_fails_every_section() {
        for body in [None, Some(""), Some("  \n\t\r\n"), Some("<!-- nothing -->")] {
            let verdict = check_body(REAL_TEMPLATE, body);
            assert_eq!(verdict.check, Check::PrBody);
            let found = messages(&verdict);
            assert_eq!(found.len(), HEADINGS.len(), "{body:?}: {found:?}");
            for (message, heading) in found.iter().zip(HEADINGS) {
                assert!(message.contains(heading), "{body:?}: {message}");
            }
        }
    }

    #[test]
    fn headings_match_after_trimming_and_extra_sections_do_not_matter() {
        let spaced = full_body().replace("## Gate", "## Gate   ");
        assert!(check_body(REAL_TEMPLATE, Some(&spaced)).passed());
        let extra = format!("{}## Notes\n\nmore\n\n## Another\n", full_body());
        assert!(check_body(REAL_TEMPLATE, Some(&extra)).passed());
        // Text before the first heading is not a section.
        let intro = format!("Closes nothing.\n\n{}", full_body());
        assert!(check_body(REAL_TEMPLATE, Some(&intro)).passed());
        // The match is exact otherwise: other case or a level-three heading is another heading.
        let lower = full_body().replace("## Gate", "## gate");
        assert_eq!(check_body(REAL_TEMPLATE, Some(&lower)).findings.len(), 1);
        let deeper = full_body().replace("## Gate", "### Gate");
        assert_eq!(check_body(REAL_TEMPLATE, Some(&deeper)).findings.len(), 1);
    }

    #[test]
    fn a_heading_in_a_comment_is_no_section() {
        let hidden = format!("{}<!--\n## Gate\n\nDone.\n-->\n", filled(&all_but("Gate")));
        let verdict = check_body(REAL_TEMPLATE, Some(&hidden));
        let found = messages(&verdict);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("no `## Gate` section"), "{found:?}");
        // A comment that is not closed runs to the end, and takes the sections after it.
        let unclosed = full_body().replace("Done.\n\n## Trial", "<!-- Done.\n\n## Trial");
        let verdict = check_body(REAL_TEMPLATE, Some(&unclosed));
        assert_eq!(verdict.findings.len(), 2, "{:?}", verdict.findings);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_heading() {
        let with_mark = format!("\u{feff}{}", full_body());
        let verdict = check_body(REAL_TEMPLATE, Some(&with_mark));
        assert!(verdict.passed(), "{:?}", verdict.findings);
    }

    #[test]
    fn a_heading_that_comes_twice_needs_content_under_one_of_them() {
        let twice = format!("## Gate\n\n{}", full_body());
        assert!(check_body(REAL_TEMPLATE, Some(&twice)).passed());
        let empty_twice = format!("## Gate\n\n## Gate\n\n{}", filled(&all_but("Gate")));
        let verdict = check_body(REAL_TEMPLATE, Some(&empty_twice));
        let found = messages(&verdict);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("`## Gate` section is empty"), "{found:?}");
    }

    #[test]
    fn findings_name_headings_and_nothing_from_the_body() {
        let body = "## Phase\n\nSECRET ::error::forged\n";
        let verdict = check_body(REAL_TEMPLATE, Some(body));
        assert_eq!(verdict.findings.len(), HEADINGS.len() - 1);
        for finding in &verdict.findings {
            assert!(!finding.message.contains("SECRET"), "{finding:?}");
            assert!(!finding.message.contains("forged"), "{finding:?}");
        }
    }

    #[test]
    fn a_pull_request_event_gives_the_description() -> TestResult {
        let root = root_with(REAL_TEMPLATE)?;
        let good = pull_request(Some(&full_body()));
        let verdict = run_with(root.path(), Some(&good))?;
        assert_eq!(verdict.check, Check::PrBody);
        assert!(verdict.passed(), "{:?}", verdict.findings);

        let missing_gate = pull_request(Some(&filled(&all_but("Gate"))));
        assert_eq!(
            run_with(root.path(), Some(&missing_gate))?.findings.len(),
            1
        );
        // An empty description reaches the check as `None` or as an empty string.
        for body in [None, Some("")] {
            let empty = pull_request(body);
            let verdict = run_with(root.path(), Some(&empty))?;
            assert_eq!(verdict.findings.len(), HEADINGS.len(), "{body:?}");
        }
        Ok(())
    }

    #[test]
    fn the_template_of_the_root_decides_the_sections() -> TestResult {
        let root = root_with("## Only\n\n<!-- say it -->\n")?;
        let good = pull_request(Some("## Only\n\nsaid\n"));
        assert!(run_with(root.path(), Some(&good))?.passed());
        let bad = pull_request(Some(&full_body()));
        assert_eq!(run_with(root.path(), Some(&bad))?.findings.len(), 1);
        Ok(())
    }

    #[test]
    fn without_a_pull_request_there_is_no_description_to_check() -> TestResult {
        let root = root_with(REAL_TEMPLATE)?;
        let push = CiEvent::Push(PushEvent {
            before: "a".repeat(40),
            after: "b".repeat(40),
        });
        for ci in [None, Some(&push), Some(&CiEvent::Other)] {
            let result = run_with(root.path(), ci);
            assert!(
                matches!(&result, Err(Error::Missing { what }) if what.contains("--body-file")),
                "{result:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_template_that_cannot_be_read_is_an_io_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        let result = read_template(dir.path());
        assert!(matches!(&result, Err(Error::Io { .. })), "{result:?}");
        let event = pull_request(Some(&full_body()));
        let result = run_with(dir.path(), Some(&event));
        assert!(matches!(&result, Err(Error::Io { .. })), "{result:?}");
        Ok(())
    }

    #[test]
    fn a_template_without_sections_is_a_config_error() -> TestResult {
        // It would pass every description.
        let root = root_with("# Title only\n\n<!-- nothing to fill -->\n")?;
        let result = read_template(root.path());
        assert!(matches!(&result, Err(Error::Config { .. })), "{result:?}");
        let event = pull_request(Some(&full_body()));
        let result = run_with(root.path(), Some(&event));
        assert!(matches!(&result, Err(Error::Config { .. })), "{result:?}");
        Ok(())
    }

    #[test]
    fn the_real_template_is_usable() -> TestResult {
        let root = root_with(REAL_TEMPLATE)?;
        assert_eq!(read_template(root.path())?, REAL_TEMPLATE);
        Ok(())
    }

    #[test]
    fn a_heading_inside_a_code_block_is_no_section() {
        let hidden = format!(
            "{}```
## Gate

Done.
```
",
            filled(&all_but("Gate"))
        );
        let verdict = check_body(REAL_TEMPLATE, Some(&hidden));
        let found = messages(&verdict);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("no `## Gate` section"), "{found:?}");
        // The same in the template: a fenced heading asks for nothing.
        let template = "## A

```
## Fenced
```
";
        assert_eq!(template_sections(template), ["A"]);
    }

    #[test]
    fn a_heading_inside_an_html_comment_is_no_section() {
        let hidden = format!(
            "{}<!--
## Gate

Done.
-->
",
            filled(&all_but("Gate"))
        );
        let verdict = check_body(REAL_TEMPLATE, Some(&hidden));
        let found = messages(&verdict);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("no `## Gate` section"), "{found:?}");
    }

    #[test]
    fn a_heading_with_a_closing_sequence_or_trailing_spaces_matches() {
        let closed = full_body().replace("## Gate", "## Gate ##");
        assert!(check_body(REAL_TEMPLATE, Some(&closed)).passed());
        let spaced = full_body().replace("## Gate", "##   Gate   ");
        assert!(check_body(REAL_TEMPLATE, Some(&spaced)).passed());
        let template = "## A ##

## B   
";
        assert_eq!(template_sections(template), ["A", "B"]);
    }

    #[test]
    fn a_section_whose_only_content_is_a_code_block_is_filled() {
        let sections: Vec<(&str, &str)> = HEADINGS
            .iter()
            .map(|heading| {
                (
                    *heading,
                    "```
cargo xtask gate
```",
                )
            })
            .collect();
        let verdict = check_body(REAL_TEMPLATE, Some(&body_of(&sections)));
        assert!(verdict.passed(), "{:?}", verdict.findings);
    }
}
