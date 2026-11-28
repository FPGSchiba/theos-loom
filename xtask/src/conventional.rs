//! Conventional Commit messages, as `CONTRIBUTING.md` §2 defines them.
//!
//! The parser is pure text in, values out, so the `commit-msg` check (every commit of a pull
//! request) and the `pr-title` check (the title, which becomes the squash commit) share it.
//! Parsing and judging are separate: [`parse_header`] and [`parse_message`] accept any type and
//! scope word and only fail on the shape; [`header_problems`] and [`message_problems`] list
//! everything the rules forbid, one sentence per problem.

use std::collections::BTreeSet;

use crate::branch::{BranchKind, Phase, phase_of};

/// The commit types of `CONTRIBUTING.md` §2.
pub const TYPES: [&str; 10] = [
    "feat", "fix", "perf", "refactor", "test", "docs", "build", "ci", "chore", "revert",
];

/// The commit scopes of `CONTRIBUTING.md` §2.
pub const SCOPES: [&str; 13] = [
    "core", "plan", "store", "run", "gate", "context", "api", "cli", "web", "xtask", "spec",
    "docs", "ci",
];

/// The first line of a commit: `type(scope)!: summary`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header {
    /// The type word, as written; [`header_problems`] says whether it is one of [`TYPES`].
    pub kind: String,
    /// The word in parentheses, as written, if there are any.
    pub scope: Option<String>,
    /// Whether there is a `!` before the colon.
    pub breaking: bool,
    /// What follows the colon and its one space.
    pub summary: String,
}

/// Reads `type(scope)!: summary` with `git-conventional`. The scope and the `!` are optional.
/// Our rule on top of the crate: exactly one space follows the colon, as `CONTRIBUTING.md` §2
/// writes the header; the crate accepts any number of spaces, or none.
///
/// Any type and scope the crate accepts are accepted here; whether they are allowed is for
/// [`header_problems`].
///
/// # Errors
///
/// Fails, with a sentence saying what is wrong, when the line does not have that shape.
pub fn parse_header(line: &str) -> std::result::Result<Header, String> {
    let not_a_header = |why: &str| {
        let hint = hint_for(line);
        format!("the header is not `type(scope)!: summary`: {why}{hint}")
    };
    if line.contains(['\n', '\r']) {
        return Err(not_a_header("the header is one line"));
    }
    let commit =
        git_conventional::Commit::parse(line).map_err(|error| not_a_header(&error.to_string()))?;
    let summary = commit.description();
    let before_summary = &line[..line.len() - summary.len()];
    if !before_summary.ends_with(": ") || before_summary.ends_with("  ") {
        return Err(not_a_header("exactly one space must follow the colon"));
    }
    Ok(header_of(&commit, line))
}

/// The header fields of a parsed commit; `!` is read from the text before the first `: ` of
/// `first_line`, because the crate's own flag also counts the `BREAKING CHANGE` footer.
fn header_of(commit: &git_conventional::Commit<'_>, first_line: &str) -> Header {
    let before_colon = first_line.split_once(": ").map_or("", |(before, _)| before);
    Header {
        kind: commit.type_().to_string(),
        scope: commit.scope().map(|scope| scope.to_string()),
        breaking: before_colon.ends_with('!'),
        summary: commit.description().to_owned(),
    }
}

/// What to add, beyond the shape, for the headers git and its tools write by themselves.
fn hint_for(line: &str) -> &'static str {
    if ["fixup! ", "squash! ", "amend! "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    {
        " (autosquash commits, `fixup!` and `squash!`, must be squashed into the commit they fix)"
    } else if line.starts_with("Revert \"") {
        " (write a revert as `revert(scope): summary`, not with git's own `Revert \"…\"` line)"
    } else {
        ""
    }
}

