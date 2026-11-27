//! The umbrella spec (`docs/specs/00-umbrella.md`): the phases, the repository each one is built
//! in and the acceptance criteria each one has.
//!
//! The phase table of §3 gives the name and the repository of every phase. The acceptance
//! criteria are the list items that start with an id (`- CE-08.6 …`) under the heading
//! `### PNN · Name` of their phase; an id is `CE-`, two digits, an optional capital letter, a
//! dot and a number. A list item that starts `- CE-` anywhere else in a phase section must be
//! such an id, so a typo in the spec is an error and not a criterion that silently does not
//! exist. `ac-trace` and `commit-msg` both read their ids from here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::branch::Phase;
use crate::markdown;
use crate::{Error, Result};

/// Where the umbrella spec is, relative to the repository root.
pub const PATH: &str = "docs/specs/00-umbrella.md";

/// One phase of the build.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PhaseInfo {
    /// The phase number.
    pub id: Phase,
    /// The name in the phase table, such as `Foundation and gate`.
    pub name: String,
    /// The repository the phase is built in, such as `theos-editor` or `tower-of-theos`.
    pub repo: String,
    /// The acceptance criteria of the phase, in the order the spec lists them.
    pub acs: Vec<String>,
}

/// The phases of the umbrella spec.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Umbrella {
    phases: Vec<PhaseInfo>,
}

impl Umbrella {
    /// Every acceptance criterion of every phase.
    #[must_use]
    pub fn all_ids(&self) -> BTreeSet<String> {
        self.phases
            .iter()
            .flat_map(|phase| phase.acs.iter().cloned())
            .collect()
    }

    /// The phase `phase`, or `None` when the spec has no such phase.
    #[must_use]
    pub fn phase(&self, phase: Phase) -> Option<&PhaseInfo> {
        self.phases.iter().find(|info| info.id == phase)
    }
}

/// Reads and parses the umbrella spec of the repository at `root`.
///
/// # Errors
///
/// Fails when the file cannot be read or does not parse, and with [`Error::Config`] when it lists
/// no acceptance criterion at all: every id would then be unknown.
pub fn load(root: &Path) -> Result<Umbrella> {
    let path = root.join(PATH);
    let text = fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
    let umbrella = parse(&text)?;
    if umbrella.all_ids().is_empty() {
        return Err(Error::Config {
            file: PATH.to_owned(),
            message: "lists no acceptance criteria (`- CE-NN.N …` items under `### PNN ·` \
                      headings)"
                .to_owned(),
        });
    }
    Ok(umbrella)
}

/// Parses the text of the umbrella spec.
///
/// # Errors
///
/// Fails, naming the line, when the phase table has no `Name` and `Repo` column, a row of it is
/// too short, names a phase twice or has a control character or line break (a lone CR, for
/// instance) in its name or repository, a phase has two sections, a section has no row in the
/// table, an item starting `- CE-` does not start with an id, or an id is listed twice.
pub fn parse(text: &str) -> Result<Umbrella> {
    reject_lone_cr(text)?;
    let rows = phase_rows(text)?;
    let mut sections: BTreeMap<Phase, Vec<String>> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for section in markdown::sections(text, 3) {
        let Some(phase) = section_phase(&section.heading.text) else {
            continue;
        };
        if sections.insert(phase, Vec::new()).is_some() {
            return Err(error(format!(
                "{phase} has two sections (line {})",
                section.heading.line
            )));
        }
        let first = body_line(text, &section);
        for item in markdown::list_items(section.body) {
            if !item.text.starts_with("CE-") {
                continue;
            }
            let number = first + item.line - 1;
            let id = item.text.split_whitespace().next().unwrap_or_default();
            if !is_ac_id(id) {
                return Err(error(format!(
                    "line {number} is a list item that starts with `- CE-` but not with an \
                     acceptance criterion id (`CE-NN.N`, `CE-NNR.N`)"
                )));
            }
            if !seen.insert(id.to_owned()) {
                return Err(error(format!("{id} is listed twice (line {number})")));
            }
            sections.entry(phase).or_default().push(id.to_owned());
        }
    }

    let mut phases = Vec::with_capacity(rows.len());
    for (id, name, repo) in rows {
        let acs = sections.remove(&id).unwrap_or_default();
        phases.push(PhaseInfo {
            id,
            name,
            repo,
            acs,
        });
    }
    if let Some(phase) = sections.keys().next() {
        return Err(error(format!(
            "{phase} has a section but no row in the phase table"
        )));
    }
    Ok(Umbrella { phases })
}

