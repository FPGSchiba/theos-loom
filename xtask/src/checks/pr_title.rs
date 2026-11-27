//! The `pr-title` check: the title of a pull request is a Conventional Commit that ends with the
//! phase, `gate` or `docs`, matching the branch (`CONTRIBUTING.md` §1 and §3, `docs/gate.md` §1).
//!
//! The title becomes the squash commit on `main`, so it is held to the rules of
//! [`header_problems`] as well: the limit of 72 characters counts the whole summary, suffix
//! included (and ` (#N)` too, when the number of the pull request is known), and the rules for
//! the words (no capital first, no full stop last) also apply to the words before the suffix. The
//! core is [`check_title`]: a title and a branch in, a verdict out.
//! [`run`] takes both from the pull request event.

use std::fmt;
use std::io::Write;
use std::path::Path;

use crate::branch::{BranchKind, Phase, parse_branch};
use crate::ci::CiEvent;
use crate::conventional::{header_problems, parse_header, squash_problem, wording_problems};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// Checks the pull request title `title`, of a pull request from `branch`, against the rules of
/// `CONTRIBUTING.md` §3.
///
/// A title on `release-plz-*` is a release title, `chore(release): vX.Y.Z` or
/// `chore(release): vX.Y.Z-rc.N`, and no other title is. Every other title is a Conventional Commit
/// whose summary ends with ` (PNN)`, ` (gate)` or ` (docs)`, as the branch says: `phase/PNN-…` and
/// `spec/PNN-…` end with ` (PNN)`, `gate/…` with ` (gate)`, `docs/…` with ` (docs)` and `fix/…`
/// with any of the three. A `spec/` title is also `docs(spec): …`. A branch that is `main` or has
/// any other name fails, because there is nothing to match the title with.
///
/// `number` is the number of the pull request, when it is known: GitHub appends ` (#N)` to the
/// summary of the squash commit, so the limit of the summary is held to the summary and that
/// suffix. A local run has no number and judges the summary alone.
#[must_use]
pub fn check_title(title: &str, branch: &BranchKind, number: Option<u64>) -> Verdict {
    let findings = problems(title, branch, number)
        .into_iter()
        .map(Finding::new)
        .collect();
    Verdict::with(Check::PrTitle, findings)
}

/// Runs the check on the title and the head branch of the pull request event.
///
/// # Errors
///
/// Fails when there is no pull request event: a title only exists on a pull request, so a local
/// run has to pass `--title`.
pub fn run(_root: &Path, ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    match ci {
        Some(CiEvent::PullRequest(pull_request)) => Ok(check_title(
            &pull_request.title,
            &parse_branch(&pull_request.head_ref),
            Some(pull_request.number),
        )),
        // The title stays out of the error: errors are printed as they are, and the runner reads
        // workflow commands in what it is given. Findings are marked line by line instead.
        Some(CiEvent::Push(_) | CiEvent::Other) | None => Err(Error::Missing {
            what: "the pull request title: there is no pull request event, so pass `--title` \
                   (and `--branch`)"
                .to_owned(),
        }),
    }
}

/// What a branch asks of a title that is not a release title.
struct Rule {
    /// The number of the pull request, if it is known.
    number: Option<u64>,
    /// The branch as a finding names it, such as ``a `phase/P02-…` branch``.
    branch: String,
    /// What the summary must end with; `None` for any of the three.
    suffix: Option<Suffix>,
    /// Whether the title must also be `docs(spec): …`.
    spec: bool,
}

/// How a summary ends: a phase, `gate` or `docs`, in parentheses after a space.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Suffix {
    Phase(Phase),
    Gate,
    Docs,
}

impl fmt::Display for Suffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Phase(phase) => write!(f, "({phase})"),
            Self::Gate => f.write_str("(gate)"),
            Self::Docs => f.write_str("(docs)"),
        }
    }
}

