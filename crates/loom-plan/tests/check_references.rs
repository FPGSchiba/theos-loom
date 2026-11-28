//! Behaviour tests of `check`: the registry, `broken-reference` and `duplicate-id`.

use std::fs;
use std::path::{Path, PathBuf};

use loom_plan::graph::{DependencyGraph, NodeId};
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
fn two_leaf_files_with_one_id_are_reported() -> TestResult {
    let finding = only_error("duplicate-id-leaf-file")?;
    assert_finding(
        &finding,
        Rule::DuplicateId,
        &format!("{CART}/tickets/story-remove-item.md"),
        &["`1`", "story-add-item.md", "story-remove-item.md"],
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
fn duplicate_criterion_number_across_a_split_list_is_reported() -> TestResult {
    let finding = only_error("duplicate-id-criterion-split")?;
    assert_finding(&finding, Rule::DuplicateId, STORY, &["criterion", "1"]);
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
        (format!("{checkout}/tickets/tickets.toml"), "the epic `9`"),
        (
            "components/app/initiative-shop/tickets/tickets.toml".to_owned(),
            "the initiative `5`",
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

fn errors_of(root: &Path) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    Ok(check(&read(root)?)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect())
}

/// A copy of `cases/<name>` in a temporary folder.
fn case_copy(name: &str) -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("repo");
    copy_dir(&fixture(&format!("cases/{name}")), &root)?;
    Ok((dir, root))
}

// AC: TU-01.2
#[test]
fn after_names_a_sibling_file_a_tracker_id_and_an_epic_table_slug() -> TestResult {
    let root = fixture("cases/after-names");
    let errors = errors_of(&root)?;
    assert!(errors.is_empty(), "{errors:#?}");
    let graph = DependencyGraph::of(&read(&root)?);
    let waits = |key: &str| graph.waits_for(&NodeId::new(key));
    // `story-pay-by-card` and `story-pay-by-card.md`, both the file of APP-3.
    assert_eq!(waits("APP-2"), [NodeId::new("APP-3")]);
    // `SHOP-77`, the tracker_id of APP-3.
    assert_eq!(waits("APP-4"), [NodeId::new("APP-3")]);
    // `{ epic = "epic-cart", needs = .. }`: the table names the epic by slug.
    assert_eq!(waits("APP-5"), [NodeId::new("APP-1")]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn after_naming_nothing_is_reported() -> TestResult {
    let (_dir, root) = case_copy("after-names")?;
    edit(
        &root,
        &format!("{CART}/tickets/story-empty-cart.md"),
        "after: [SHOP-77]",
        "after: [SHOP-78]",
    )?;
    edit(
        &root,
        "components/app/initiative-checkout/tickets/tickets.toml",
        "epic = \"epic-cart\"",
        "epic = \"basket\"",
    )?;
    let errors = errors_of(&root)?;
    assert_eq!(errors.len(), 2, "{errors:#?}");
    let story = errors
        .iter()
        .find(|f| f.path == format!("{CART}/tickets/story-empty-cart.md"))
        .ok_or("no finding on the story")?;
    assert_finding(
        story,
        Rule::BrokenReference,
        &story.path,
        &["`SHOP-78`", "tracker_id", "file name"],
    );
    let table = errors
        .iter()
        .find(|f| f.path == "components/app/initiative-checkout/tickets/tickets.toml")
        .ok_or("no finding on the table")?;
    assert_finding(
        table,
        Rule::BrokenReference,
        &table.path,
        &["`basket`", "slug"],
    );
    Ok(())
}

// AC: TU-01.2
#[test]
fn backlog_plan_joins_its_bug_by_file_stem() -> TestResult {
    let root = fixture("cases/backlog-plan");
    let repo = read(&root)?;
    let bug = repo
        .components
        .first()
        .and_then(|c| c.backlog.first())
        .ok_or("no backlog bug")?;
    let plan = bug.plan.as_ref().ok_or("the plan did not join")?;
    assert_eq!(
        plan.path,
        "components/app/backlog/bug-total-rounding-plan.md"
    );
    assert!(repo.stray_plans.is_empty());
    let errors = errors_of(&root)?;
    assert!(errors.is_empty(), "{errors:#?}");

    let (_dir, copy) = case_copy("backlog-plan")?;
    edit(
        &copy,
        "components/app/backlog/bug-total-rounding-plan.md",
        "ticket: bug-total-rounding",
        "ticket: bug-nope",
    )?;
    let errors = errors_of(&copy)?;
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_finding(
        &errors[0],
        Rule::BrokenReference,
        "components/app/backlog/bug-total-rounding-plan.md",
        &["`bug-nope`", "ticket"],
    );
    Ok(())
}

const EMPTY_CART: &str =
    "components/app/initiative-checkout/tickets/epic-cart/tickets/story-empty-cart.md";
const PAY_BY_CARD: &str =
    "components/app/initiative-checkout/tickets/epic-cart/tickets/story-pay-by-card.md";

/// The errors of `root` and what APP-4 and APP-5 wait for.
type Outcome = (Vec<Finding>, Vec<NodeId>, Vec<NodeId>);

fn outcome(root: &Path) -> Result<Outcome, Box<dyn std::error::Error>> {
    let graph = DependencyGraph::of(&read(root)?);
    Ok((
        errors_of(root)?,
        graph.waits_for(&NodeId::new("APP-4")),
        graph.waits_for(&NodeId::new("APP-5")),
    ))
}

// AC: TU-01.2
#[test]
fn a_mistyped_number_never_lands_on_a_numeric_tracker_id() -> TestResult {
    let (_dir, root) = case_copy("after-names")?;
    edit(
        &root,
        PAY_BY_CARD,
        "tracker_id: SHOP-77",
        "tracker_id: \"4\"",
    )?;
    edit(&root, EMPTY_CART, "after: [SHOP-77]", "after: [4]")?;
    let (errors, app4, _) = outcome(&root)?;
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_finding(
        &errors[0],
        Rule::BrokenReference,
        EMPTY_CART,
        &["`4`", "story"],
    );
    assert!(app4.is_empty(), "no edge for a broken reference: {app4:?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn a_missing_entry_of_a_known_epic_is_not_tried_as_a_name() -> TestResult {
    let (_dir, root) = case_copy("after-names")?;
    edit(
        &root,
        PAY_BY_CARD,
        "tracker_id: SHOP-77",
        "tracker_id: \"1.9\"",
    )?;
    edit(&root, EMPTY_CART, "after: [SHOP-77]", "after: [\"1.9\"]")?;
    let (errors, app4, _) = outcome(&root)?;
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_finding(
        &errors[0],
        Rule::BrokenReference,
        EMPTY_CART,
        &["`1.9`", "epic-cart has no story"],
    );
    assert!(app4.is_empty(), "{app4:?}");
    Ok(())
}

// AC: TU-01.2
#[test]
fn an_epic_table_waits_on_epics_only_while_the_epic_file_may_name_a_story() -> TestResult {
    let (_dir, root) = case_copy("after-names")?;
    let initiative = "components/app/initiative-checkout/tickets/tickets.toml";
    edit(&root, initiative, "epic = \"epic-cart\"", "epic = \"1.2\"")?;
    edit(
        &root,
        "components/app/initiative-checkout/tickets/epic-search/epic-search.md",
        "key: APP-5",
        "key: APP-5\nafter: [\"1.2\"]",
    )?;
    let (errors, _, app5) = outcome(&root)?;
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_finding(
        &errors[0],
        Rule::BrokenReference,
        initiative,
        &["`1.2`", "epic-search", "epics only"],
    );
    // The file's `after: ["1.2"]` is the story APP-3; the table's is no edge.
    assert_eq!(app5, [NodeId::new("APP-3")]);
    Ok(())
}
