//! Behaviour tests of `check`: the registry, `broken-reference` and `duplicate-id`.

use std::fs;
use std::path::{Path, PathBuf};

use loom_plan::{Finding, Rule, Severity, check, read};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CART: &str = "components/app/initiative-checkout/tickets/epic-cart";
const STORY: &str =
    "components/app/initiative-checkout/tickets/epic-cart/tickets/story-add-item.md";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Replaces `from` by `to` in the file at `path` below `root`; fails when `from` is not there.
fn edit(root: &Path, path: &str, from: &str, to: &str) -> TestResult {
    let file = root.join(path);
    let text = fs::read_to_string(&file)?;
    assert!(text.contains(from), "{from:?} not in {path}");
    fs::write(&file, text.replacen(from, to, 1))?;
    Ok(())
}

/// The one error `check` reports on `invalid/<name>`; fails when there are more or none.
fn only_error(name: &str) -> Result<Finding, Box<dyn std::error::Error>> {
    let repo = read(&fixture(&format!("invalid/{name}")))?;
    let mut errors: Vec<Finding> = check(&repo)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{name}: {errors:#?}");
    errors.pop().ok_or_else(|| "no error".into())
}

/// Asserts `finding` is `rule` at `path` and its message holds every word of `words`.
fn assert_finding(finding: &Finding, rule: Rule, path: &str, words: &[&str]) {
    assert_eq!(finding.rule, rule, "{finding:#?}");
    assert_eq!(finding.path, path, "{finding:#?}");
    for word in words {
        assert!(
            finding.message.contains(word),
            "{word:?} not in {:?}",
            finding.message
        );
    }
}

// AC: TU-01.2
#[test]
fn unknown_component_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-component")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        "_state/nope/ED-1.toml",
        &["`nope`", "component"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn unknown_epic_in_a_state_path_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-state-epic")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        "_state/app/APP-9.toml",
        &["`APP-9`", "epic"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn unknown_epic_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-epic")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        STORY,
        &["`epic-nope`", "epic", "after"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn unknown_story_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-story")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        STORY,
        &["`1.9`", "story", "after"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn unknown_key_in_needs_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-needs")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        STORY,
        &["`API-9`", "needs"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn unknown_requirement_in_covers_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-covers")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        STORY,
        &["`R7`", "covers", "epic-cart"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn unknown_parent_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-parent")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        STORY,
        &["`epic-basket`", "parent", "epic-cart"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn plan_naming_no_ticket_is_reported() -> TestResult {
    let finding = only_error("broken-reference-unknown-ticket")?;
    assert_finding(
        &finding,
        Rule::BrokenReference,
        &format!("{CART}/tickets/story-add-item-plan.md"),
        &["`7`", "ticket"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn duplicate_key_is_reported() -> TestResult {
    let finding = only_error("duplicate-id-key")?;
    assert_finding(
        &finding,
        Rule::DuplicateId,
        &format!("{CART}/tickets/story-remove-item.md"),
        &["`APP-2`", "story-add-item.md"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn duplicate_entry_id_is_reported() -> TestResult {
    let finding = only_error("duplicate-id-entry")?;
    assert_finding(
        &finding,
        Rule::DuplicateId,
        &format!("{CART}/tickets/tickets.toml"),
        &["`1`", "entry"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn duplicate_criterion_number_is_reported() -> TestResult {
    let finding = only_error("duplicate-id-criterion")?;
    assert_finding(&finding, Rule::DuplicateId, STORY, &["criterion", "2"]);
    assert!(finding.line.is_some(), "{finding:#?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn duplicate_decision_number_is_reported() -> TestResult {
    let finding = only_error("duplicate-id-decision")?;
    assert_finding(
        &finding,
        Rule::DuplicateId,
        "decisions/0001-use-postgres.md",
        &["0001", "0001-one-country.md"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn valid_tree_has_no_errors() -> TestResult {
    let repo = read(&fixture("valid"))?;
    let errors: Vec<Finding> = check(&repo)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    Ok(())
}

#[test]
fn check_keeps_read_findings_and_sorts_errors_first_then_by_path() -> TestResult {
    let repo = read(&fixture("invalid/drift-loom"))?;
    let findings = check(&repo);
    assert!(
        findings.iter().any(|f| f.rule == Rule::Drift),
        "{findings:#?}"
    );
    let order: Vec<(Severity, &str, Option<u32>)> = findings
        .iter()
        .map(|f| (f.severity, f.path.as_str(), f.line))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
    Ok(())
}

// AC: TU-01.2
#[test]
fn every_reference_form_is_checked() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("repo");
    copy_dir(&fixture("valid"), &root)?;
    let checkout = "components/app/initiative-checkout";
    edit(
        &root,
        &format!("{checkout}/tickets/tickets.toml"),
        "id = 2\nslug = \"epic-search\"",
        "id = 2\nslug = \"epic-search\"\nafter = [9]",
    )?;
    edit(
        &root,
        "components/app/initiative-shop/tickets/tickets.toml",
        "covers = [\"R1\"]",
        "covers = [\"R1\"]\nafter = [5]",
    )?;
    edit(
        &root,
        &format!("{checkout}/initiative-checkout.md"),
        "status: in-progress",
        "status: in-progress\nafter: [initiative-nope]",
    )?;
    edit(
        &root,
        &format!("{checkout}/tickets/epic-search/epic-search.md"),
        "covers: [\"R2\"]",
        "covers: [\"R9\"]",
    )?;
    edit(
        &root,
        "components/app/backlog/bug-total-rounding.md",
        "severity: P2",
        "severity: P2\nparent: epic-cart\nafter: [\"1.2\", epic-cart]",
    )?;
    edit(
        &root,
        "components/api/initiative-payments/tickets/story-charge-card.md",
        "covers: [\"R1\"]",
        "covers: [\"R1\"]\nafter: [\"5.1\"]",
    )?;
    edit(
        &root,
        "_state/app/APP-1.toml",
        "APP-3 = \"queued\"",
        "APP-3 = \"queued\"\nAPP-99 = \"queued\"",
    )?;
    let findings = check(&read(&root)?);
    let expected = [
        (format!("{checkout}/tickets/tickets.toml"), "epic id `9`"),
        (
            "components/app/initiative-shop/tickets/tickets.toml".to_owned(),
            "initiative id `5`",
        ),
        (
            format!("{checkout}/initiative-checkout.md"),
            "`initiative-nope`",
        ),
        (
            format!("{checkout}/tickets/epic-search/epic-search.md"),
            "`R9`",
        ),
        (
            "components/app/backlog/bug-total-rounding.md".to_owned(),
            "no parent container",
        ),
        (
            "components/app/backlog/bug-total-rounding.md".to_owned(),
            "`1.2`, but a backlog story",
        ),
        (
            "components/app/backlog/bug-total-rounding.md".to_owned(),
            "`epic-cart`, but a backlog story",
        ),
        (
            "components/api/initiative-payments/tickets/story-charge-card.md".to_owned(),
            "lists no epic with id `5`",
        ),
        ("_state/app/APP-1.toml".to_owned(), "`APP-99`"),
    ];
    let broken: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.rule == Rule::BrokenReference)
        .collect();
    for (path, words) in &expected {
        assert!(
            broken
                .iter()
                .any(|f| &f.path == path && f.message.contains(words)),
            "{path}: {words:?} not in {broken:#?}"
        );
    }
    assert_eq!(broken.len(), expected.len(), "{broken:#?}");
    Ok(())
}