/// Everything wrong with `title` on `branch`, one sentence for each problem.
fn problems(title: &str, branch: &BranchKind, number: Option<u64>) -> Vec<String> {
    let release = is_release_title(title);
    let titled_on = |name: String, suffix: Option<Suffix>, spec: bool| {
        titled(
            title,
            &Rule {
                number,
                branch: name,
                suffix,
                spec,
            },
        )
    };
    match branch {
        BranchKind::Release if release => Vec::new(),
        BranchKind::Release => vec![
            "a pull request from a `release-plz-*` branch has a release title, \
             `chore(release): vX.Y.Z` or `chore(release): vX.Y.Z-rc.N`"
                .to_owned(),
        ],
        BranchKind::Main => vec![unknown_branch("main")],
        BranchKind::Other(name) => vec![unknown_branch(name)],
        _ if release => vec![
            "a release title is only for the pull requests of the release pipeline, from \
             `release-plz-*` branches; end this title with ` (PNN)`, ` (gate)` or ` (docs)` \
             (CONTRIBUTING.md §3)"
                .to_owned(),
        ],
        BranchKind::Phase(phase) => titled_on(
            format!("a `phase/{phase}-…` branch"),
            Some(Suffix::Phase(*phase)),
            false,
        ),
        BranchKind::Spec(phase) => titled_on(
            format!("a `spec/{phase}-…` branch"),
            Some(Suffix::Phase(*phase)),
            true,
        ),
        BranchKind::Gate => titled_on("a `gate/…` branch".to_owned(), Some(Suffix::Gate), false),
        BranchKind::Docs => titled_on("a `docs/…` branch".to_owned(), Some(Suffix::Docs), false),
        BranchKind::Fix => titled_on("a `fix/…` branch".to_owned(), None, false),
    }
}

/// The finding for the branch `name`, which has no rule.
fn unknown_branch(name: &str) -> String {
    format!(
        "the branch `{name}` is not named as CONTRIBUTING.md §1 requires (`spec/PNN-<slug>`, \
         `phase/PNN-<slug>`, `gate/<slug>`, `docs/<slug>`, `fix/<slug>` or `release-plz-*`), so \
         the title cannot be matched to it; rename the branch"
    )
}

/// The problems of a title that is not a release title, on a branch with a rule.
fn titled(title: &str, rule: &Rule) -> Vec<String> {
    let header = match parse_header(title) {
        Ok(header) => header,
        Err(problem) => return vec![problem],
    };
    let mut problems = header_problems(&header);
    problems.extend(
        rule.number
            .and_then(|number| squash_problem(&header, number)),
    );
    // The suffix ends the summary, so `header_problems` cannot see a full stop before it, as in
    // `add thing. (P02)`. The rules for the words apply to the summary without the suffix too;
    // what the whole summary has already is not said twice. The limit of 72 characters stays
    // with the whole summary, and with the squash subject when the number is known.
    if let Some((words, _)) = split_suffix(&header.summary) {
        for problem in wording_problems(words) {
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }
    }
    if rule.spec && !(header.kind == "docs" && header.scope.as_deref() == Some("spec")) {
        problems.push(format!(
            "a title on {} starts with `docs(spec):`",
            rule.branch
        ));
    }
    problems.extend(suffix_problem(&header.summary, rule));
    problems
}

fn suffix_problem(summary: &str, rule: &Rule) -> Option<String> {
    let branch = &rule.branch;
    match (split_suffix(summary).map(|(_, suffix)| suffix), rule.suffix) {
        (Some(_), None) => None,
        (Some(found), Some(wanted)) if found == wanted => None,
        (Some(found), Some(wanted)) => Some(format!(
            "the title ends with ` {found}`, but a title on {branch} ends with ` {wanted}`"
        )),
        (None, Some(wanted)) => Some(format!(
            "the summary must end with ` {wanted}`, as a title on {branch} does \
             (CONTRIBUTING.md §3)"
        )),
        (None, None) => Some(format!(
            "the summary must end with ` (PNN)`, ` (gate)` or ` (docs)`, as a title on {branch} \
             does (CONTRIBUTING.md §3)"
        )),
    }
}