/// Everything wrong with a header that parsed: a type or scope that is not in [`TYPES`] or
/// [`SCOPES`], and a summary of more than 72 characters (counted as `char`s), that starts with
/// an upper-case letter or that ends with a full stop.
#[must_use]
pub fn header_problems(header: &Header) -> Vec<String> {
    let mut problems = Vec::new();
    if !TYPES.contains(&header.kind.as_str()) {
        problems.push(format!(
            "the type `{}` is not allowed; use one of {}",
            header.kind,
            TYPES.join(", ")
        ));
    }
    if let Some(scope) = &header.scope
        && !SCOPES.contains(&scope.as_str())
    {
        problems.push(format!(
            "the scope `{scope}` is not allowed; use one of {}, or leave the scope out",
            SCOPES.join(", ")
        ));
    }
    let length = header.summary.chars().count();
    if length > MAX_SUMMARY {
        problems.push(format!(
            "the summary has {length} characters; the limit is {MAX_SUMMARY}"
        ));
    }
    problems.extend(wording_problems(&header.summary));
    problems
}

/// The rules for the words of a summary, which [`header_problems`] applies to the whole summary:
/// it does not start with an upper-case letter and does not end with a full stop.
///
/// `pr-title` applies them again to the words before the phase it ends with, which a check of
/// the last character cannot see.
#[must_use]
pub fn wording_problems(summary: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if summary.starts_with(char::is_uppercase) {
        problems.push("the summary must not start with an upper-case letter".to_owned());
    }
    if summary.trim_end().ends_with('.') {
        problems.push("the summary must not end with a full stop".to_owned());
    }
    problems
}

/// The problem of a summary that fits the limit by itself but not with ` (#N)`, which GitHub
/// appends to the subject of a squash commit (`squash_merge_commit_title = PR_TITLE`), where
/// `commit-msg` judges it on `main`. A summary that is over the limit already has the finding of
/// [`header_problems`].
#[must_use]
pub fn squash_problem(header: &Header, number: u64) -> Option<String> {
    let length = header.summary.chars().count();
    let squashed = length + format!(" (#{number})").chars().count();
    (length <= MAX_SUMMARY && squashed > MAX_SUMMARY).then(|| {
        format!(
            "the squash commit on `main` gets ` (#{number})` after the summary, which then has \
             {squashed} characters; the limit is {MAX_SUMMARY}: shorten the title"
        )
    })
}

/// The longest summary, in characters.
const MAX_SUMMARY: usize = 72;

/// The longest body or footer line, in characters, unless it has no space.
const MAX_LINE: usize = 100;

/// A commit message: the header, the body lines and the footers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    /// The first line.
    pub header: Header,
    /// The lines between the header and the footers, with a blank line between paragraphs.
    pub body: Vec<String>,
    /// The footers as `(key, value)`; a value that continues on further lines has them joined
    /// with `\n`.
    pub footers: Vec<(String, String)>,
    /// Every line after the header, as written (the crate drops the separator spacing of a
    /// footer, so the length rule reads the lines from here).
    pub lines: Vec<String>,
}

/// Reads a whole commit message with `git-conventional`.
///
/// CRLF is read as LF. The crate decides what is the body and what are the footers, and that a
/// blank line follows the header when anything follows it. The header gets the rule of
/// [`parse_header`].
///
/// # Errors
///
/// Fails, with a sentence saying what is wrong, when the header does not parse or the blank line
/// after it is missing.
pub fn parse_message(text: &str) -> std::result::Result<Message, String> {
    let text = text.replace("\r\n", "\n");
    let first_line = text.split('\n').next().unwrap_or_default();
    let header = parse_header(first_line)?;
    let commit = git_conventional::Commit::parse(&text)
        .map_err(|_| "a blank line must separate the header from the body".to_owned())?;
    Ok(Message {
        header,
        lines: text.split('\n').skip(1).map(str::to_owned).collect(),
        body: commit
            .body()
            .map(|body| body.split('\n').map(str::to_owned).collect())
            .unwrap_or_default(),
        footers: commit
            .footers()
            .iter()
            .map(|footer| (footer.token().to_string(), footer.value().to_owned()))
            .collect(),
    })
}

/// Everything wrong with a parsed message beyond its header ([`header_problems`]):
///
/// - a body or footer line of more than 100 characters, unless it has no space (a URL);
/// - `!` without a `BREAKING CHANGE` or `BREAKING-CHANGE` footer, or the footer without `!`;
/// - a `Phase:` footer that is not a phase, or that is not the phase of a `phase/` branch, or
///   that is missing on a `phase/` branch;
/// - a `Refs:` id that is not in `known_acs`. With no known ids at all the list is unavailable,
///   and an id only has to look like one (`TU-00.10`, `TU-01.1`).
#[must_use]
pub fn message_problems(
    message: &Message,
    branch: &BranchKind,
    known_acs: &BTreeSet<String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    line_length_problems(message, &mut problems);
    breaking_problems(message, &mut problems);
    phase_problems(message, branch, &mut problems);
    refs_problems(message, known_acs, &mut problems);
    problems
}

