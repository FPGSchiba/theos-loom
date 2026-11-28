//! Behaviour tests of the unfinished-work rules: `epic-incomplete`, `story-without-acs` and
//! `missing-key`, errors when the item is ready and notes otherwise (decision 0022).

use std::path::{Path, PathBuf};

use loom_plan::{Finding, Rule, Severity, check, read};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn findings(tree: &str) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    Ok(check(&read(&fixture(tree))?))
}

fn of_severity(all: Vec<Finding>, severity: Severity) -> Vec<Finding> {
    all.into_iter().filter(|f| f.severity == severity).collect()
}

/// The one error `check` reports on `invalid/<name>`; fails when there are more or none.
fn only_error(name: &str) -> Result<Finding, Box<dyn std::error::Error>> {
    let mut errors = of_severity(findings(&format!("invalid/{name}"))?, Severity::Error);
    assert_eq!(errors.len(), 1, "{name}: {errors:#?}");
    errors.pop().ok_or_else(|| "no error".into())
}

/// Asserts `finding` is of `rule` and its message contains every one of `words`.
fn assert_finding(finding: &Finding, rule: Rule, words: &[&str]) {
    assert_eq!(finding.rule, rule, "{finding:#?}");
    for word in words {
        assert!(
            finding.message.contains(word),
            "{word:?} missing from {finding:#?}"
        );
    }
}

// AC: TU-01.2
#[test]
fn ready_epic_without_goal_is_an_error() -> TestResult {
    let finding = only_error("epic-incomplete-no-goal")?;
    assert_finding(&finding, Rule::EpicIncomplete, &["goal", "## Description"]);
    assert!(finding.path.ends_with("epic-cart.md"), "{finding:#?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn ready_epic_without_scope_is_an_error() -> TestResult {
    let finding = only_error("epic-incomplete-no-scope")?;
    assert_finding(&finding, Rule::EpicIncomplete, &["scope", "## Boundaries"]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn ready_epic_without_requirement_lines_is_an_error() -> TestResult {
    let finding = only_error("epic-incomplete-no-requirements")?;
    assert_finding(
        &finding,
        Rule::EpicIncomplete,
        &["requirement", "## Requirements"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn story_without_acs_in_a_ready_epic_is_an_error() -> TestResult {
    let finding = only_error("story-without-acs-file")?;
    assert_finding(&finding, Rule::StoryWithoutAcs, &["criteria"]);
    assert!(finding.path.ends_with("story-add-item.md"), "{finding:#?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn entry_only_story_in_a_ready_epic_is_an_error() -> TestResult {
    let finding = only_error("story-without-acs-entry")?;
    assert_finding(&finding, Rule::StoryWithoutAcs, &["criteria"]);
    assert!(finding.path.ends_with("tickets.toml"), "{finding:#?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn ready_backlog_bug_without_criteria_is_an_error() -> TestResult {
    let finding = only_error("story-without-acs-backlog-bug")?;
    assert_finding(&finding, Rule::StoryWithoutAcs, &["criteria"]);
    assert!(
        finding.path.ends_with("bug-total-rounding.md"),
        "{finding:#?}"
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn ready_story_without_key_is_an_error() -> TestResult {
    let finding = only_error("missing-key-ready-story")?;
    assert_finding(&finding, Rule::MissingKey, &["key"]);
    assert!(finding.path.ends_with("story-add-item.md"), "{finding:#?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn epic_without_stories_is_valid() -> TestResult {
    let all = findings("cases/epic-without-stories")?;
    assert_eq!(all, Vec::<Finding>::new());
    Ok(())
}

// AC: TU-01.2
#[test]
fn unfinished_work_outside_ready_epics_gives_notes_only() -> TestResult {
    let all = findings("cases/unfinished-work-outside-ready")?;
    assert_eq!(of_severity(all.clone(), Severity::Error), []);
    let notes = of_severity(all, Severity::Note);
    for rule in [
        Rule::EpicIncomplete,
        Rule::StoryWithoutAcs,
        Rule::MissingKey,
    ] {
        assert!(
            notes.iter().any(|f| f.rule == rule),
            "no {rule} note in {notes:#?}"
        );
    }
    Ok(())
}

// AC: TU-01.2
#[test]
fn ready_epic_without_stories_still_needs_a_goal() -> TestResult {
    let finding = only_error("epic-incomplete-empty-no-goal")?;
    assert_finding(&finding, Rule::EpicIncomplete, &["goal"]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn ready_epic_without_stories_still_needs_a_key() -> TestResult {
    let finding = only_error("missing-key-empty-epic")?;
    assert_finding(&finding, Rule::MissingKey, &["epic", "key"]);
    assert!(finding.path.ends_with("epic-cart.md"), "{finding:#?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn story_ready_by_its_own_mark_in_a_draft_epic_is_an_error() -> TestResult {
    let all = findings("cases/ready-story-in-draft-epic")?;
    let errors = of_severity(all, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_finding(&errors[0], Rule::StoryWithoutAcs, &["criteria"]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn paused_epic_gives_notes_only() -> TestResult {
    let all = findings("cases/paused-epic")?;
    assert_eq!(of_severity(all.clone(), Severity::Error), []);
    let notes = of_severity(all, Severity::Note);
    assert_eq!(notes.len(), 3, "{notes:#?}");
    assert!(notes.iter().all(|f| f.rule == Rule::EpicIncomplete));
    Ok(())
}

// AC: TU-01.2
#[test]
fn dropped_ready_epic_gives_no_finding() -> TestResult {
    assert_eq!(findings("cases/dropped-ready-epic")?, Vec::<Finding>::new());
    Ok(())
}

// AC: TU-01.2
#[test]
fn dropped_ready_story_gives_no_finding() -> TestResult {
    assert_eq!(
        findings("cases/dropped-ready-story")?,
        Vec::<Finding>::new()
    );
    Ok(())
}
