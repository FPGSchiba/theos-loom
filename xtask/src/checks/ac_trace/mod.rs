//! The `ac-trace` check: every acceptance criterion of the phase has a test that proves it, and
//! every tag names a criterion that exists (`docs/gate.md` §4).
//!
//! A test proves criteria by a `// AC: <id>[, <id>]*` comment directly above it, in Rust
//! ([`rust_tags`]) and in TypeScript ([`ts_tags`], which covers Vitest and Playwright). The core
//! is [`check`]: the umbrella spec, the phase and the tags in, a verdict and a table of criteria
//! out. [`run`] and [`run_phase`] gather the tags from the repository, the phase from the branch
//! and the test results from the nextest `JUnit` report ([`junit`]).
//!
//! The table is for people. Its lines come from the pull request (paths and test titles), and
//! the runner reads a line that starts with `::` as a workflow command, so every line starts with
//! fixed text and the text from sources has its control characters replaced.

pub mod junit;
pub mod phase_spec;
pub mod rust_tags;
pub mod ts_tags;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::branch::{Phase, parse_branch, phase_of};
use crate::ci::CiEvent;
use crate::files::{extension, is_fixture, repo_files};
use crate::git::Git;
use crate::umbrella::{self, Umbrella};
use crate::verdict::{Check, Finding, Verdict};
use crate::view::{Shared, View};

use crate::{Error, Result};
pub use phase_spec::Scope;

/// A `// AC: <id>[, <id>]*` comment that is directly above a test.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tag {
    /// The file, repo-relative with `/`.
    pub file: String,
    /// The line of the comment, counting from 1.
    pub line: u32,
    /// The ids the comment names, in the order written.
    pub ids: Vec<String>,
    /// The name of the test function; in TypeScript the title of the test, when it is a plain
    /// string.
    pub test: Option<String>,
    /// Whether the test proves its criteria: it is not ignored or skipped, and no `cfg` can
    /// switch it off.
    pub counts: bool,
}

/// Compares the tags with the umbrella spec and writes the table of criteria to `out`.
///
/// Every id of a tag must be in the umbrella. With a `phase` built in this repository every
/// criterion of the `scope` (the phase spec's list, or else the phase's in the umbrella) also
/// needs a tag that counts; a phase built in another repository
/// demands nothing, and without a phase only the rules for tags apply. When `junit` has the
/// results of the last test run, the table gets a result column; it informs and decides
/// nothing, because the `test` check fails for a failing test.
///
/// A table that cannot be written is a note: the report writes to the same `out` right after and
/// fails there.
#[must_use]
pub fn check(
    umbrella: &Umbrella,
    phase: Option<Phase>,
    scope: &Scope,
    tags: &[Tag],
    junit: Option<&BTreeMap<String, bool>>,
    out: &mut dyn Write,
) -> Verdict {
    let mut verdict = Verdict::pass(Check::AcTrace);
    let known = umbrella.all_ids();
    for tag in tags {
        for id in tag.ids.iter().filter(|id| !known.contains(*id)) {
            verdict.findings.push(Finding::at(
                &tag.file,
                tag.line,
                format!(
                    "the tag names `{id}`, which is not an acceptance criterion of the umbrella \
                     spec ({})",
                    umbrella::PATH
                ),
            ));
        }
    }

    let (heading, listed) = match phase {
        None => {
            verdict.notes.push(
                "there is no phase (the branch is not `phase/PNN-…`), so only the rules for tags \
                 were checked"
                    .to_owned(),
            );
            let tagged: BTreeSet<&String> = tags
                .iter()
                .flat_map(|tag| &tag.ids)
                .filter(|id| known.contains(*id))
                .collect();
            let heading = format!("no phase; criteria with a tag: {}", tagged.len());
            (heading, tagged.into_iter().cloned().collect())
        }
        Some(phase) => match umbrella.phase(phase) {
            None => {
                verdict.findings.push(Finding::new(format!(
                    "the umbrella spec ({}) has no phase {phase}",
                    umbrella::PATH
                )));
                (phase.to_string(), Vec::new())
            }
            Some(info) if info.repo != umbrella::OWN_REPO => {
                verdict.notes.push(format!(
                    "{phase} is built in {}, where its tests are, so no test is demanded here",
                    info.repo
                ));
                let heading = format!(
                    "{phase} {} ({}); nothing is demanded here",
                    info.name, info.repo
                );
                (heading, Vec::new())
            }
            Some(info) => {
                let (acs, source) = match scope {
                    Scope::Umbrella => (&info.acs, String::new()),
                    Scope::PhaseSpec { file, acs } => (acs, format!(" from {file}")),
                };
                for id in acs.iter().filter(|id| !proven(tags, id)) {
                    verdict.findings.push(Finding::new(format!(
                        "{id} of {phase} has no test that counts: put `// AC: {id}` directly above \
                         a test that is not ignored and not behind a `cfg` other than the target \
                         OS"
                    )));
                }
                let heading = format!(
                    "{phase} {} ({}); criteria{source}: {}",
                    info.name,
                    info.repo,
                    acs.len()
                );
                (heading, acs.clone())
            }
        },
    };

    let mut lines = vec![format!(
        "ac-trace: {heading}{}",
        if junit.is_some() {
            "; results of the last test run"
        } else {
            ""
        }
    )];
    for id in &listed {
        lines.extend(
            tags.iter()
                .filter(|tag| tag.ids.contains(id))
                .map(|tag| row(id, tag, junit)),
        );
        if !proven(tags, id) {
            let status = junit.map(|_| NO_RESULT);
            lines.push(line(id, status, "no counting test"));
        }
    }
    for text in &lines {
        if let Err(error) = writeln!(out, "{text}") {
            verdict.notes.push(format!(
                "the table of criteria could not be written: {error}"
            ));
            break;
        }
    }
    verdict
}