fn line_length_problems(message: &Message, problems: &mut Vec<String>) {
    for line in &message.lines {
        let length = line.chars().count();
        if length > MAX_LINE && line.contains(' ') {
            let start: String = line.chars().take(30).collect();
            problems.push(format!(
                "the line starting \"{start}\" has {length} characters; the limit for body and \
                 footer lines is {MAX_LINE} (a line without spaces, such as a URL, may be longer)"
            ));
        }
    }
}

fn breaking_problems(message: &Message, problems: &mut Vec<String>) {
    let footer = message
        .footers
        .iter()
        .any(|(key, _)| key == "BREAKING CHANGE" || key == "BREAKING-CHANGE");
    match (message.header.breaking, footer) {
        (true, false) => problems.push(
            "the header has a `!` but there is no `BREAKING CHANGE:` footer to say what breaks"
                .to_owned(),
        ),
        (false, true) => problems.push(
            "there is a `BREAKING CHANGE:` footer but the header has no `!` after the type or \
             scope"
                .to_owned(),
        ),
        _ => {}
    }
}

fn phase_problems(message: &Message, branch: &BranchKind, problems: &mut Vec<String>) {
    let wanted = phase_of(branch);
    let mut found = false;
    for (_, value) in message.footers.iter().filter(|(key, _)| key == "Phase") {
        found = true;
        let value = value.trim();
        match (value.parse::<Phase>(), wanted) {
            (Err(_), _) => problems.push(format!(
                "`Phase: {value}` is not a phase; write `P` and two digits, such as `Phase: P02`"
            )),
            (Ok(phase), Some(wanted)) if phase != wanted => problems.push(format!(
                "`Phase: {phase}` is not the phase of this branch; the footer must be \
                 `Phase: {wanted}`"
            )),
            _ => {}
        }
    }
    if let Some(wanted) = wanted
        && !found
    {
        problems.push(format!(
            "commits on a phase branch need the footer `Phase: {wanted}`"
        ));
    }
}