/// The words of `summary` before its suffix, and the suffix: the word in parentheses that ends
/// it after a space, if it is one of the three. The space is not part of the words.
fn split_suffix(summary: &str) -> Option<(&str, Suffix)> {
    let (words, word) = summary.strip_suffix(')')?.rsplit_once(" (")?;
    let suffix = match word {
        "gate" => Suffix::Gate,
        "docs" => Suffix::Docs,
        _ => Suffix::Phase(word.parse().ok()?),
    };
    Some((words, suffix))
}

/// `chore(release): vX.Y.Z` or `chore(release): vX.Y.Z-rc.N`, with numbers of ASCII digits.
fn is_release_title(title: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let Some(version) = title.strip_prefix("chore(release): v") else {
        return false;
    };
    let (numbers, candidate) = match version.split_once("-rc.") {
        Some((numbers, candidate)) => (numbers, Some(candidate)),
        None => (version, None),
    };
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| digits(part)) && candidate.is_none_or(digits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::parse_branch;
    use crate::ci::{PullRequestEvent, PushEvent};

    fn check(title: &str, branch: &str) -> Verdict {
        check_title(title, &parse_branch(branch), None)
    }

    fn check_numbered(title: &str, branch: &str, number: u64) -> Verdict {
        check_title(title, &parse_branch(branch), Some(number))
    }

    /// A phase title whose summary has exactly `length` characters.
    fn title_with_summary(length: usize) -> String {
        let suffix = " (P02)";
        format!(
            "feat(content): {}{suffix}",
            "a".repeat(length - suffix.len())
        )
    }

    /// The findings of `check`, one per line.
    fn messages(verdict: &Verdict) -> String {
        verdict
            .findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Asserts that the title fails and that a finding says `needle`.
    fn assert_fails(title: &str, branch: &str, needle: &str) {
        let verdict = check(title, branch);
        assert_eq!(verdict.check, Check::PrTitle);
        assert!(
            messages(&verdict).contains(needle),
            "{title:?} on {branch}: {:?}",
            verdict.findings
        );
    }

    fn assert_passes(title: &str, branch: &str) {
        let verdict = check(title, branch);
        assert_eq!(verdict.check, Check::PrTitle);
        assert!(
            verdict.passed(),
            "{title:?} on {branch}: {:?}",
            verdict.findings
        );
    }

    fn pull_request(title: &str, head_ref: &str) -> CiEvent {
        CiEvent::PullRequest(PullRequestEvent {
            action: "edited".to_owned(),
            number: 1,
            title: title.to_owned(),
            body: None,
            base_sha: "a".repeat(40),
            head_sha: "b".repeat(40),
            head_ref: head_ref.to_owned(),
            label: None,
        })
    }

    fn run_with(ci: Option<&CiEvent>) -> Result<Verdict> {
        run(Path::new("."), ci, &mut Vec::new())
    }

    // AC: CE-08.9
    #[test]
    fn phase_title_passes() {
        assert_passes(
            "feat(content): records and grammars (P02)",
            "phase/P02-records-and-grammars",
        );
        // Any type and scope of CONTRIBUTING.md §2, and no scope at all.
        assert_passes("fix(xtask)!: rename the flag (P00)", "phase/P00-x");
        assert_passes("chore: tidy up (P11)", "phase/P11-x");
    }

    // AC: CE-08.9
    #[test]
    fn spec_title_passes() {
        assert_passes(
            "docs(spec): foundation and gate (P00)",
            "spec/P00-foundation-and-gate",
        );
    }

    // AC: CE-08.9
    #[test]
    fn spec_title_needs_docs_spec() {
        assert_fails("feat(spec): x (P00)", "spec/P00-x", "docs(spec)");
        assert_fails("docs(content): x (P00)", "spec/P00-x", "docs(spec)");
        assert_fails("docs: x (P00)", "spec/P00-x", "docs(spec)");
        assert_fails("feat(content): x (P00)", "spec/P00-x", "docs(spec)");
        // The suffix is still the phase of the branch.
        assert_fails("docs(spec): x (P01)", "spec/P00-x", "(P00)");
        assert_fails("docs(spec): x (gate)", "spec/P00-x", "(P00)");
        // Only a spec branch requires `docs(spec)`; the title of another branch may have it.
        assert_passes("docs(spec): x (docs)", "docs/x");
    }

    // AC: CE-08.9
    #[test]
    fn gate_and_docs_titles_pass() {
        assert_passes("ci(gate): x (gate)", "gate/x");
        assert_passes("docs: x (docs)", "docs/x");
        // The suffix names the kind of branch, not the type of the commit.
        assert_fails("ci(gate): x (docs)", "gate/x", "(gate)");
        assert_fails("ci(gate): x (P02)", "gate/x", "(gate)");
        assert_fails("docs: x (gate)", "docs/x", "(docs)");
        assert_fails("docs: x (P02)", "docs/x", "(docs)");
    }

    // AC: CE-08.9
    #[test]
    fn fix_titles_take_any_of_the_three_suffixes() {
        for suffix in ["(P02)", "(P00)", "(gate)", "(docs)"] {
            assert_passes(&format!("fix(content): x {suffix}"), "fix/x");
        }
        assert_fails("fix(content): x", "fix/x", "(PNN)");
        assert_fails("fix(content): x (P2)", "fix/x", "(PNN)");
        assert_fails("fix(content): x (nope)", "fix/x", "(PNN)");
    }

    // AC: CE-08.9
    #[test]
    fn release_titles_pass() {
        for title in [
            "chore(release): v0.1.0",
            "chore(release): v0.2.0-rc.1",
            "chore(release): v10.20.30",
            "chore(release): v1.0.0-rc.12",
        ] {
            assert_passes(title, "release-plz-2026-11-01");
        }
    }

    // AC: CE-08.9
    #[test]
    fn a_release_branch_needs_a_release_title() {
        assert_fails(
            "feat(content): records (P02)",
            "release-plz-2026-11-01",
            "release title",
        );
        for title in [
            "chore(release): v1.0",
            "chore(release): v1.0.0.0",
            "chore(release): v1.0.0-rc",
            "chore(release): v1.0.0-rc.",
            "chore(release): v1.0.0-rc.x",
            "chore(release): v1.0.0-rc.1.2",
            "chore(release): v1.0.0-beta.1",
            "chore(release): v1..0",
            "chore(release): 1.0.0",
            "chore(release): V1.0.0",
            "chore(release): v1.0.0 ",
            "chore(release): v1.0.0 (gate)",
            "chore(release):v1.0.0",
            "chore(release)!: v1.0.0",
            "chore: v1.0.0",
            "chore(release): v\u{661}.0.0",
            "chore(release): v1.0.0\n",
            "",
        ] {
            assert_fails(title, "release-plz-2026-11-01", "release title");
        }
    }

    // AC: CE-08.9
    #[test]
    fn a_release_title_is_only_for_a_release_branch() {
        for branch in [
            "phase/P02-x",
            "spec/P02-x",
            "gate/x",
            "docs/x",
            "fix/x",
            "main",
            "feature/x",
        ] {
            assert_fails("chore(release): v0.1.0", branch, "release-plz-");
        }
    }

    // AC: CE-08.9
    #[test]
    fn not_conventional_fails() {
        assert_fails(
            "Records and grammars (P02)",
            "phase/P02-x",
            "type(scope)!: summary",
        );
        for title in [
            "",
            "feat",
            "feat:x (P02)",
            "feat:  x (P02)",
            "feat(): x (P02)",
        ] {
            assert_fails(title, "phase/P02-x", "type(scope)!: summary");
        }
    }

    // AC: CE-08.9
    #[test]
    fn the_rules_for_a_commit_header_apply_to_the_title() {
        assert_fails("feature(content): x (P02)", "phase/P02-x", "`feature`");
        assert_fails("feat(nope): x (P02)", "phase/P02-x", "`nope`");
        assert_fails("feat(content): Records (P02)", "phase/P02-x", "upper-case");
        let long = format!("feat(content): {} (P02)", "a".repeat(70));
        assert_fails(&long, "phase/P02-x", "the limit is 72");
        // Everything wrong is reported at once: the scope, the capital and the missing suffix.
        let verdict = check("feat(nope): Records", "phase/P02-x");
        assert_eq!(verdict.findings.len(), 3, "{:?}", verdict.findings);
    }

    // AC: CE-08.9
    #[test]
    fn the_squash_subject_with_the_pull_request_number_is_held_to_the_limit() {
        // GitHub appends ` (#N)` to the squash subject, and `commit-msg` judges it on `main`.
        let seventy = title_with_summary(70);
        let verdict = check_numbered(&seventy, "phase/P02-x", 7);
        assert!(!verdict.passed(), "{:?}", verdict.findings);
        let said = messages(&verdict);
        assert!(said.contains("squash") && said.contains("75"), "{said}");
        assert!(said.contains("shorten"), "{said}");
        let verdict = check_numbered(&title_with_summary(66), "phase/P02-x", 7);
        assert!(verdict.passed(), "{:?}", verdict.findings);
        // 67 + 5 = 72 is still within the limit; 68 + 5 = 73 is not.
        assert!(check_numbered(&title_with_summary(67), "phase/P02-x", 7).passed());
        assert!(!check_numbered(&title_with_summary(68), "phase/P02-x", 7).passed());
        // A local run has no number, so the rule stays as it was.
        assert_passes(&seventy, "phase/P02-x");
    }

    // AC: CE-08.9
    #[test]
    fn a_pull_request_number_with_more_digits_counts_its_length() {
        // ` (#1234)` is 8 characters: 64 + 8 = 72 passes, 65 + 8 = 73 fails.
        assert!(check_numbered(&title_with_summary(64), "phase/P02-x", 1234).passed());
        assert!(!check_numbered(&title_with_summary(65), "phase/P02-x", 1234).passed());
    }

    // AC: CE-08.9
    #[test]
    fn a_title_over_the_limit_is_said_once() {
        let verdict = check_numbered(&title_with_summary(80), "phase/P02-x", 7);
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
    }

    // AC: CE-08.9
    #[test]
    fn a_full_stop_before_the_suffix_fails() {
        // The suffix ends the summary, so the full stop is hidden from a check of its last
        // character, and the title becomes the squash commit with it.
        for (title, branch) in [
            ("feat(content): add thing. (P02)", "phase/P02-x"),
            ("feat(content): x. (P02)", "phase/P02-x"),
            ("ci(gate): add thing. (gate)", "gate/x"),
            ("docs: add thing. (docs)", "docs/x"),
            ("docs(spec): add thing. (P02)", "spec/P02-x"),
            ("fix(content): add thing. (gate)", "fix/x"),
            ("feat(content): add thing.  (P02)", "phase/P02-x"),
        ] {
            assert_fails(title, branch, "full stop");
        }
        // Only that is wrong with it, and it is said once.
        let verdict = check("feat(content): add thing. (P02)", "phase/P02-x");
        assert_eq!(verdict.findings.len(), 1, "{:?}", verdict.findings);
        let verdict = check("feat(content): add thing.", "phase/P02-x");
        let stops = messages(&verdict).matches("full stop").count();
        assert_eq!(stops, 1, "{:?}", verdict.findings);
        // The other rules for words are not said twice either.
        let verdict = check("feat(content): Add thing. (P02)", "phase/P02-x");
        assert_eq!(verdict.findings.len(), 2, "{:?}", verdict.findings);

        // A full stop inside the summary, or none, is fine.
        assert_passes("feat(content): add thing (P02)", "phase/P02-x");
        assert_passes("feat(content): add v1.2 and more (P02)", "phase/P02-x");
        assert_passes("feat(content): add thing.rs (P02)", "phase/P02-x");
    }

    // AC: CE-08.9
    #[test]
    fn missing_suffix_fails() {
        assert_fails("feat(content): records", "phase/P02-x", "(P02)");
        assert_fails("feat(content): records", "gate/x", "(gate)");
        assert_fails("feat(content): records", "docs/x", "(docs)");
        assert_fails("feat(content): records", "spec/P02-x", "docs(spec)");
        assert_fails("docs(spec): records", "spec/P02-x", "(P02)");
        // The suffix is the last thing in the summary, after a space, as one word in parentheses.
        for title in [
            "feat(content): records (P02) and more",
            "feat(content): records(P02)",
            "feat(content): (P02)",
            "feat(content): records (P02",
            "feat(content): records (P02))",
            "feat(content): records (p02)",
            "feat(content): records (P2)",
            "feat(content): records (P002)",
            "feat(content): records (GATE)",
            "feat(content): records [P02]",
            "feat(content): records P02",
            "feat(content): records (P02) ",
        ] {
            assert_fails(title, "phase/P02-x", "(P02)");
        }
    }

    // AC: CE-08.9
    #[test]
    fn suffix_must_match_branch() {
        assert_fails("feat(content): x (P03)", "phase/P02-x", "(P02)");
        assert_fails("feat(content): x (gate)", "phase/P02-x", "(P02)");
        assert_fails("feat(content): x (docs)", "phase/P02-x", "(P02)");
        // The finding says what was found as well as what is needed.
        assert_fails("feat(content): x (P03)", "phase/P02-x", "(P03)");
    }

    // AC: CE-08.9
    #[test]
    fn unknown_branch_fails() {
        assert_fails("feat(content): x (P02)", "feature/x", "CONTRIBUTING.md §1");
        assert_fails("feat(content): x (P02)", "main", "CONTRIBUTING.md §1");
        for branch in ["phase/P2-x", "phase/P02", "Phase/P02-x", "", "origin/main"] {
            assert_fails("feat(content): x (P02)", branch, "CONTRIBUTING.md §1");
        }
        // The finding names the branch, so it is clear which one was read.
        assert_fails("feat(content): x (P02)", "feature/x", "`feature/x`");
    }

    // AC: CE-08.9
    #[test]
    fn a_pull_request_event_gives_the_title_and_the_branch() -> Result<()> {
        let good = pull_request("feat(content): records (P02)", "phase/P02-x");
        let verdict = run_with(Some(&good))?;
        assert_eq!(verdict.check, Check::PrTitle);
        assert!(verdict.passed(), "{:?}", verdict.findings);

        // The branch is the head of the pull request, not the branch checked out here.
        let wrong_phase = pull_request("feat(content): records (P02)", "phase/P03-x");
        assert!(!run_with(Some(&wrong_phase))?.passed());
        let release = pull_request("chore(release): v0.1.0", "release-plz-2026-11-01");
        assert!(run_with(Some(&release))?.passed());
        let bad_title = pull_request("records", "fix/x");
        assert!(!run_with(Some(&bad_title))?.passed());
        Ok(())
    }

    // AC: CE-08.9
    #[test]
    fn without_a_pull_request_there_is_no_title_to_check() {
        let push = CiEvent::Push(PushEvent {
            before: "a".repeat(40),
            after: "b".repeat(40),
        });
        for ci in [None, Some(&push), Some(&CiEvent::Other)] {
            let result = run_with(ci);
            assert!(
                matches!(&result, Err(Error::Missing { what }) if what.contains("--title")),
                "{result:?}"
            );
        }
    }
}