/// Whether some tag that counts names `id`.
fn proven(tags: &[Tag], id: &str) -> bool {
    tags.iter()
        .any(|tag| tag.counts && tag.ids.iter().any(|named| named == id))
}

/// The result column of a test that has no result.
const NO_RESULT: &str = "–";

/// The table row of `tag`, which names `id`.
fn row(id: &str, tag: &Tag, junit: Option<&BTreeMap<String, bool>>) -> String {
    let mut place = format!("{}:{}", printable(&tag.file), tag.line);
    if let Some(test) = &tag.test {
        place.push(' ');
        place.push_str(&printable(test));
    }
    if !tag.counts {
        place.push_str(" (does not count)");
    }
    let status = junit.map(|junit| {
        let result = tag
            .test
            .as_deref()
            .filter(|_| tag.counts)
            .and_then(|test| result_of(junit, test));
        match result {
            Some(true) => "pass",
            Some(false) => "FAIL",
            None => NO_RESULT,
        }
    });
    line(id, status, &place)
}

/// One line of the table: the id, the result column when there is one, and the text.
fn line(id: &str, status: Option<&str>, text: &str) -> String {
    match status {
        Some(status) => format!("{id:<9} {status:<4}  {text}"),
        None => format!("{id:<9} {text}"),
    }
}

/// Whether the test `name` passed, or `None` when the report has no such test.
///
/// A test of the report is the one when its name is `name` or ends with `::name`. When several
/// are, the test passed only if all of them did.
fn result_of(junit: &BTreeMap<String, bool>, name: &str) -> Option<bool> {
    let mut result = None;
    for (case, passed) in junit {
        let is_it = case
            .strip_suffix(name)
            .is_some_and(|head| head.is_empty() || head.ends_with("::"));
        if is_it {
            result = Some(result.unwrap_or(true) && *passed);
        }
    }
    result
}

/// `text` with every control character and line break replaced by `?`.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// Runs the check on the repository at `root`, for the phase of the branch.
///
/// The branch is the head branch of a pull request event, or else the one that is checked out;
/// only `phase/PNN-…` has a phase. The results are read from the report of the profile `ci`
/// under GitHub Actions and `default` elsewhere. The Rust tests come from the workspace view of
/// `shared`.
///
/// # Errors
///
/// Fails when the umbrella spec or a source file cannot be read, the umbrella lists no criteria,
/// the workspace view cannot be built, the report exists and does not parse, or `HEAD` is
/// detached outside a pull request.
pub fn run(
    root: &Path,
    ci: Option<&CiEvent>,
    shared: &Shared,
    out: &mut dyn Write,
) -> Result<Verdict> {
    let branch = match ci {
        Some(CiEvent::PullRequest(pull_request)) => parse_branch(&pull_request.head_ref),
        Some(CiEvent::Push(_) | CiEvent::Other) | None => match Git::new(root).current_branch()? {
            Some(name) => parse_branch(&name),
            None => {
                return Err(Error::Missing {
                    what: "the branch name: HEAD is detached, so pass `--phase PNN`".to_owned(),
                });
            }
        },
    };
    run_phase(root, phase_of(&branch), ci.is_some(), shared, out)
}