fn refs_problems(message: &Message, known_acs: &BTreeSet<String>, problems: &mut Vec<String>) {
    for (_, value) in message.footers.iter().filter(|(key, _)| key == "Refs") {
        for id in value.split(',').map(str::trim) {
            let known = if known_acs.is_empty() {
                crate::umbrella::is_ac_id(id)
            } else {
                known_acs.contains(id)
            };
            if id.is_empty() {
                problems.push(
                    "`Refs:` has an empty entry; separate acceptance criteria with `, `".to_owned(),
                );
            } else if !known {
                problems.push(format!(
                    "`Refs:` names `{id}`, which is not an acceptance criterion of the umbrella \
                     spec"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::parse_branch;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn header(kind: &str, scope: Option<&str>, breaking: bool, summary: &str) -> Header {
        Header {
            kind: kind.to_owned(),
            scope: scope.map(str::to_owned),
            breaking,
            summary: summary.to_owned(),
        }
    }

    fn known(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    /// The problems of `text` on `branch`, from both functions that judge a message.
    fn problems(text: &str, branch: &str, known_acs: &[&str]) -> Result<Vec<String>, String> {
        let message = parse_message(text)?;
        let mut all = header_problems(&message.header);
        all.extend(message_problems(
            &message,
            &parse_branch(branch),
            &known(known_acs),
        ));
        Ok(all)
    }

    fn has_problem(found: &[String], needle: &str) -> bool {
        found.iter().any(|problem| problem.contains(needle))
    }

    // AC: TU-00.4
    #[test]
    fn parses_header_with_scope_and_bang() -> TestResult {
        assert_eq!(
            parse_header("feat(xtask)!: drop the old flag")?,
            header("feat", Some("xtask"), true, "drop the old flag")
        );
        assert_eq!(
            parse_header("fix(gate): keep the colon: it is part of the summary")?,
            header(
                "fix",
                Some("gate"),
                false,
                "keep the colon: it is part of the summary"
            )
        );
        assert_eq!(
            parse_header("chore: bump")?,
            header("chore", None, false, "bump")
        );
        assert_eq!(
            parse_header("feat!: rename")?,
            header("feat", None, true, "rename")
        );
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn headers_with_the_wrong_shape_do_not_parse() {
        for bad in [
            "",
            "Added stuff",
            "feat",
            "feat:x",
            "feat:  two spaces",
            "feat: ",
            "feat:",
            "feat(): empty scope",
            "feat(xtask: unclosed",
            "feat (xtask): space before the scope",
            "feat !: space before the bang",
            "feat!(xtask): bang before the scope",
            "feat(xtask)(gate): two scopes",
            "feat(xtask)x: junk after the scope",
            ": no type",
            "(xtask): no type",
            "fe at: a space in the type",
            "fixup! feat(xtask): x",
            "squash! feat(xtask): x",
            "Revert \"feat(xtask): x\"",
            "Merge branch 'main' into phase/P00-x",
        ] {
            assert!(parse_header(bad).is_err(), "{bad:?} parsed");
        }
    }

    // AC: TU-00.4
    #[test]
    fn autosquash_and_revert_headers_say_what_to_do() {
        let fixup = parse_header("fixup! feat(xtask): x")
            .err()
            .unwrap_or_default();
        assert!(fixup.contains("fixup!"), "{fixup}");
        assert!(fixup.contains("squash"), "{fixup}");
        let revert = parse_header("Revert \"feat(xtask): x\"")
            .err()
            .unwrap_or_default();
        assert!(revert.contains("revert("), "{revert}");
    }

    // AC: TU-00.4
    #[test]
    fn rejects_unknown_type() -> TestResult {
        let problems = header_problems(&parse_header("feature(xtask): add a thing")?);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`feature`"), "{problems:?}");
        let problems = header_problems(&parse_header("Fix: add a thing")?);
        assert!(has_problem(&problems, "`Fix`"), "{problems:?}");
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn rejects_unknown_scope() -> TestResult {
        let problems = header_problems(&parse_header("feat(nope): add a thing")?);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`nope`"), "{problems:?}");
        let problems = header_problems(&parse_header("feat(xtask,gate): add a thing")?);
        assert!(has_problem(&problems, "`xtask,gate`"), "{problems:?}");
        Ok(())
    }

    #[test]
    fn every_listed_type_and_scope_is_allowed() -> TestResult {
        for kind in TYPES {
            assert_eq!(
                header_problems(&parse_header(&format!("{kind}: x"))?),
                Vec::<String>::new(),
                "{kind}"
            );
            for scope in SCOPES {
                let line = format!("{kind}({scope})!: x");
                assert_eq!(
                    header_problems(&parse_header(&line)?),
                    Vec::<String>::new(),
                    "{line}"
                );
            }
        }
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn summary_of_72_chars_passes_73_fails() -> TestResult {
        let line = |summary: &str| format!("feat(xtask): {summary}");
        let at_limit = parse_header(&line(&"a".repeat(72)))?;
        assert_eq!(header_problems(&at_limit), Vec::<String>::new());
        let over = header_problems(&parse_header(&line(&"a".repeat(73)))?);
        assert_eq!(over.len(), 1, "{over:?}");
        assert!(over[0].contains("73"), "{over:?}");
        // Characters, not bytes: 72 two-byte characters fit, 73 do not.
        let wide_ok = parse_header(&line(&"é".repeat(72)))?;
        assert_eq!(header_problems(&wide_ok), Vec::<String>::new());
        let wide_over = parse_header(&line(&"é".repeat(73)))?;
        assert_eq!(header_problems(&wide_over).len(), 1);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn rejects_uppercase_start_and_full_stop() -> TestResult {
        let upper = header_problems(&parse_header("feat(xtask): Add a thing")?);
        assert_eq!(upper.len(), 1, "{upper:?}");
        assert!(upper[0].contains("upper-case"), "{upper:?}");
        let accented = header_problems(&parse_header("feat(xtask): Écrire un test")?);
        assert!(has_problem(&accented, "upper-case"), "{accented:?}");
        let stop = header_problems(&parse_header("feat(xtask): add a thing.")?);
        assert_eq!(stop.len(), 1, "{stop:?}");
        assert!(stop[0].contains("full stop"), "{stop:?}");
        let both = header_problems(&parse_header("feat(xtask): Add a thing.")?);
        assert_eq!(both.len(), 2, "{both:?}");
        for fine in [
            "feat: 3 new checks",
            "feat: `code` first",
            "feat: add a thing..x",
        ] {
            assert_eq!(
                header_problems(&parse_header(fine)?),
                Vec::<String>::new(),
                "{fine}"
            );
        }
        Ok(())
    }

    #[test]
    fn the_wording_rules_are_what_header_problems_applies_to_the_summary() {
        assert_eq!(wording_problems("add a thing"), Vec::<String>::new());
        let upper = wording_problems("Add a thing");
        assert!(has_problem(&upper, "upper-case"), "{upper:?}");
        let stop = wording_problems("add a thing. ");
        assert!(has_problem(&stop, "full stop"), "{stop:?}");
        assert_eq!(wording_problems("Add a thing.").len(), 2);
    }

    // AC: TU-00.4
    #[test]
    fn crlf_message_parses_like_lf() -> TestResult {
        let lf = "feat(xtask)!: drop the flag\n\nThe flag was never used.\nSecond line.\n\n\
                  BREAKING CHANGE: the flag is gone\nRefs: TU-00.10\nPhase: P00\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(parse_message(&crlf)?, parse_message(lf)?);
        let message = parse_message(&crlf)?;
        assert_eq!(message.body, ["The flag was never used.", "Second line."]);
        assert_eq!(message.footers.len(), 3);
        assert!(
            message
                .footers
                .iter()
                .all(|(key, value)| !key.contains('\r') && !value.contains('\r')),
            "{message:?}"
        );
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_blank_line_must_follow_the_header() -> TestResult {
        let error = parse_message("feat: x\nbody right after the header")
            .err()
            .unwrap_or_default();
        assert!(error.contains("blank line"), "{error}");
        assert!(parse_message("feat: x\nPhase: P00").is_err());
        // Nothing after the header needs no blank line.
        for alone in ["feat: x", "feat: x\n", "feat: x\n\n", "feat: x\r\n"] {
            let message = parse_message(alone)?;
            assert!(
                message.body.is_empty() && message.footers.is_empty(),
                "{alone:?}"
            );
        }
        assert!(parse_message("Added stuff\n\nbody").is_err());
        assert!(parse_message("").is_err());
        Ok(())
    }

    #[test]
    fn a_paragraph_shaped_like_a_footer_starts_the_footers() -> TestResult {
        let both =
            parse_message("feat: x\n\nFirst paragraph\nstill first.\n\nSecond.\n\nRefs: TU-00.1")?;
        assert_eq!(
            both.body,
            ["First paragraph", "still first.", "", "Second."]
        );
        assert_eq!(both.footers, [("Refs".to_owned(), "TU-00.1".to_owned())]);

        // git-conventional's grammar: a paragraph that starts like a footer starts the footers,
        // and the lines after it continue its value.
        let shaped = parse_message("feat: x\n\nWhy: no footers here, a single sentence.\n\nMore.")?;
        assert_eq!(shaped.body, Vec::<String>::new());
        assert_eq!(
            shaped.footers,
            [(
                "Why".to_owned(),
                "no footers here, a single sentence.\n\nMore.".to_owned()
            )]
        );

        // The last paragraph is the footers, even when it is the only one.
        let only_footers = parse_message("feat: x\n\nPhase: P00\nRefs: TU-00.1, TU-00.2")?;
        assert_eq!(only_footers.body, Vec::<String>::new());
        assert_eq!(
            only_footers.footers,
            [
                ("Phase".to_owned(), "P00".to_owned()),
                ("Refs".to_owned(), "TU-00.1, TU-00.2".to_owned()),
            ]
        );

        // Trailing blank lines do not count.
        let padded = parse_message("feat: x\n\nbody\n\nPhase: P00\n\n\n")?;
        assert_eq!(padded.body, ["body"]);
        assert_eq!(padded.footers.len(), 1);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn the_rules_still_judge_a_message_whose_first_paragraph_looks_like_a_footer() -> TestResult {
        let text = "feat: x

Why: a sentence.

More.
Phase: P01
Refs: TU-99.1";
        let found = problems(text, "phase/P00-x", &["TU-00.1"])?;
        assert!(has_problem(&found, "Phase: P01"), "{found:?}");
        assert!(has_problem(&found, "TU-99.1"), "{found:?}");
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn multi_line_footer_joins() -> TestResult {
        let message = parse_message(
            "feat(cli)!: rename the flag\n\nWhy.\n\nBREAKING CHANGE: `--old` is now `--new`;\n\
             scripts that pass `--old` must change.\nRefs: TU-00.1\nPhase: P00",
        )?;
        assert_eq!(
            message.footers,
            [
                (
                    "BREAKING CHANGE".to_owned(),
                    "`--old` is now `--new`;\nscripts that pass `--old` must change.".to_owned()
                ),
                ("Refs".to_owned(), "TU-00.1".to_owned()),
                ("Phase".to_owned(), "P00".to_owned()),
            ]
        );
        assert_eq!(message.body, ["Why."]);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn bang_requires_breaking_footer_and_back() -> TestResult {
        let bang_only = problems("feat(cli)!: rename the flag\n\nPhase: P00", "main", &[])?;
        assert_eq!(bang_only.len(), 1, "{bang_only:?}");
        assert!(bang_only[0].contains("BREAKING CHANGE"), "{bang_only:?}");

        let footer_only = problems(
            "feat(cli): rename the flag\n\nBREAKING CHANGE: it is renamed",
            "main",
            &[],
        )?;
        assert_eq!(footer_only.len(), 1, "{footer_only:?}");
        assert!(footer_only[0].contains('!'), "{footer_only:?}");

        let both = problems(
            "feat(cli)!: rename the flag\n\nBREAKING CHANGE: it is renamed",
            "main",
            &[],
        )?;
        assert_eq!(both, Vec::<String>::new());
        let neither = problems("feat(cli): rename the flag", "main", &[])?;
        assert_eq!(neither, Vec::<String>::new());
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn both_breaking_spellings_count() -> TestResult {
        for key in ["BREAKING CHANGE", "BREAKING-CHANGE"] {
            let text = format!("feat(cli)!: rename the flag\n\n{key}: it is renamed");
            assert_eq!(problems(&text, "main", &[])?, Vec::<String>::new(), "{key}");
            let without_bang = format!("feat(cli): rename the flag\n\n{key}: it is renamed");
            assert_eq!(problems(&without_bang, "main", &[])?.len(), 1, "{key}");
        }
        // Other spellings are ordinary footers.
        let lower = problems(
            "feat(cli): rename the flag\n\nBreaking change: it is renamed",
            "main",
            &[],
        )?;
        assert_eq!(lower, Vec::<String>::new());
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn long_body_line_fails_but_url_line_passes() -> TestResult {
        let at_limit = "word ".repeat(19) + "word";
        assert_eq!(at_limit.chars().count(), 99);
        let exact = format!("{at_limit}x");
        assert_eq!(exact.chars().count(), 100);
        let over = format!("{exact}y");
        let url = format!("https://example.com/{}", "a".repeat(150));

        let text = |line: &str| format!("feat: x\n\n{line}\n\nPhase: P00");
        assert_eq!(problems(&text(&exact), "main", &[])?, Vec::<String>::new());
        assert_eq!(problems(&text(&url), "main", &[])?, Vec::<String>::new());
        let failing = problems(&text(&over), "main", &[])?;
        assert_eq!(failing.len(), 1, "{failing:?}");
        assert!(failing[0].contains("101"), "{failing:?}");

        // Characters, not bytes.
        let wide = "é ".repeat(50);
        assert_eq!(wide.chars().count(), 100);
        assert_eq!(problems(&text(&wide), "main", &[])?, Vec::<String>::new());
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn footer_lines_are_held_to_the_same_limit() -> TestResult {
        let long = "words ".repeat(20);
        let footer = format!("feat: x\n\nPhase: P00\nCo-Authored-By: {long}");
        let found = problems(&footer, "main", &[])?;
        assert_eq!(found.len(), 1, "{found:?}");
        let continued = format!("feat: x\n\nCo-Authored-By: fine\n{long}\nPhase: P00");
        let found = problems(&continued, "main", &[])?;
        assert_eq!(found.len(), 1, "{found:?}");
        // The first line of a footer counts with its key.
        let key_pushes_it_over = format!("feat: x\n\nNote: {}", "w ".repeat(48));
        assert_eq!(key_pushes_it_over.lines().last().map(str::len), Some(102));
        assert_eq!(problems(&key_pushes_it_over, "main", &[])?.len(), 1);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn phase_footer_must_match_a_phase_branch() -> TestResult {
        let on = |footers: &str| problems(&format!("feat: x\n\n{footers}"), "phase/P00-x", &[]);
        assert_eq!(on("Phase: P00")?, Vec::<String>::new());
        let missing = on("Refs: TU-00.1")?;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("P00"), "{missing:?}");
        let no_footers = problems("feat: x", "phase/P00-x", &[])?;
        assert!(has_problem(&no_footers, "Phase: P00"), "{no_footers:?}");
        let other = on("Phase: P01")?;
        assert_eq!(other.len(), 1, "{other:?}");
        assert!(
            other[0].contains("P01") && other[0].contains("P00"),
            "{other:?}"
        );
        let bad = on("Phase: zero")?;
        assert!(has_problem(&bad, "zero"), "{bad:?}");
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn phase_footer_is_optional_but_valid_elsewhere() -> TestResult {
        for branch in [
            "main",
            "fix/x",
            "gate/x",
            "docs/x",
            "spec/P00-x",
            "feature/x",
        ] {
            assert_eq!(
                problems("feat: x", branch, &[])?,
                Vec::<String>::new(),
                "{branch}"
            );
            assert_eq!(
                problems("feat: x\n\nPhase: P07", branch, &[])?,
                Vec::<String>::new(),
                "{branch}"
            );
            let bad = problems("feat: x\n\nPhase: P7", branch, &[])?;
            assert_eq!(bad.len(), 1, "{branch}: {bad:?}");
        }
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn refs_must_all_be_known() -> TestResult {
        let acs = ["TU-00.1", "TU-00.10", "TU-01.1"];
        let refs = |value: &str| problems(&format!("feat: x\n\nRefs: {value}"), "main", &acs);
        assert_eq!(refs("TU-00.1")?, Vec::<String>::new());
        assert_eq!(refs("TU-00.1, TU-00.10,TU-01.1")?, Vec::<String>::new());
        let unknown = refs("TU-00.1, TU-99.1")?;
        assert_eq!(unknown.len(), 1, "{unknown:?}");
        assert!(unknown[0].contains("TU-99.1"), "{unknown:?}");
        assert_eq!(refs("TU-98.1, TU-99.1")?.len(), 2);
        assert_eq!(refs("TU-00.1,")?.len(), 1);
        assert_eq!(refs("TU-00.1,,TU-00.10")?.len(), 1);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn refs_without_a_list_only_have_to_look_like_ids() -> TestResult {
        let refs = |value: &str| problems(&format!("feat: x\n\nRefs: {value}"), "main", &[]);
        assert_eq!(refs("TU-00.10, TU-01.1, TU-12.345")?, Vec::<String>::new());
        for bad in [
            "TU-00", "TU-.1", "TU-00.", "ce-08.1", "TU-00.1a", "08.1", "TU-0 0.1", "nope",
        ] {
            assert_eq!(refs(bad)?.len(), 1, "{bad}");
        }
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn the_bang_comes_from_the_header_not_from_the_footer() -> TestResult {
        let text = "feat(cli): rename the flag

BREAKING CHANGE: it is renamed";
        let message = parse_message(text)?;
        assert!(!message.header.breaking);
        let found = problems(text, "main", &[])?;
        assert!(has_problem(&found, "no `!`"), "{found:?}");
        Ok(())
    }

    #[test]
    fn a_good_message_has_no_problems() -> TestResult {
        let text = "feat(xtask): check commit messages against contributing rules\n\n\
                    Every commit of a pull request is parsed and judged.\n\n\
                    Refs: TU-00.10\nPhase: P00\n\
                    Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>\n\
                    Claude-Session: 666a12cd-ea1d-44dd-bf09-077295de681d";
        assert_eq!(
            problems(text, "phase/P00-foundation-and-gate", &["TU-00.10"])?,
            Vec::<String>::new()
        );
        Ok(())
    }
}