/// A CR that does not end a line (CRLF does) is a line break for the Markdown parser but not for
/// `str::lines`, so a cell could hide text from one reader and show it to another. The umbrella is
/// part of a pull request: refuse it, naming the line.
fn reject_lone_cr(text: &str) -> Result<()> {
    let bytes = text.as_bytes();
    let lone = bytes
        .iter()
        .enumerate()
        .find(|(index, byte)| **byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'));
    match lone {
        Some((offset, _)) => Err(error(format!(
            "line {} has a carriage return that does not end a line",
            markdown::line_of(text, offset)
        ))),
        None => Ok(()),
    }
}

/// The line the body of `section` starts on. The body is a slice of `text`, so its place is the
/// difference of the two addresses.
fn body_line(text: &str, section: &markdown::Section<'_>) -> u32 {
    let start = section
        .body
        .as_ptr()
        .addr()
        .wrapping_sub(text.as_ptr().addr());
    markdown::line_of(text, start)
}

/// The `(phase, name, repo)` of every row of the phase table: the first table with the columns
/// `Phase`, `Name` and `Repo`.
fn phase_rows(text: &str) -> Result<Vec<(Phase, String, String)>> {
    let tables = markdown::tables(text);
    let Some((table, columns)) = tables
        .iter()
        .find_map(|table| Columns::of(&table.header).map(|columns| (table, columns)))
    else {
        return Err(error(
            "there is no phase table (a table with the columns `Phase`, `Name` and `Repo`)"
                .to_owned(),
        ));
    };
    let mut rows: Vec<(Phase, String, String)> = Vec::new();
    for (index, cells) in table.rows.iter().enumerate() {
        // The header is a line, then the delimiter row, then the rows.
        let number = table.line + 2 + u32::try_from(index).unwrap_or(u32::MAX);
        let Some(phase) = cells
            .get(columns.phase)
            .and_then(|cell| cell.parse::<Phase>().ok())
        else {
            continue;
        };
        // The parser pads a short row with empty cells, so a short row has an empty one.
        let (Some(name), Some(repo)) = (cells.get(columns.name), cells.get(columns.repo)) else {
            return Err(error(format!(
                "the row of {phase} in the phase table (line {number}) is too short"
            )));
        };
        if name.is_empty() || repo.is_empty() {
            return Err(error(format!(
                "the row of {phase} in the phase table (line {number}) is too short or has an \
                 empty name or repository"
            )));
        }
        // Names and repositories are written to the output of checks, and the umbrella is part
        // of a pull request. The line is named, the cell is not quoted.
        if !is_plain(name) || !is_plain(repo) {
            return Err(error(format!(
                "the name or the repository of {phase} in the phase table (line {number}) has a \
                 control character or a line break in it"
            )));
        }
        if rows.iter().any(|(known, _, _)| *known == phase) {
            return Err(error(format!(
                "{phase} is in the phase table twice (line {number})"
            )));
        }
        rows.push((phase, name.clone(), repo.clone()));
    }
    Ok(rows)
}

/// Where the columns the check needs are in the phase table.
struct Columns {
    phase: usize,
    name: usize,
    repo: usize,
}

impl Columns {
    fn of(header: &[String]) -> Option<Self> {
        let position = |title: &str| header.iter().position(|cell| cell == title);
        Some(Self {
            phase: position("Phase")?,
            name: position("Name")?,
            repo: position("Repo")?,
        })
    }
}

fn error(message: String) -> Error {
    Error::Parse {
        what: "the umbrella spec".to_owned(),
        message,
    }
}

/// The phase of a heading `PNN · Name`; `None` for any other heading.
fn section_phase(title: &str) -> Option<Phase> {
    let (word, rest) = title.split_once(' ')?;
    let phase = word.parse().ok()?;
    rest.trim_start().starts_with('·').then_some(phase)
}

/// Whether `text` has no control character and no Unicode line break. `str::lines` only ends a
/// line at LF, so a lone CR can be in the middle of a cell, and a reader that ends lines at CR
/// would see the text after it as a line of its own.
fn is_plain(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
}

/// `CE-`, two digits, an optional capital letter, a dot and at least one digit.
fn is_ac_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("CE-") else {
        return false;
    };
    let Some((group, number)) = rest.split_once('.') else {
        return false;
    };
    let group = group
        .strip_suffix(|c: char| c.is_ascii_uppercase())
        .unwrap_or(group);
    group.len() == 2
        && group.bytes().all(|b| b.is_ascii_digit())
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    const REAL: &str = include_str!("../../docs/specs/00-umbrella.md");

    fn phase(text: &str) -> TestResult<Phase> {
        Ok(text.parse()?)
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    /// A small umbrella: a table of two phases and the sections `body` holds.
    fn excerpt(body: &str) -> String {
        format!(
            "## 3. Phases\n\n\
             | Phase | Name | Repo | Slice | Depends on |\n\
             |---|---|---|---|---|\n\
             | P90 | Fixture phase | theos-editor | S2 | – |\n\
             | P91 | Elsewhere | tower-of-theos | S1 | – |\n\n\
             {body}"
        )
    }

    // AC: CE-08.6
    #[test]
    fn parses_phases_repos_and_ids() -> TestResult {
        let umbrella = parse(REAL)?;

        let p00 = umbrella.phase(phase("P00")?).ok_or("no P00")?;
        let expected: Vec<String> = (1..=12).map(|n| format!("CE-08.{n}")).collect();
        assert_eq!(p00.acs, expected);
        assert_eq!(p00.name, "Foundation and gate");
        assert_eq!(p00.repo, "theos-editor");

        let p05 = umbrella.phase(phase("P05")?).ok_or("no P05")?;
        assert!(p05.acs.iter().any(|id| id == "CE-17.1"), "{:?}", p05.acs);

        let p01 = umbrella.phase(phase("P01")?).ok_or("no P01")?;
        assert_eq!(p01.repo, "tower-of-theos");
        let p20 = umbrella.phase(phase("P20")?).ok_or("no P20")?;
        assert_eq!(p20.repo, "tower-of-theos");

        // The ids of the work outside the build (§4) are not criteria.
        assert!(!umbrella.all_ids().contains("CE-02"));
        assert!(umbrella.all_ids().contains("CE-06.1"));
        // A phase the table does not list has no entry.
        let listed = table_phases()?;
        let unlisted = (0..100)
            .map(|number| format!("P{number:02}"))
            .find(|candidate| candidate.parse().is_ok_and(|id| !listed.contains(&id)))
            .ok_or("every phase is listed")?;
        assert!(umbrella.phase(phase(&unlisted)?).is_none());
        Ok(())
    }

    /// The phases the real umbrella's phase table lists, read from the table itself.
    fn table_phases() -> TestResult<Vec<Phase>> {
        let table = markdown::tables(REAL)
            .into_iter()
            .find(|table| table.header.first().map(String::as_str) == Some("Phase"))
            .ok_or("no phase table")?;
        let phases: Vec<Phase> = table
            .rows
            .iter()
            .filter_map(|row| row.first()?.parse().ok())
            .collect();
        assert_ne!(phases.len(), 0);
        Ok(phases)
    }

    #[test]
    fn every_phase_of_the_real_umbrella_has_criteria() -> TestResult {
        let umbrella = parse(REAL)?;
        let listed = table_phases()?;
        for id in &listed {
            let info = umbrella.phase(*id).ok_or(format!("no {id}"))?;
            assert!(!info.acs.is_empty(), "{id} has no criteria");
        }
        // Nothing but the table's rows is a phase.
        assert_eq!(umbrella.phases.len(), listed.len());
        Ok(())
    }

    #[test]
    fn a_criterion_that_spans_two_lines_is_one_id() -> TestResult {
        let text = excerpt(
            "### P90 · Fixture phase\n\n\
             - CE-90.1 The first criterion goes on\n  \
             CE-90.9 over a second line and mentions an id.\n\
             - CE-90.2 The second.\n",
        );
        let umbrella = parse(&text)?;
        let info = umbrella.phase(phase("P90")?).ok_or("no P90")?;
        assert_eq!(info.acs, ids(&["CE-90.1", "CE-90.2"]));
        Ok(())
    }

    #[test]
    fn a_level_four_heading_stays_inside_its_phase() -> TestResult {
        let text = excerpt(
            "### P90 · Fixture phase\n\
             - CE-90.1 Before.\n\
             #### Notes\n\
             - CE-90.2 After the deeper heading.\n\n\
             ### P91 · Elsewhere\n\
             - CE-91.1 Other.\n",
        );
        let umbrella = parse(&text)?;
        let p90 = umbrella.phase(phase("P90")?).ok_or("no P90")?;
        assert_eq!(p90.acs, ids(&["CE-90.1", "CE-90.2"]));
        Ok(())
    }

    #[test]
    fn criteria_belong_to_the_section_they_are_in() -> TestResult {
        let text = excerpt(
            "### P90 · Fixture phase\n\n\
             **CE-90 Group**\n\
             - CE-90.1 First.\n\
             - CE-90R.2 Second, with a letter.\n\
             #### A deeper heading stays in the section\n\
             - CE-90.3 Third.\n\n\
             ### P91 · Elsewhere (tower-of-theos)\n\
             - CE-91.1 Other.\n\n\
             ## 4. Work outside the build\n\
             - CE-92.1 Not in any phase.\n",
        );
        let umbrella = parse(&text)?;
        let p90 = umbrella.phase(phase("P90")?).ok_or("no P90")?;
        assert_eq!(p90.acs, ids(&["CE-90.1", "CE-90R.2", "CE-90.3"]));
        let p91 = umbrella.phase(phase("P91")?).ok_or("no P91")?;
        assert_eq!(p91.acs, ids(&["CE-91.1"]));
        assert_eq!(
            umbrella.all_ids().into_iter().collect::<Vec<_>>(),
            ids(&["CE-90.1", "CE-90.3", "CE-90R.2", "CE-91.1"])
        );
        Ok(())
    }

    #[test]
    fn table_columns_may_come_in_any_order() -> TestResult {
        let text = "| Phase | Repo | Name |\n|---|---|---|\n| P90 | theos-editor | A phase |\n";
        let umbrella = parse(text)?;
        let info = umbrella.phase(phase("P90")?).ok_or("no P90")?;
        assert_eq!(
            (info.name.as_str(), info.repo.as_str()),
            ("A phase", "theos-editor")
        );
        Ok(())
    }

    #[test]
    fn a_list_item_that_is_not_an_id_is_an_error() {
        for item in [
            "CE-8.1 One digit.",
            "CE-081.1 Three digits.",
            "CE-90. None.",
            "CE-90.1: x",
        ] {
            let text = excerpt(&format!("### P90 · Fixture phase\n- {item}\n"));
            let result = parse(&text);
            assert!(
                matches!(result, Err(Error::Parse { .. })),
                "{item}: {result:?}"
            );
        }
    }

    #[test]
    fn a_broken_spec_is_an_error() {
        let two_sections = excerpt("### P90 · A\n- CE-90.1 x\n### P90 · B\n- CE-90.2 y\n");
        let no_row = excerpt("### P92 · No row\n- CE-92.1 x\n");
        let twice = excerpt("### P90 · A\n- CE-90.1 x\n- CE-90.1 y\n");
        let twice_elsewhere = excerpt("### P90 · A\n- CE-90.1 x\n### P91 · B\n- CE-90.1 y\n");
        let no_table = "### P90 · A\n- CE-90.1 x\n".to_owned();
        let no_repo = "| Phase | Name |\n|---|---|\n| P90 | A |\n".to_owned();
        let short_row = "| Phase | Name | Repo |\n|---|---|---|\n| P90 | A |\n".to_owned();
        let table_twice = excerpt("").replace(
            "| P91 |",
            "| P90 | Again | theos-editor | S2 | – |\n| P91 |",
        );
        for (name, text) in [
            ("two sections", two_sections),
            ("no row", no_row),
            ("id twice", twice),
            ("id in two phases", twice_elsewhere),
            ("no table", no_table),
            ("no repo column", no_repo),
            ("short row", short_row),
            ("phase twice in the table", table_twice),
        ] {
            let result = parse(&text);
            assert!(
                matches!(result, Err(Error::Parse { .. })),
                "{name}: {result:?}"
            );
        }
    }

    #[test]
    fn the_error_of_a_short_row_reads_as_one_line_of_prose() {
        let text = "| Phase | Name | Repo |\n|---|---|---|\n| P90 | A |\n";
        let message = match parse(text) {
            Err(error) => error.to_string(),
            Ok(_) => String::new(),
        };
        assert!(message.contains("empty name or repository"), "{message}");
        assert!(!message.contains("  "), "{message}");
    }

    // AC: CE-08.6
    #[test]
    fn control_characters_in_the_name_or_the_repository_are_an_error() {
        // `str::lines` leaves a lone CR (and the other breaks) in the middle of a cell.
        for hostile in [
            "\r", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}", "\t", "\u{0}",
        ] {
            let cells = [
                (format!("A{hostile}::error::x"), "theos-editor".to_owned()),
                (
                    "A".to_owned(),
                    format!("theos-editor{hostile}::stop-commands::x"),
                ),
            ];
            for (name, repo) in cells {
                let text =
                    format!("| Phase | Name | Repo |\n|---|---|---|\n| P90 | {name} | {repo} |\n");
                let result = parse(&text);
                // The error names the line and quotes nothing of the cell.
                assert!(
                    matches!(&result, Err(Error::Parse { message, .. })
                        if message.contains("line 3") && !message.contains("::")),
                    "{hostile:?}: {result:?}"
                );
            }
        }
    }

    #[test]
    fn names_and_repositories_may_use_any_printable_character() -> TestResult {
        let text =
            "| Phase | Name | Repo |\n|---|---|---|\n| P90 | Äußere Phase · ünï | théos-éditor |\n";
        let umbrella = parse(text)?;
        let info = umbrella.phase(phase("P90")?).ok_or("no P90")?;
        assert_eq!(info.name, "Äußere Phase · ünï");
        assert_eq!(info.repo, "théos-éditor");
        Ok(())
    }

    #[test]
    fn crlf_line_ends_parse_like_lf() -> TestResult {
        let text = excerpt("### P90 · A\n- CE-90.1 x\n").replace('\n', "\r\n");
        let umbrella = parse(&text)?;
        let info = umbrella.phase(phase("P91")?).ok_or("no P91")?;
        assert_eq!(info.repo, "tower-of-theos");
        assert_eq!(umbrella.all_ids().len(), 1);
        Ok(())
    }

    #[test]
    fn load_reads_the_umbrella_of_a_repository() -> TestResult {
        let dir = tempfile::tempdir()?;
        let result = load(dir.path());
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");

        let specs = dir.path().join("docs").join("specs");
        fs::create_dir_all(&specs)?;
        let file = specs.join("00-umbrella.md");
        fs::write(&file, excerpt("### P90 · A\n- CE-90.1 x\n"))?;
        assert!(load(dir.path())?.all_ids().contains("CE-90.1"));

        // With no criterion at all every id would be unknown, so that is a broken spec.
        fs::write(&file, excerpt("### P90 · A\nNothing yet.\n"))?;
        let result = load(dir.path());
        assert!(matches!(result, Err(Error::Config { .. })), "{result:?}");
        Ok(())
    }
}