/// Runs the check on the repository at `root` for `phase`; `ci` chooses the profile of the test
/// report, and `shared` holds the workspace view, as in [`run`].
///
/// # Errors
///
/// Fails when the umbrella spec or a source file cannot be read, the umbrella lists no criteria,
/// the phase spec of the phase is wrong ([`phase_spec::scope`]), the workspace view cannot be
/// built, or the report exists and does not parse.
pub fn run_phase(
    root: &Path,
    phase: Option<Phase>,
    ci: bool,
    shared: &Shared,
    out: &mut dyn Write,
) -> Result<Verdict> {
    let umbrella = umbrella::load(root)?;
    let (tags, mut findings) = scan_files(root, shared.view(root)?)?;
    let junit = read_junit(root, ci)?;
    let scope = match phase.and_then(|phase| umbrella.phase(phase).map(|info| (phase, info))) {
        Some((phase, info)) if info.repo == umbrella::OWN_REPO => {
            phase_spec::scope(root, &umbrella, phase)?
        }
        _ => Scope::Umbrella,
    };
    let mut verdict = check(&umbrella, phase, &scope, &tags, junit.as_ref(), out);
    // The problems with tags come first, in file order, then the ones about ids and criteria.
    findings.append(&mut verdict.findings);
    verdict.findings = findings;
    Ok(verdict)
}

/// The tags and the malformed or dangling tags of every file that is read.
///
/// The Rust files are those of the workspace `view`, which compile in some target; the findings
/// of the view (files that do not parse) come first. A tag in a file that no test target compiles
/// is dangling. TypeScript files come from the repository ([`is_typescript`]).
fn scan_files(root: &Path, view: &View) -> Result<(Vec<Tag>, Vec<Finding>)> {
    let mut tags = Vec::new();
    let mut findings = view.findings.clone();
    let test_files = view.test_files();
    for source in view.sources() {
        let (found, problems) = rust_tags::scan(source, test_files.contains(source.path.as_str()));
        tags.extend(found);
        findings.extend(problems);
    }
    for path in repo_files(root)?.iter().filter(|path| is_typescript(path)) {
        let full = root.join(path);
        let bytes = fs::read(&full).map_err(|source| Error::Io { path: full, source })?;
        let (found, problems) = ts_tags::scan(path, &String::from_utf8_lossy(&bytes));
        tags.extend(found);
        findings.extend(problems);
    }
    Ok((tags, findings))
}

/// Whether `path` (repo-relative, with `/`) is a TypeScript file that is read: `.ts` or `.tsx`,
/// except fixtures (`tests/fixtures/`) and anything under `target/` or `node_modules/`. Every
/// other folder is read, `web/` and the crates alike.
fn is_typescript(path: &str) -> bool {
    let built = path
        .split('/')
        .any(|part| part == "target" || part == "node_modules");
    matches!(extension(path), Some("ts" | "tsx")) && !is_fixture(path) && !built
}

/// The results of the last test runs, if nextest wrote a report.
///
/// The report of the profile `ci` (under GitHub Actions) or `default`. Without a report there is
/// no result column.
fn read_junit(root: &Path, ci: bool) -> Result<Option<BTreeMap<String, bool>>> {
    let profile = if ci { "ci" } else { "default" };
    let path = root
        .join("target")
        .join("nextest")
        .join(profile)
        .join("junit.xml");
    match fs::read(&path) {
        Ok(bytes) => junit::parse(&String::from_utf8_lossy(&bytes)).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io { path, source }),
    }
}

/// What a line is, as far as tags are concerned.
enum Comment {
    /// A well-formed tag, with its ids.
    Tag(Vec<String>),
    /// A line that starts like a tag and is not one.
    Malformed,
    /// Anything else.
    Other,
}

/// Sorts a source line into a tag, a malformed tag or something else.
///
/// A tag line is exactly `// AC: <id>[, <id>]*`, with white space allowed around it. Any other
/// comment that starts with `AC` as a word, such as `//AC: x` or `// AC - x`, is malformed;
/// `// ACs …` is not, and neither are doc comments (`///`).
fn classify(line: &str) -> Comment {
    let Some(after_slashes) = line.trim_start().strip_prefix("//") else {
        return Comment::Other;
    };
    let Some(after_ac) = after_slashes.trim_start().strip_prefix("AC") else {
        return Comment::Other;
    };
    if after_ac.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return Comment::Other;
    }
    let ids: Option<Vec<String>> = line
        .trim()
        .strip_prefix("// AC: ")
        .map(|list| list.split(", ").map(str::to_owned).collect());
    match ids {
        Some(ids) if ids.iter().all(|id| is_tag_id(id)) => Comment::Tag(ids),
        _ => Comment::Malformed,
    }
}

/// An id as a tag may write it: an acceptance criterion id ([`umbrella::is_ac_id`]). Whether it
/// is a criterion of the spec is for [`check`].
fn is_tag_id(id: &str) -> bool {
    umbrella::is_ac_id(id)
}

/// The number of the line at `index` (counting from 0) in the unit of a finding: from 1.
pub(crate) fn line_number(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

/// The finding for a comment that looks like a tag and is not one.
fn malformed(file: &str, line: u32) -> Finding {
    Finding::at(
        file,
        line,
        "this comment starts like a tag but is not one: a tag is exactly `// AC: <id>`, or \
         `// AC: <id>, <id>` for several, with one space after `//` and after `:`",
    )
}

/// The finding for a tag that is not directly above a test; `why` says what is in the way.
fn dangling(file: &str, line: u32, why: &str) -> Finding {
    Finding::at(
        file,
        line,
        format!("this tag is not directly above a test: {why}"),
    )
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::git::testing::{TestResult, commit, git, init_repo};
    use crate::view::ViewOptions;
    use crate::{Error, umbrella};

    const UMBRELLA: &str =
        include_str!("../../../tests/fixtures/ac-trace/complete-phase/docs/specs/00-umbrella.md");

    /// A holder of a view that builds on a fixture workspace: offline, no `--locked`.
    fn shared() -> Shared {
        Shared::with(ViewOptions::FIXTURE)
    }

    fn phase(text: &str) -> TestResult<Phase> {
        Ok(text.parse()?)
    }

    fn tag(file: &str, line: u32, ids: &[&str], test: Option<&str>, counts: bool) -> Tag {
        Tag {
            file: file.to_owned(),
            line,
            ids: ids.iter().map(|id| (*id).to_owned()).collect(),
            test: test.map(str::to_owned),
            counts,
        }
    }

    fn report(results: &[(&str, bool)]) -> BTreeMap<String, bool> {
        results
            .iter()
            .map(|(name, passed)| ((*name).to_owned(), *passed))
            .collect()
    }

    /// The verdict and the table of a check against the fixture umbrella (P90, two criteria).
    fn run_check(
        phase: Option<Phase>,
        tags: &[Tag],
        junit: Option<&BTreeMap<String, bool>>,
    ) -> TestResult<(Verdict, Vec<String>)> {
        let mut out = Vec::new();
        let verdict = check(
            &umbrella::parse(UMBRELLA)?,
            phase,
            &Scope::Umbrella,
            tags,
            junit,
            &mut out,
        );
        let table = String::from_utf8(out)?.lines().map(str::to_owned).collect();
        Ok((verdict, table))
    }

    // AC: TU-00.4
    #[test]
    fn a_phase_criterion_without_a_counting_tag_is_a_finding() -> TestResult {
        let p90 = Some(phase("P90")?);
        let first = tag("src/a.rs", 3, &["TU-90.1"], Some("one"), true);

        let (verdict, _) = run_check(p90, std::slice::from_ref(&first), None)?;
        assert_eq!(verdict.check, Check::AcTrace);
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        assert!(verdict.findings[0].message.contains("TU-90.2"));

        // A tag that does not count proves nothing.
        let ignored = tag("src/a.rs", 9, &["TU-90.2"], Some("two"), false);
        let (verdict, _) = run_check(p90, &[first.clone(), ignored], None)?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);

        // One tag may prove several criteria.
        let both = tag("src/a.rs", 3, &["TU-90.1", "TU-90.2"], Some("both"), true);
        let (verdict, _) = run_check(p90, &[both], None)?;
        assert!(verdict.passed(), "{:?}", verdict.findings);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn an_unknown_id_is_a_finding_at_its_tag() -> TestResult {
        let tags = [
            tag("src/a.rs", 3, &["TU-90.1", "TU-90.2"], Some("one"), true),
            tag("src/b.rs", 7, &["TU-99.9"], Some("two"), true),
            // Even a test that does not count must name a criterion that exists.
            tag("src/c.rs", 2, &["TU-98.1"], Some("three"), false),
            // An id of another phase is known; it just proves nothing for this one.
            tag("src/d.rs", 4, &["TU-91.1"], Some("four"), true),
        ];
        for phase in [Some(phase("P90")?), None] {
            let (verdict, _) = run_check(phase, &tags, None)?;
            let places: Vec<(Option<&str>, Option<u32>)> = verdict
                .findings
                .iter()
                .map(|finding| (finding.file.as_deref(), finding.line))
                .collect();
            assert_eq!(
                places,
                [(Some("src/b.rs"), Some(7)), (Some("src/c.rs"), Some(2))]
            );
            assert!(verdict.findings[0].message.contains("TU-99.9"));
            assert!(verdict.findings[1].message.contains("TU-98.1"));
        }
        Ok(())
    }

    #[test]
    fn without_a_phase_only_the_rules_for_tags_apply() -> TestResult {
        let (verdict, table) = run_check(None, &[], None)?;
        assert!(verdict.passed(), "{:?}", verdict.findings);
        assert_eq!(verdict.notes.len(), 1, "{:?}", verdict.notes);
        assert_eq!(table, ["ac-trace: no phase; criteria with a tag: 0"]);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_phase_built_in_another_repository_demands_nothing() -> TestResult {
        let (verdict, table) = run_check(Some(phase("P91")?), &[], None)?;
        assert!(verdict.passed(), "{:?}", verdict.findings);
        assert_eq!(verdict.notes.len(), 1, "{:?}", verdict.notes);
        assert!(
            verdict.notes[0].contains("FPGSchiba/bmad-method"),
            "{:?}",
            verdict.notes
        );
        assert_eq!(table.len(), 1, "{table:?}");
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_phase_spec_narrows_the_criteria_demanded() -> TestResult {
        let p90 = Some(phase("P90")?);
        let file = "docs/specs/phases/P90-x.md";
        let scope = Scope::PhaseSpec {
            file: file.to_owned(),
            acs: vec!["TU-90.1".to_owned()],
        };
        let tags = [tag("src/a.rs", 3, &["TU-90.1"], Some("one"), true)];
        let mut out = Vec::new();
        let verdict = check(
            &umbrella::parse(UMBRELLA)?,
            p90,
            &scope,
            &tags,
            None,
            &mut out,
        );
        assert!(verdict.passed(), "{:?}", verdict.findings);
        let table: Vec<String> = String::from_utf8(out)?.lines().map(str::to_owned).collect();
        assert!(table[0].contains(file), "{table:?}");
        assert_eq!(table.len(), 2, "{table:?}");

        // The umbrella alone would also demand TU-90.2.
        let (verdict, _) = run_check(p90, &tags, None)?;
        assert!(!verdict.passed());
        Ok(())
    }

    #[test]
    fn a_phase_that_is_not_in_the_umbrella_is_a_finding() -> TestResult {
        let (verdict, _) = run_check(Some(phase("P77")?), &[], None)?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        assert!(verdict.findings[0].message.contains("P77"));
        Ok(())
    }

    #[test]
    fn the_table_lists_every_criterion_with_its_tests() -> TestResult {
        let tags = [
            tag("src/a.rs", 3, &["TU-90.1"], Some("one"), true),
            tag(
                "web/a.spec.ts",
                12,
                &["TU-90.1", "TU-90.2"],
                Some("shows a record"),
                true,
            ),
            tag("src/a.rs", 20, &["TU-90.1"], None, true),
            tag("src/a.rs", 30, &["TU-90.2"], Some("later"), false),
        ];
        let (verdict, table) = run_check(Some(phase("P90")?), &tags, None)?;
        assert!(verdict.passed(), "{:?}", verdict.findings);
        assert_eq!(
            table,
            [
                "ac-trace: P90 Fixture phase (theos-loom); criteria: 2",
                "TU-90.1   src/a.rs:3 one",
                "TU-90.1   web/a.spec.ts:12 shows a record",
                "TU-90.1   src/a.rs:20",
                "TU-90.2   web/a.spec.ts:12 shows a record",
                "TU-90.2   src/a.rs:30 later (does not count)",
            ]
        );

        let (_, table) = run_check(Some(phase("P90")?), &tags[..1], None)?;
        assert_eq!(
            table.last().map(String::as_str),
            Some("TU-90.2   no counting test")
        );

        // Without a phase the table is every criterion that has a tag.
        let (_, table) = run_check(None, &tags[..1], None)?;
        assert_eq!(
            table,
            [
                "ac-trace: no phase; criteria with a tag: 1",
                "TU-90.1   src/a.rs:3 one",
            ]
        );
        Ok(())
    }

    #[test]
    fn a_report_adds_the_result_of_each_test() -> TestResult {
        let tags = [
            tag("src/a.rs", 3, &["TU-90.1"], Some("passes"), true),
            tag("src/a.rs", 9, &["TU-90.1"], Some("fails"), true),
            tag("src/a.rs", 12, &["TU-90.2"], Some("in_two_modules"), true),
            tag("src/a.rs", 15, &["TU-90.2"], Some("not_run"), true),
            tag("src/a.rs", 21, &["TU-90.2"], Some("ignored_one"), false),
        ];
        let junit = report(&[
            ("tests::passes", true),
            ("tests::fails", false),
            ("a::in_two_modules", true),
            ("b::in_two_modules", false),
            ("ignored_one", true),
        ]);
        let (_, table) = run_check(Some(phase("P90")?), &tags, Some(&junit))?;
        assert_eq!(
            table,
            [
                "ac-trace: P90 Fixture phase (theos-loom); criteria: 2; results of the last \
                 test run",
                "TU-90.1   pass  src/a.rs:3 passes",
                "TU-90.1   FAIL  src/a.rs:9 fails",
                // It failed in one of the two places, so it did not pass.
                "TU-90.2   FAIL  src/a.rs:12 in_two_modules",
                "TU-90.2   –     src/a.rs:15 not_run",
                "TU-90.2   –     src/a.rs:21 ignored_one (does not count)",
            ]
        );

        // A missing criterion has no result either.
        let (_, table) = run_check(Some(phase("P90")?), &tags[..1], Some(&junit))?;
        assert_eq!(
            table.last().map(String::as_str),
            Some("TU-90.2   –     no counting test")
        );

        // A test of a top-level function in an integration test file has no path.
        let junit = report(&[("passes", true)]);
        let (_, table) = run_check(Some(phase("P90")?), &tags[..1], Some(&junit))?;
        assert_eq!(table[1], "TU-90.1   pass  src/a.rs:3 passes");
        Ok(())
    }

    #[test]
    fn a_test_is_matched_by_the_end_of_its_path_and_not_by_the_end_of_its_name() -> TestResult {
        let junit = report(&[("tests::big_test", true), ("tests::test", false)]);
        let tags = [
            tag("src/a.rs", 3, &["TU-90.1"], Some("test"), true),
            tag("src/a.rs", 9, &["TU-90.2"], Some("big_test"), true),
        ];
        let (_, table) = run_check(Some(phase("P90")?), &tags, Some(&junit))?;
        assert_eq!(table[1], "TU-90.1   FAIL  src/a.rs:3 test");
        assert_eq!(table[2], "TU-90.2   pass  src/a.rs:9 big_test");
        Ok(())
    }

    #[test]
    fn every_line_of_the_table_starts_with_fixed_text() -> TestResult {
        // Paths and titles come from the pull request; the runner reads a line that starts with
        // two colons as a workflow command, and a lone carriage return starts a new line.
        let tags = [
            tag(
                "a\r::error x.ts",
                1,
                &["TU-90.1"],
                Some("t\n::error b\r::warning c"),
                true,
            ),
            tag(
                "src/\u{2028}::error.ts",
                2,
                &["TU-90.2"],
                Some("\u{85}::error"),
                false,
            ),
        ];
        let junit = report(&[("t", true)]);
        for junit in [None, Some(&junit)] {
            let (_, table) = run_check(Some(phase("P90")?), &tags, junit)?;
            assert!(table.len() >= 3, "{table:?}");
            for line in &table {
                assert!(
                    line.starts_with("ac-trace: ") || line.starts_with("TU-90."),
                    "{line:?}"
                );
                assert!(
                    !line.contains(['\r', '\n', '\u{85}', '\u{2028}']),
                    "{line:?}"
                );
            }
        }

        // The umbrella is part of the pull request too, and the name of the phase goes into the
        // heading of the table and into a note. The parser does not let them
        // hold a line break or a control character, so no such line can be written.
        let p90 = "| P90 | Fixture phase | S2 | – |";
        let p91 = "| P91 | Elsewhere | S1 | – |";
        assert!(UMBRELLA.contains(p90) && UMBRELLA.contains(p91));
        for cell in [
            "A\r::error::x",
            "A\u{2028}::error",
            "A\u{85}::error",
            "A\t::error",
        ] {
            for hostile in [
                UMBRELLA.replace(p90, &format!("| P90 | {cell} | S2 | – |")),
                UMBRELLA.replace(p91, &format!("| P91 | {cell} | S1 | – |")),
            ] {
                let result = umbrella::parse(&hostile);
                assert!(
                    matches!(result, Err(Error::Parse { .. })),
                    "{cell:?}: {result:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_table_that_cannot_be_written_is_a_note() -> TestResult {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let verdict = check(
            &umbrella::parse(UMBRELLA)?,
            None,
            &Scope::Umbrella,
            &[],
            None,
            &mut Closed,
        );
        assert!(verdict.passed());
        assert!(
            verdict.notes.iter().any(|note| note.contains("closed")),
            "{:?}",
            verdict.notes
        );
        Ok(())
    }

    #[test]
    fn typescript_files_are_tracked_outside_fixtures_node_modules_and_target() {
        for path in [
            "web/e2e/editor.spec.ts",
            "web/src/lib/store.test.ts",
            "web/src/prototype.spec.ts",
            "crates/loom-core/tests/fixturesX/a.ts",
            "web/src/App.test.tsx",
        ] {
            assert!(is_typescript(path), "{path}");
        }
        for path in [
            "README.md",
            "src/lib.rs",
            "web/src/App.svelte",
            "web/src/main.js",
            "tests/fixtures/a.ts",
            "xtask/tests/fixtures/ac-trace/case/e2e/a.ts",
            "crates/loom-core/tests/fixtures/records/a.ts",
            "target/debug/build/x.ts",
            "crates/a/target/x.ts",
            "web/node_modules/pkg/index.ts",
            "web/node_modules/x/a.tsx",
            "node_modules/pkg/index.ts",
        ] {
            assert!(!is_typescript(path), "{path}");
        }
    }

    #[test]
    fn printable_text_has_no_line_breaks_or_control_characters() {
        assert_eq!(printable("a/b.rs"), "a/b.rs");
        assert_eq!(
            printable("a\r\nb\tc\u{0}d\u{85}e\u{2028}f\u{2029}g"),
            "a??b?c?d?e?f?g"
        );
        assert_eq!(printable("ünï/ć.rs"), "ünï/ć.rs");
    }

    /// A git repository with the fixture umbrella, a file that tags `TU-90.1` and one that tags
    /// `TU-90.2`, on a branch called `branch`.
    fn repository(branch: &str) -> TestResult<tempfile::TempDir> {
        let dir = init_repo()?;
        let path = dir.path();
        std::fs::create_dir_all(path.join("docs").join("specs"))?;
        std::fs::write(path.join(umbrella::PATH), UMBRELLA)?;
        std::fs::create_dir_all(path.join("src"))?;
        std::fs::write(
            path.join("Cargo.toml"),
            "[package]
name = \"fixture\"
version = \"0.0.0\"
edition = \"2024\"

[lib]
path = \"src/lib.rs\"

[workspace]
",
        )?;
        std::fs::write(
            path.join("Cargo.lock"),
            "version = 4

[[package]]
name = \"fixture\"
version = \"0.0.0\"
",
        )?;
        std::fs::write(
            path.join("src").join("lib.rs"),
            "// AC: TU-90.1\n#[test]\nfn proves_the_first() {}\n",
        )?;
        commit(path, "chore: start")?;
        git(path, &["switch", "-q", "-c", branch])?;
        Ok(dir)
    }

    fn pull_request(head_ref: &str) -> CiEvent {
        CiEvent::PullRequest(crate::ci::PullRequestEvent {
            action: "synchronize".to_owned(),
            number: 1,
            title: "feat(xtask): a title (P90)".to_owned(),
            body: None,
            base_sha: "a".repeat(40),
            head_sha: "b".repeat(40),
            head_ref: head_ref.to_owned(),
            label: None,
        })
    }

    // AC: TU-00.4
    #[test]
    fn the_phase_comes_from_the_branch_or_from_the_pull_request() -> TestResult {
        let dir = repository("phase/P90-fixture")?;
        let path = dir.path();
        let verdict = run(path, None, &shared(), &mut Vec::new())?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        assert!(verdict.findings[0].message.contains("TU-90.2"));

        // The head branch of the pull request wins over the one checked out.
        let verdict = run(
            path,
            Some(&pull_request("fix/x")),
            &shared(),
            &mut Vec::new(),
        )?;
        assert!(verdict.passed(), "{:?}", verdict.findings);
        let on_phase = pull_request("phase/P90-fixture");
        assert!(!run(path, Some(&on_phase), &shared(), &mut Vec::new())?.passed());
        Ok(())
    }

    #[test]
    fn a_branch_without_a_phase_checks_only_the_tags() -> TestResult {
        for branch in ["fix/x", "spec/P90-x", "docs/x", "gate/x"] {
            let dir = repository(branch)?;
            let verdict = run(dir.path(), None, &shared(), &mut Vec::new())?;
            assert!(verdict.passed(), "{branch}: {:?}", verdict.findings);
        }
        let dir = repository("fix/x")?;
        std::fs::write(
            dir.path().join("src").join("lib.rs"),
            "mod more;\n// AC: TU-90.1\n#[test]\nfn proves_the_first() {}\n",
        )?;
        std::fs::write(
            dir.path().join("src").join("more.rs"),
            "// AC: TU-99.9\n#[test]\nfn unknown() {}\n",
        )?;
        let verdict = run(dir.path(), None, &shared(), &mut Vec::new())?;
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        Ok(())
    }

    #[test]
    fn a_detached_head_outside_a_pull_request_is_an_error() -> TestResult {
        let dir = repository("fix/x")?;
        git(dir.path(), &["switch", "-q", "--detach", "HEAD"])?;
        let result = run(dir.path(), None, &shared(), &mut Vec::new());
        assert!(
            matches!(&result, Err(Error::Missing { what }) if what.contains("--phase")),
            "{result:?}"
        );
        // The pull request says which branch it is from.
        assert!(
            run(
                dir.path(),
                Some(&pull_request("fix/x")),
                &shared(),
                &mut Vec::new()
            )?
            .passed()
        );
        Ok(())
    }

    #[test]
    fn the_report_of_the_profile_is_read_when_there_is() -> TestResult {
        let dir = repository("phase/P90-fixture")?;
        let path = dir.path();
        let first_row = |out: Vec<u8>| -> TestResult<String> {
            let text = String::from_utf8(out)?;
            Ok(text.lines().nth(1).unwrap_or_default().to_owned())
        };

        let mut out = Vec::new();
        run(path, None, &shared(), &mut out)?;
        assert_eq!(first_row(out)?, "TU-90.1   src/lib.rs:1 proves_the_first");

        for (profile, ci) in [("default", false), ("ci", true)] {
            let folder = path.join("target").join("nextest").join(profile);
            std::fs::create_dir_all(&folder)?;
            std::fs::write(
                folder.join("junit.xml"),
                "<testsuites><testsuite name=\"x\"><testcase name=\"lib::proves_the_first\"/></testsuite></testsuites>",
            )?;
            let mut out = Vec::new();
            run_phase(path, Some(phase("P90")?), ci, &shared(), &mut out)?;
            assert_eq!(
                first_row(out)?,
                "TU-90.1   pass  src/lib.rs:1 proves_the_first",
                "{profile}"
            );
        }

        // A report that is not one is an error, not an empty column.
        std::fs::write(
            path.join("target")
                .join("nextest")
                .join("default")
                .join("junit.xml"),
            "nope",
        )?;
        let result = run(path, None, &shared(), &mut Vec::new());
        assert!(matches!(result, Err(Error::Parse { .. })), "{result:?}");
        Ok(())
    }
}
